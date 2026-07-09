//! Thin DAO around the interface DB: reads function rows, writes
//! `test_runs`, and applies the status transition. All statements
//! use prepared bindings; nothing is string-interpolated.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// A row from `scalars` we need to make cache-vs-run decisions.
#[derive(Debug, Clone)]
pub struct FunctionRow {
    pub extension: String,
    pub name: String,
    pub signature_hash: Option<String>,
    pub implementation_hash: Option<String>,
    pub status: String,
    pub last_verified_signature_hash: Option<String>,
    pub last_verified_implementation_hash: Option<String>,
    pub last_verified_at: Option<String>,
    pub last_seen_upstream_version: Option<String>,
}

/// One test case (source of truth: the `test_cases` table).
#[derive(Debug, Clone)]
pub struct TestCase {
    pub extension: String,
    pub function_name: String,
    pub case_name: String,
    pub sql_inline: String,
    pub expected: String,
    pub source: String,
    /// Raw `tags_json` column (JSON array of strings). Batch mode
    /// consults this to skip `fixture_bad` cases at runtime; the
    /// coverage roll-up parses it for `leaf:*` bucketing.
    pub tags_json: String,
}

impl TestCase {
    /// True if `tags_json` contains the given tag literal (exact
    /// match, quoted). Cheap substring check — avoids a JSON parse
    /// per row on the hot batch loop. The scraper writes tags as a
    /// quoted-string JSON array (e.g. `["leaf:foo","fixture_bad"]`)
    /// so a bare `"fixture_bad"` needle won't collide with the
    /// `fixture_bad_pattern:*` companion tag.
    pub fn has_tag(&self, tag: &str) -> bool {
        let needle = format!("\"{}\"", tag);
        self.tags_json.contains(&needle)
    }

    /// Convenience for the scraper's `fixture_bad` marker. Cases
    /// carrying this tag are known-broken upstream fixtures the
    /// shim cannot execute; batch mode skips them entirely rather
    /// than folding them into the pass/fail totals.
    pub fn is_fixture_bad(&self) -> bool {
        self.has_tag("fixture_bad")
    }
}

pub fn open(interface_db: &Path) -> Result<Connection> {
    let conn = Connection::open(interface_db)
        .with_context(|| format!("open {}", interface_db.display()))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

pub fn load_scalar(
    conn: &Connection,
    extension: &str,
    function: &str,
) -> Result<Option<FunctionRow>> {
    let mut stmt = conn.prepare(
        "SELECT extension, name, signature_hash, implementation_hash,
                status, last_verified_signature_hash,
                last_verified_implementation_hash, last_verified_at,
                last_seen_upstream_version
         FROM scalars WHERE extension = ?1 AND name = ?2",
    )?;
    let row = stmt
        .query_row(params![extension, function], |r| {
            Ok(FunctionRow {
                extension: r.get(0)?,
                name: r.get(1)?,
                signature_hash: r.get(2)?,
                implementation_hash: r.get(3)?,
                status: r.get(4)?,
                last_verified_signature_hash: r.get(5)?,
                last_verified_implementation_hash: r.get(6)?,
                last_verified_at: r.get(7)?,
                last_seen_upstream_version: r.get(8)?,
            })
        })
        .optional()?;
    Ok(row)
}

pub fn load_cases(
    conn: &Connection,
    extension: &str,
    function: &str,
    only: Option<&str>,
) -> Result<Vec<TestCase>> {
    load_cases_ex(conn, extension, function, only, true)
}

/// Extended `load_cases` variant with an explicit `include_pg_only`
/// switch. Kept as a separate entry point so historical callers
/// (`test-fn run` per-function mode) continue to see every case for
/// the function while batch selection paths can opt out of the
/// pg_only bucket.
pub fn load_cases_ex(
    conn: &Connection,
    extension: &str,
    function: &str,
    only: Option<&str>,
    include_pg_only: bool,
) -> Result<Vec<TestCase>> {
    let mut sql = String::from(
        "SELECT extension, function_name, case_name,
                COALESCE(sql_inline, ''), COALESCE(expected, ''),
                source, COALESCE(tags_json, '[]')
         FROM test_cases
         WHERE extension = ?1 AND function_name = ?2",
    );
    if only.is_some() {
        sql.push_str(" AND case_name = ?3");
    }
    if !include_pg_only {
        sql.push_str(" AND tags_json NOT LIKE '%\"pg_only\"%'");
    }
    sql.push_str(" ORDER BY case_name");
    let mut stmt = conn.prepare(&sql)?;
    let cases: Vec<TestCase> = if let Some(c) = only {
        stmt.query_map(params![extension, function, c], row_to_case)?
            .collect::<Result<_, _>>()?
    } else {
        stmt.query_map(params![extension, function], row_to_case)?
            .collect::<Result<_, _>>()?
    };
    Ok(cases)
}

fn row_to_case(r: &rusqlite::Row) -> rusqlite::Result<TestCase> {
    Ok(TestCase {
        extension: r.get(0)?,
        function_name: r.get(1)?,
        case_name: r.get(2)?,
        sql_inline: r.get(3)?,
        expected: r.get(4)?,
        source: r.get(5)?,
        tags_json: r.get(6)?,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn insert_test_run(
    conn: &Connection,
    extension: &str,
    function: &str,
    case_name: &str,
    status: &str,
    actual: Option<&str>,
    duration_ms: i64,
    host_version: &str,
    provider_wasm_hash: &str,
    bridge_wasm_hash: &str,
    upstream_version: Option<&str>,
    ran_at: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO test_runs
            (extension, function_name, case_name, status, actual,
             duration_ms, host_version, provider_wasm_hash,
             bridge_wasm_hash, upstream_version, ran_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            extension,
            function,
            case_name,
            status,
            actual,
            duration_ms,
            host_version,
            provider_wasm_hash,
            bridge_wasm_hash,
            upstream_version,
            ran_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Promote a scalar row to `implemented_verified`, stamping the
/// hashes and timestamp the design mandates in §7.
pub fn mark_verified(
    conn: &Connection,
    extension: &str,
    function: &str,
    signature_hash: &str,
    implementation_hash: &str,
    upstream_version: Option<&str>,
    ran_at: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE scalars SET
            status = 'implemented_verified',
            last_verified_signature_hash = ?3,
            last_verified_implementation_hash = ?4,
            last_verified_upstream_version = COALESCE(?5, last_verified_upstream_version),
            last_verified_at = ?6
         WHERE extension = ?1 AND name = ?2",
        params![
            extension,
            function,
            signature_hash,
            implementation_hash,
            upstream_version,
            ran_at
        ],
    )?;
    Ok(())
}

/// Demote a scalar row on failure. MVP: we reuse
/// `implemented_unverified` because the schema's status enum
/// doesn't yet include a distinct `broken` value (future
/// enhancement — see docs/DISCOVERY-UX §7). Alongside the status
/// flip we NULL out `last_verified_signature_hash` and
/// `last_verified_implementation_hash` so `status::is_cache_hit`
/// returns false on the next invocation and the harness re-runs
/// without needing `--force`. The failure reason lands in
/// `notes` for post-mortem.
pub fn mark_failed(
    conn: &Connection,
    extension: &str,
    function: &str,
    reason: &str,
    ran_at: &str,
) -> Result<()> {
    let note = format!("test-fn FAIL @ {}: {}", ran_at, reason);
    conn.execute(
        "UPDATE scalars SET
            status = 'implemented_unverified',
            last_verified_signature_hash = NULL,
            last_verified_implementation_hash = NULL,
            notes = ?3
         WHERE extension = ?1 AND name = ?2",
        params![extension, function, note],
    )?;
    Ok(())
}

/// Return the list of distinct function names in `test_cases` for
/// this extension, in stable name order.
pub fn list_functions_with_cases(conn: &Connection, extension: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT function_name FROM test_cases
         WHERE extension = ?1 ORDER BY function_name",
    )?;
    let names: Vec<String> = stmt
        .query_map(params![extension], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    Ok(names)
}

/// Return the list of distinct function names whose `test_cases`
/// rows carry the `leaf:<leaf>` tag. Only exact-match tags are
/// checked (the scraper writes them as JSON array strings).
pub fn list_functions_by_leaf(
    conn: &Connection,
    extension: &str,
    leaf: &str,
) -> Result<Vec<String>> {
    let tag = format!("\"leaf:{}\"", leaf);
    let mut stmt = conn.prepare(
        "SELECT DISTINCT function_name FROM test_cases
         WHERE extension = ?1 AND tags_json LIKE '%' || ?2 || '%'
         ORDER BY function_name",
    )?;
    let names: Vec<String> = stmt
        .query_map(params![extension, tag], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    Ok(names)
}

/// Load cases whose `tags_json` contains the given `leaf:<leaf>`
/// tag, regardless of function. Used by batch mode when the
/// operator asked for a `--leaf` slice and wants only the leaf's
/// own cases (not the transitive union of every case for every
/// function the leaf touched).
///
/// `include_pg_only == false` (the default in batch mode) drops
/// rows carrying the `pg_only` tag — these are pg-catalog-specific
/// probes the scraper preserved for coverage bookkeeping but that
/// the shim cannot execute meaningfully. Set to `true` to keep
/// them (useful when auditing the pg_only bucket by leaf).
pub fn load_cases_by_leaf(
    conn: &Connection,
    extension: &str,
    leaf: &str,
    include_pg_only: bool,
) -> Result<Vec<TestCase>> {
    let tag = format!("\"leaf:{}\"", leaf);
    let base = String::from(
        "SELECT extension, function_name, case_name,
                COALESCE(sql_inline, ''), COALESCE(expected, ''),
                source, COALESCE(tags_json, '[]')
           FROM test_cases
          WHERE extension = ?1
            AND tags_json LIKE '%' || ?2 || '%'",
    );
    let sql = if include_pg_only {
        format!("{} ORDER BY function_name, case_name", base)
    } else {
        format!(
            "{} AND tags_json NOT LIKE '%\"pg_only\"%' ORDER BY function_name, case_name",
            base
        )
    };
    let mut stmt = conn.prepare(&sql)?;
    let cases: Vec<TestCase> = stmt
        .query_map(params![extension, tag], row_to_case)?
        .collect::<Result<_, _>>()?;
    Ok(cases)
}

/// Coverage grouped by `leaf:*` tag. For each leaf we roll up:
/// total cases, cases with at least one passing run, cases with
/// at least one failing run (no pass yet), untested cases.
pub fn coverage_by_leaf(
    interface_db: &Path,
    extension: &str,
    json: bool,
    show_pg_only: bool,
    show_fixture_bad: bool,
) -> Result<()> {
    let conn = open(interface_db)?;
    // Pull all cases with their tags.
    let mut stmt = conn.prepare(
        "SELECT tc.function_name, tc.case_name, tc.tags_json,
                (SELECT COUNT(*) FROM test_runs tr
                    WHERE tr.extension = tc.extension
                      AND tr.function_name = tc.function_name
                      AND tr.case_name = tc.case_name
                      AND tr.status = 'pass') AS pass_runs,
                (SELECT COUNT(*) FROM test_runs tr
                    WHERE tr.extension = tc.extension
                      AND tr.function_name = tc.function_name
                      AND tr.case_name = tc.case_name
                      AND tr.status = 'fail') AS fail_runs
           FROM test_cases tc
          WHERE tc.extension = ?1",
    )?;
    #[derive(Default, Debug)]
    struct LeafAgg {
        cases: i64,
        pass: i64,
        fail: i64,
        untested: i64,
        pg_only: i64,
        /// Count of `fixture_bad`-tagged cases in this leaf.
        /// Tracked separately so operators can see the deferred-
        /// work backlog on known-broken upstream fixtures without
        /// inflating the runnable-case totals.
        fixture_bad: i64,
    }
    let mut by_leaf: std::collections::BTreeMap<String, LeafAgg> =
        std::collections::BTreeMap::new();
    let rows = stmt.query_map(params![extension], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
        ))
    })?;
    for row in rows {
        let (_fn_name, _case_name, tags_json, pass_runs, fail_runs) = row?;
        let tags: Vec<String> =
            serde_json::from_str(&tags_json).unwrap_or_default();
        let is_pg_only = tags.iter().any(|t| t == "pg_only");
        let is_fixture_bad = tags.iter().any(|t| t == "fixture_bad");
        // Skip pg_only cases from the primary leaf rollup unless
        // the operator asked for them — they inflate leaf totals
        // with rows the shim can't execute.
        if is_pg_only && !show_pg_only {
            let mut charged = false;
            for t in &tags {
                if let Some(leaf) = t.strip_prefix("leaf:") {
                    let a = by_leaf.entry(leaf.to_string()).or_default();
                    a.pg_only += 1;
                    if is_fixture_bad {
                        a.fixture_bad += 1;
                    }
                    charged = true;
                }
            }
            if !charged {
                let a = by_leaf.entry("<untagged>".to_string()).or_default();
                a.pg_only += 1;
                if is_fixture_bad {
                    a.fixture_bad += 1;
                }
            }
            continue;
        }
        // Same treatment for `fixture_bad`: batch mode skips these
        // at runtime (see runner.rs `is_fixture_bad()`), so exclude
        // them from the primary CASES/PASS/FAIL/UNTESTED rollup
        // unless the operator explicitly asked to fold them in.
        // Tracked separately in `fixture_bad` so operators can see
        // the deferred-work backlog per leaf regardless.
        if is_fixture_bad && !show_fixture_bad {
            let mut charged = false;
            for t in &tags {
                if let Some(leaf) = t.strip_prefix("leaf:") {
                    let a = by_leaf.entry(leaf.to_string()).or_default();
                    a.fixture_bad += 1;
                    charged = true;
                }
            }
            if !charged {
                let a = by_leaf.entry("<untagged>".to_string()).or_default();
                a.fixture_bad += 1;
            }
            continue;
        }
        let mut had_leaf = false;
        for t in &tags {
            if let Some(leaf) = t.strip_prefix("leaf:") {
                had_leaf = true;
                let a = by_leaf.entry(leaf.to_string()).or_default();
                a.cases += 1;
                if pass_runs > 0 {
                    a.pass += 1;
                } else if fail_runs > 0 {
                    a.fail += 1;
                } else {
                    a.untested += 1;
                }
                if is_pg_only {
                    a.pg_only += 1;
                }
                if is_fixture_bad {
                    a.fixture_bad += 1;
                }
            }
        }
        if !had_leaf {
            let a = by_leaf.entry("<untagged>".to_string()).or_default();
            a.cases += 1;
            if pass_runs > 0 {
                a.pass += 1;
            } else if fail_runs > 0 {
                a.fail += 1;
            } else {
                a.untested += 1;
            }
            if is_pg_only {
                a.pg_only += 1;
            }
            if is_fixture_bad {
                a.fixture_bad += 1;
            }
        }
    }
    if json {
        let obj: serde_json::Value = serde_json::json!({
            "extension": extension,
            "show_pg_only": show_pg_only,
            "show_fixture_bad": show_fixture_bad,
            "by_leaf": by_leaf.iter().map(|(k, v)| {
                let coverage_pct = if v.cases > 0 {
                    100.0 * (v.pass as f64) / (v.cases as f64)
                } else { 0.0 };
                serde_json::json!({
                    "leaf": k,
                    "cases": v.cases,
                    "pass": v.pass,
                    "fail": v.fail,
                    "untested": v.untested,
                    "pg_only": v.pg_only,
                    "fixture_bad": v.fixture_bad,
                    "coverage_pct": coverage_pct,
                })
            }).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else if show_fixture_bad {
        println!("{:<28} {:>7} {:>7} {:>7} {:>8} {:>8} {:>10} {:>10}",
            "LEAF", "CASES", "PASS", "FAIL", "UNTESTED", "PG-ONLY", "FIX-BAD", "COVERAGE");
        for (leaf, a) in &by_leaf {
            let pct = if a.cases > 0 {
                100.0 * (a.pass as f64) / (a.cases as f64)
            } else { 0.0 };
            println!("{:<28} {:>7} {:>7} {:>7} {:>8} {:>8} {:>10} {:>9.1}%",
                leaf, a.cases, a.pass, a.fail, a.untested, a.pg_only, a.fixture_bad, pct);
        }
        println!();
        println!("(FIX-BAD counts fixture_bad-tagged cases per leaf. With");
        println!(" --show-fixture-bad they are ALSO counted in CASES/PASS/FAIL/");
        println!(" UNTESTED; drop the flag to exclude them from the rollup.)");
        if !show_pg_only {
            println!();
            println!("(pg_only rows are excluded from CASES/PASS/FAIL/UNTESTED; ");
            println!(" re-run with --show-pg-only to include them.)");
        }
    } else {
        println!("{:<28} {:>7} {:>7} {:>7} {:>8} {:>8} {:>10} {:>10}",
            "LEAF", "CASES", "PASS", "FAIL", "UNTESTED", "PG-ONLY", "FIX-BAD", "COVERAGE");
        for (leaf, a) in &by_leaf {
            let pct = if a.cases > 0 {
                100.0 * (a.pass as f64) / (a.cases as f64)
            } else { 0.0 };
            println!("{:<28} {:>7} {:>7} {:>7} {:>8} {:>8} {:>10} {:>9.1}%",
                leaf, a.cases, a.pass, a.fail, a.untested, a.pg_only, a.fixture_bad, pct);
        }
        println!();
        println!("(FIX-BAD counts fixture_bad-tagged cases per leaf. They are");
        println!(" excluded from CASES/PASS/FAIL/UNTESTED — batch mode skips");
        println!(" these known-broken upstream fixtures. Re-run with");
        println!(" --show-fixture-bad to fold them into the rollup.)");
        if !show_pg_only {
            println!();
            println!("(pg_only rows are excluded from CASES/PASS/FAIL/UNTESTED; ");
            println!(" re-run with --show-pg-only to include them.)");
        }
    }
    Ok(())
}

/// Coverage grouped by function name (top-N by case count).
pub fn coverage_by_function(interface_db: &Path, extension: &str, json: bool) -> Result<()> {
    let conn = open(interface_db)?;
    let mut stmt = conn.prepare(
        "SELECT tc.function_name,
                COUNT(*) AS cases,
                SUM(CASE WHEN EXISTS (
                    SELECT 1 FROM test_runs tr
                     WHERE tr.extension = tc.extension
                       AND tr.function_name = tc.function_name
                       AND tr.case_name = tc.case_name
                       AND tr.status = 'pass') THEN 1 ELSE 0 END) AS pass_cases,
                SUM(CASE WHEN EXISTS (
                    SELECT 1 FROM test_runs tr
                     WHERE tr.extension = tc.extension
                       AND tr.function_name = tc.function_name
                       AND tr.case_name = tc.case_name
                       AND tr.status = 'fail') AND NOT EXISTS (
                    SELECT 1 FROM test_runs tr
                     WHERE tr.extension = tc.extension
                       AND tr.function_name = tc.function_name
                       AND tr.case_name = tc.case_name
                       AND tr.status = 'pass') THEN 1 ELSE 0 END) AS fail_cases
           FROM test_cases tc
          WHERE tc.extension = ?1
          GROUP BY tc.function_name
          ORDER BY cases DESC",
    )?;
    let rows: Vec<(String, i64, i64, i64)> = stmt
        .query_map(params![extension], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<Result<_, _>>()?;
    if json {
        let obj = serde_json::json!({
            "extension": extension,
            "by_function": rows.iter().map(|(name, c, p, f)| serde_json::json!({
                "function": name,
                "cases": c,
                "pass": p,
                "fail": f,
                "coverage_pct": if *c > 0 { 100.0 * (*p as f64) / (*c as f64) } else { 0.0 },
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        println!("{:<40} {:>7} {:>7} {:>7} {:>10}", "FUNCTION", "CASES", "PASS", "FAIL", "COVERAGE");
        for (name, c, p, f) in &rows {
            let pct = if *c > 0 { 100.0 * (*p as f64) / (*c as f64) } else { 0.0 };
            println!("{:<40} {:>7} {:>7} {:>7} {:>9.1}%", name, c, p, f, pct);
        }
    }
    Ok(())
}

pub fn coverage(interface_db: &Path, extension: &str, json: bool) -> Result<()> {
    let conn = open(interface_db)?;
    let mut stmt = conn.prepare(
        "SELECT status, COUNT(*) FROM scalars WHERE extension = ?1
         GROUP BY status ORDER BY status",
    )?;
    let rows: Vec<(String, i64)> = stmt
        .query_map(params![extension], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    if json {
        let obj: serde_json::Value = serde_json::json!({
            "extension": extension,
            "scalars_by_status": rows.iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect::<std::collections::BTreeMap<_, _>>(),
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        println!("extension: {}", extension);
        println!("  scalars:");
        for (status, n) in &rows {
            println!("    {:32} {:>6}", status, n);
        }
    }
    Ok(())
}
