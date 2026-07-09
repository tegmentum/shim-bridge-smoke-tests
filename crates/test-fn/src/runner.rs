//! Actual case-execution loop for a single function.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};

use crate::{bridge_cache, db, status};

pub struct Args {
    pub interface: PathBuf,
    pub extension: String,
    pub function: String,
    pub case: Option<String>,
    pub bridge: PathBuf,
    pub provider: Option<PathBuf>,
    pub ducklink: PathBuf,
    pub force: bool,
    pub json: bool,
}

#[derive(Debug)]
struct CaseOutcome {
    case_name: String,
    status: String,
    actual: String,
    duration_ms: i64,
}

pub fn run(args: Args) -> Result<()> {
    let conn = db::open(&args.interface)?;
    let row = db::load_scalar(&conn, &args.extension, &args.function)?
        .ok_or_else(|| {
            anyhow!(
                "no scalar row for {}.{}",
                args.extension,
                args.function
            )
        })?;

    // §8 idempotency guard. Only kicks in when running the full
    // suite (i.e. no `--case`).
    if args.case.is_none() && !args.force && status::is_cache_hit(&row) {
        let sig8 = row
            .signature_hash
            .as_deref()
            .map(|s| &s[..s.len().min(8)])
            .unwrap_or("--");
        let ts = row.last_verified_at.as_deref().unwrap_or("?");
        if args.json {
            println!(
                "{}",
                serde_json::json!({
                    "summary": true,
                    "function": args.function,
                    "cached": true,
                    "signature_hash_prefix": sig8,
                    "last_verified_at": ts,
                })
            );
        } else {
            println!(
                "{}: up-to-date (verified @ {}, sig={})",
                args.function, ts, sig8
            );
        }
        return Ok(());
    }

    let provider = args.provider.clone().unwrap_or_else(|| args.bridge.clone());
    let cache_root = std::env::current_dir()?.join("bridges");
    let resolved =
        bridge_cache::ensure(&row, &args.bridge, &provider, &cache_root)?;

    let cases = db::load_cases(
        &conn,
        &args.extension,
        &args.function,
        args.case.as_deref(),
    )?;
    if cases.is_empty() {
        if args.case.is_some() {
            bail!(
                "no case '{}' for {}.{}",
                args.case.as_deref().unwrap_or(""),
                args.extension,
                args.function
            );
        }
        eprintln!(
            "warn: zero cases for {}.{} -- status left as '{}'",
            args.extension, args.function, row.status
        );
        return Ok(());
    }

    // Prepare a scratch ducklink `--extensions-dir` with the
    // bridge under its canonical name. Design §5 provider hash
    // stamp uses the *same* file for MVP monolith mode.
    let scratch = tempfile::tempdir()?;
    let ext_name = args.extension.clone();
    let scratch_bridge = scratch.path().join(format!("{ext_name}.wasm"));
    fs::copy(&resolved.bridge_path, &scratch_bridge).with_context(|| {
        format!(
            "copy {} -> {}",
            resolved.bridge_path.display(),
            scratch_bridge.display()
        )
    })?;

    let host_version = format!(
        "{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    let mut pass = 0i64;
    let mut fail = 0i64;
    let mut outcomes: Vec<CaseOutcome> = Vec::with_capacity(cases.len());
    let ran_at = chrono::Utc::now().to_rfc3339();

    for case in &cases {
        let started = Instant::now();
        let actual = execute_probe(
            &args.ducklink,
            scratch.path(),
            &ext_name,
            &case.sql_inline,
            args.json,
        )?;
        let duration_ms = started.elapsed().as_millis() as i64;
        let expected = case.expected.trim().to_string();
        let actual_trim = actual.trim().to_string();
        let pass_this = expected == actual_trim;
        let status_str = if pass_this { "pass" } else { "fail" };
        db::insert_test_run(
            &conn,
            &args.extension,
            &args.function,
            &case.case_name,
            status_str,
            Some(&actual_trim),
            duration_ms,
            &host_version,
            &resolved.provider_wasm_hash,
            &resolved.bridge_wasm_hash,
            row.last_seen_upstream_version.as_deref(),
            &ran_at,
        )?;
        if pass_this {
            pass += 1;
        } else {
            fail += 1;
        }
        if args.json {
            println!(
                "{}",
                serde_json::json!({
                    "function": args.function,
                    "case": case.case_name,
                    "status": status_str,
                    "expected": expected,
                    "actual": actual_trim,
                    "duration_ms": duration_ms,
                    "bridge_wasm_hash": resolved.bridge_wasm_hash,
                    "provider_wasm_hash": resolved.provider_wasm_hash,
                })
            );
        } else if pass_this {
            println!("  PASS   {:<32} ({} ms)", case.case_name, duration_ms);
        } else {
            println!("  FAIL   {:<32} ({} ms)", case.case_name, duration_ms);
            println!("    expected: {}", expected);
            println!("    actual:   {}", actual_trim);
        }
        outcomes.push(CaseOutcome {
            case_name: case.case_name.clone(),
            status: status_str.to_string(),
            actual: actual_trim,
            duration_ms,
        });
    }

    // §7 promotion: only touch status when we ran the full suite.
    let previous_status = row.status.clone();
    let mut new_status = previous_status.clone();
    if args.case.is_none() {
        let sig = row.signature_hash.clone().unwrap_or_default();
        let im = row.implementation_hash.clone().unwrap_or_default();
        if fail == 0 && pass > 0 {
            db::mark_verified(
                &conn,
                &args.extension,
                &args.function,
                &sig,
                &im,
                row.last_seen_upstream_version.as_deref(),
                &ran_at,
            )?;
            new_status = "implemented_verified".to_string();
        } else if fail > 0 {
            let first_fail = outcomes
                .iter()
                .find(|o| o.status == "fail")
                .map(|o| o.case_name.as_str())
                .unwrap_or("?");
            db::mark_failed(
                &conn,
                &args.extension,
                &args.function,
                &format!("case={first_fail}"),
                &ran_at,
            )?;
            // MVP: mark_failed reuses `implemented_unverified` (the
            // schema's status enum doesn't include `broken` yet).
            // The next run without --force will re-execute because
            // the verified-hash columns are now NULL, so
            // `status::is_cache_hit` returns false.
            new_status = "implemented_unverified".to_string();
        }
    }

    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "summary": true,
                "function": args.function,
                "pass": pass,
                "fail": fail,
                "previous_status": previous_status,
                "new_status": new_status,
            })
        );
    } else {
        println!(
            "status: {} (was: {})",
            new_status, previous_status
        );
        println!("{} passed, {} failed", pass, fail);
    }
    Ok(())
}

pub struct BatchArgs {
    pub interface: PathBuf,
    pub extension: String,
    pub all: bool,
    pub leaf: Option<String>,
    pub functions: Vec<String>,
    pub limit: Option<usize>,
    pub bridge: PathBuf,
    pub provider: Option<PathBuf>,
    pub ducklink: PathBuf,
    pub keep_going: bool,
    pub json: bool,
}

/// Batch case runner. Iterates the selected `(function, case)` set
/// against a single composed bridge, streaming test_runs rows as
/// it goes so a Ctrl-C mid-batch leaves partial progress visible.
///
/// Selection rules:
///   `--all`             every function with cases in `test_cases`
///   `--leaf <leaf>`     functions tagged `leaf:<leaf>`
///   `--functions f1,f2` explicit function list
/// `--limit N` caps the total number of cases run across all
/// functions in this invocation, bounding runtime for workflow use.
pub fn run_batch(args: BatchArgs) -> Result<()> {
    let conn = db::open(&args.interface)?;
    // Collect cases directly rather than functions -> cases per
    // function; `--leaf` selects the *cases tagged with that leaf*
    // (not every case for every function the leaf touches, which
    // was too broad and blew past `--limit` on unrelated files).
    let mut cases_by_fn: std::collections::BTreeMap<String, Vec<db::TestCase>> =
        std::collections::BTreeMap::new();
    if !args.functions.is_empty() {
        for f in &args.functions {
            let fl = f.to_lowercase();
            let cs = db::load_cases(&conn, &args.extension, &fl, None)?;
            if !cs.is_empty() {
                cases_by_fn.insert(fl, cs);
            }
        }
    } else if let Some(leaf) = &args.leaf {
        let cs = db::load_cases_by_leaf(&conn, &args.extension, leaf)?;
        for c in cs {
            cases_by_fn
                .entry(c.function_name.clone())
                .or_default()
                .push(c);
        }
    } else if args.all {
        for fn_name in db::list_functions_with_cases(&conn, &args.extension)? {
            let cs = db::load_cases(&conn, &args.extension, &fn_name, None)?;
            if !cs.is_empty() {
                cases_by_fn.insert(fn_name, cs);
            }
        }
    } else {
        bail!("batch: one of --all, --leaf, or --functions is required");
    };
    let functions: Vec<String> = cases_by_fn.keys().cloned().collect();
    if functions.is_empty() {
        println!("batch: 0 functions selected. Nothing to do.");
        return Ok(());
    }

    let provider = args.provider.clone().unwrap_or_else(|| args.bridge.clone());
    let cache_root = std::env::current_dir()?.join("bridges");

    // Single composed bridge is used for the whole batch. Hash it
    // once up-front rather than re-computing per function.
    let scratch = tempfile::tempdir()?;
    let ext_name = args.extension.clone();
    let scratch_bridge = scratch.path().join(format!("{ext_name}.wasm"));
    let bridge_hash = crate::hashing::sha256_hex(&args.bridge)?;
    let provider_hash = crate::hashing::sha256_hex(&provider)?;
    fs::copy(&args.bridge, &scratch_bridge).with_context(|| {
        format!(
            "copy {} -> {}",
            args.bridge.display(),
            scratch_bridge.display()
        )
    })?;
    let _ = cache_root; // batch mode shares the composed bridge; per-fn cache irrelevant.

    let host_version = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let ran_at = chrono::Utc::now().to_rfc3339();

    let mut total_ran = 0usize;
    let mut total_pass = 0i64;
    let mut total_fail = 0i64;
    let mut error_patterns: std::collections::HashMap<String, i64> = Default::default();

    let total_cases: usize = cases_by_fn.values().map(|v| v.len()).sum();
    let cap = args.limit.unwrap_or(total_cases);
    eprintln!(
        "batch: {} functions, {} cases (running up to {})",
        functions.len(),
        total_cases,
        cap,
    );

    'outer: for fn_name in &functions {
        let empty: Vec<db::TestCase> = Vec::new();
        let cases: &Vec<db::TestCase> = cases_by_fn.get(fn_name).unwrap_or(&empty);
        for case in cases {
            if total_ran >= cap {
                break 'outer;
            }
            let started = Instant::now();
            let actual = execute_probe(
                &args.ducklink,
                scratch.path(),
                &ext_name,
                &case.sql_inline,
                args.json,
            )
            .unwrap_or_else(|e| format!("ERROR: probe-spawn {}", e));
            let duration_ms = started.elapsed().as_millis() as i64;
            let expected = case.expected.trim().to_string();
            let actual_trim = actual.trim().to_string();
            let pass_this = expected == actual_trim;
            let status_str = if pass_this { "pass" } else { "fail" };
            db::insert_test_run(
                &conn,
                &args.extension,
                fn_name,
                &case.case_name,
                status_str,
                Some(&actual_trim),
                duration_ms,
                &host_version,
                &provider_hash,
                &bridge_hash,
                None,
                &ran_at,
            )?;
            total_ran += 1;
            if pass_this {
                total_pass += 1;
            } else {
                total_fail += 1;
                // Bucket failing outputs by their leading "ERROR:" / prefix.
                let bucket = fail_bucket(&actual_trim);
                *error_patterns.entry(bucket).or_insert(0) += 1;
                if !args.keep_going {
                    break 'outer;
                }
            }
            if args.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "function": fn_name,
                        "case": case.case_name,
                        "status": status_str,
                        "expected": expected,
                        "actual": actual_trim,
                        "duration_ms": duration_ms,
                    })
                );
            } else if total_ran % 25 == 0 || !pass_this {
                let tag = if pass_this { "PASS" } else { "FAIL" };
                eprintln!(
                    "  [{:>4}/{:<4}] {} {}.{} ({} ms)",
                    total_ran, cap, tag, fn_name, case.case_name, duration_ms
                );
            }
        }
    }

    println!();
    println!(
        "batch summary: ran {} cases across {} functions -- {} pass, {} fail",
        total_ran,
        functions.len(),
        total_pass,
        total_fail
    );
    if !error_patterns.is_empty() {
        println!("top failing patterns:");
        let mut items: Vec<_> = error_patterns.iter().collect();
        items.sort_by(|a, b| b.1.cmp(a.1));
        for (pat, n) in items.iter().take(10) {
            println!("  {:>5}  {}", n, pat);
        }
    }
    Ok(())
}

/// Bucket a failing `actual` output into a coarse pattern key so
/// the batch summary can rank the dominant failure modes without
/// swamping the operator in unique diagnostic strings.
fn fail_bucket(actual: &str) -> String {
    let a = actual.trim();
    if a.is_empty() {
        return "<empty output>".to_string();
    }
    if let Some(rest) = a.strip_prefix("ERROR:") {
        // Take the first ~60 chars post-prefix as the bucket.
        let t = rest.trim();
        let n = t.chars().take(60).collect::<String>();
        return format!("ERROR: {}", n);
    }
    // Non-error mismatch: keep the first token as the type shape.
    let head: String = a.chars().take(30).collect();
    format!("mismatch: {}", head)
}

fn execute_probe(
    ducklink: &std::path::Path,
    ext_dir: &std::path::Path,
    ext_name: &str,
    probe_sql: &str,
    json: bool,
) -> Result<String> {
    let mut script = String::new();
    script.push_str(&format!("LOAD {};\n", ext_name));
    script.push_str(".mode csv\n");
    if !probe_sql.trim_end().ends_with(';') {
        script.push_str(probe_sql);
        script.push_str(";\n");
    } else {
        script.push_str(probe_sql);
        script.push('\n');
    }

    let mut cmd = Command::new(ducklink);
    cmd.env("DUCKLINK_AUTOLOAD", "")
        .arg("--extensions-dir")
        .arg(ext_dir)
        .arg("--")
        .arg("duckdb-cli")
        .arg(":memory:")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawn {}", ducklink.display()))?;
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin.write_all(script.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    // Ducklink prints lots of `[extension-*]` chatter to stderr
    // and interleaves prompts (`D> `) on stdout. Merge both,
    // then extract the last CSV result row before any errors.
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    // If the child bailed out — nonexistent bridge path, dylib
    // load failure, panic, missing ducklink components — the
    // exit status is our only reliable signal. Fold it into
    // `actual` as an ERROR string so the test_runs row carries
    // a diagnostic instead of empty output.
    if !out.status.success() {
        if !json {
            eprintln!(
                "child exited non-zero ({:?}); full stderr:\n{}",
                out.status.code(),
                stderr
            );
        }
        let tail = stderr
            .lines()
            .filter(|l| !l.trim().is_empty())
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | ");
        return Ok(format!(
            "ERROR: exit={:?} stderr={}",
            out.status.code(),
            tail
        ));
    }
    // For an internal binder error we want to surface it — the
    // string carries the diagnostic. Prefer stderr first.
    if stderr.contains("internal error") || stdout.contains("internal error") {
        // Extract a compact reason.
        let combined = format!("{stderr}\n{stdout}");
        let line = combined
            .lines()
            .find(|l| l.contains("internal error"))
            .unwrap_or("internal error");
        return Ok(format!("ERROR: {}", line.trim()));
    }
    // Filter stdout: drop lines starting with `[` (ducklink
    // extension-manager chatter), prompt sigils, and blank lines.
    // Return the last non-noise line.
    let last = stdout
        .lines()
        .map(strip_prompt)
        .filter(|l| !l.trim().is_empty())
        .filter(|l| !l.starts_with('['))
        .filter(|l| !l.starts_with('|'))
        .filter(|l| !l.starts_with('+'))
        .filter(|l| !l.contains("Success"))
        .filter(|l| !l.trim().eq("LOAD postgis"))
        .filter(|l| !l.trim().starts_with(".mode"))
        // Column-name header row from `.mode csv` on the guest
        // CLI (e.g. `st_astext(...)` -- the auto-derived alias).
        // We only want the data row(s) that follow.
        .filter(|l| {
            !(l.starts_with("st_") || l.starts_with('"'))
                || l.chars().any(|c| c.is_ascii_digit())
                || l.starts_with('0')
        })
        .last()
        .unwrap_or_default()
        .to_string();
    Ok(last)
}

/// Strip a leading `D> ` (repeated ducklink guest-CLI prompt).
fn strip_prompt(l: &str) -> String {
    let mut s = l.trim_start();
    while let Some(rest) = s.strip_prefix("D> ") {
        s = rest.trim_start();
    }
    while let Some(rest) = s.strip_prefix("...> ") {
        s = rest.trim_start();
    }
    s.to_string()
}
