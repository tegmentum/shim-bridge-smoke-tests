//! `funcs-md-gen` — auto-generate a `FUNCTIONS.md` status doc for
//! one shim from its shim-interface catalog (B3).
//!
//! The hand-maintained `POSTGIS_FUNCTIONS.md` in `postgis-wasm`
//! predates the catalog and was drifting behind reality: the
//! catalog knows every function name the shim advertises, every
//! test case the scraper found, every test run the harness has
//! recorded, and the `implemented_verified` bit the harness flips
//! when a function passes all its cases. This binary reads that
//! catalog and produces the same shape of markdown table without
//! human intervention.
//!
//! Grouping is by `leaf:*` tag from `test_cases.tags_json`. A
//! function whose canonical row lives in `scalars` but has zero
//! `test_cases` is listed under an "Uncovered" section, not
//! silently dropped. Aliases (from `scalar_aliases`) show up as
//! a parenthetical after the canonical name.
//!
//! CLI shape:
//!
//! ```text
//! funcs-md-gen --interface <sqlite> \
//!              --extension <postgis|mobilitydb|timescaledb> \
//!              --out <md-file>
//! ```
//!
//! `--summary-only` skips the per-leaf tables and emits just the
//! top-line "N verified, M with cases, T total" summary — handy
//! for CI budget checks that don't want the full doc.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use rusqlite::{params, Connection};

#[derive(Parser, Debug)]
#[command(
    name = "funcs-md-gen",
    about = "Auto-generate FUNCTIONS.md from a shim-interface catalog."
)]
struct Cli {
    /// Interface DB path (one of the canonical
    /// `~/git/*-shim-interface/*-interface.sqlite` files).
    #[arg(long)]
    interface: PathBuf,

    /// Extension name (`postgis` / `mobilitydb` / `timescaledb`).
    #[arg(long)]
    extension: String,

    /// Output markdown file. Overwritten in place.
    #[arg(long)]
    out: PathBuf,

    /// Skip the per-leaf tables; emit just the summary line.
    #[arg(long)]
    summary_only: bool,
}

/// A row from `scalars` plus the derived per-name totals we need
/// to render one line of the leaf table.
struct FnRow {
    name: String,
    status: String,
    last_verified_at: Option<String>,
    case_count: i64,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if !cli.interface.exists() {
        anyhow::bail!(
            "interface DB {} does not exist",
            cli.interface.display()
        );
    }
    let conn = Connection::open(&cli.interface)
        .with_context(|| format!("open {}", cli.interface.display()))?;

    let extension = cli.extension.as_str();
    let upstream_version = current_upstream_version(&conn, extension)?
        .unwrap_or_else(|| "unknown".into());

    // Aggregate scalar-status counts. The doc-facing "total" number
    // uses `COUNT(*) FROM scalars` so it aligns with `SELECT count
    // scalars` folk-lore; the status split covers every scalar
    // (aggregates/table/window are folded into totals in the
    // "Uncovered" area later if they lack cases).
    let (verified, unverified, broken, deprecated, unimplemented, skip, total_scalars) =
        scalar_status_counts(&conn, extension)?;

    // Names of every function with at least one row in test_cases.
    let names_with_cases = names_with_cases(&conn, extension)?;

    if cli.summary_only {
        let mut out = fs::File::create(&cli.out)
            .with_context(|| format!("create {}", cli.out.display()))?;
        writeln!(
            out,
            "{} @ {} — {}/{} verified, {}/{} have test cases",
            extension_title(extension),
            upstream_version,
            verified,
            total_scalars,
            names_with_cases.len(),
            total_scalars,
        )?;
        return Ok(());
    }

    // Load per-function row (case_count included) so leaf-table
    // rendering is a straight iteration.
    let per_fn_rows = load_scalar_rows_with_case_counts(&conn, extension)?;

    // Group functions with at least one case by their leaf tag.
    let by_leaf = group_by_leaf(&conn, extension, &per_fn_rows)?;

    // Functions that live in `scalars` but never appeared in
    // `test_cases`. Sorted so the uncovered dump stays stable.
    let uncovered: Vec<&FnRow> = per_fn_rows
        .values()
        .filter(|r| r.case_count == 0)
        .collect();

    let mut out = fs::File::create(&cli.out)
        .with_context(|| format!("create {}", cli.out.display()))?;

    // Header + status summary.
    let title = extension_title(extension);
    writeln!(out, "# {title} Functions Implementation Tracking")?;
    writeln!(out)?;
    writeln!(
        out,
        "Auto-generated from `{}` @ {} — do not edit by hand.",
        cli.interface
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("interface.sqlite"),
        upstream_version,
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "**Current Status: {} scalar functions in catalog ({} verified, \
         {} unverified, {} broken, {} deprecated, {} unimplemented, {} skip)**",
        total_scalars,
        verified,
        unverified,
        broken,
        deprecated,
        unimplemented,
        skip,
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "**Test coverage: {} scalars have at least one test case.**",
        names_with_cases.len()
    )?;
    writeln!(out)?;

    writeln!(out, "## Legend")?;
    writeln!(out, "- Verified (has passing test cases): {}", STATUS_VERIFIED)?;
    writeln!(out, "- Implemented, unverified: {}", STATUS_UNVERIFIED)?;
    writeln!(out, "- Broken (test cases failing): {}", STATUS_BROKEN)?;
    writeln!(out, "- Deprecated: {}", STATUS_DEPRECATED)?;
    writeln!(out, "- Unimplemented: {}", STATUS_UNIMPLEMENTED)?;
    writeln!(out, "- Skip: {}", STATUS_SKIP)?;
    writeln!(out)?;

    // Per-leaf tables.
    for (leaf, rows) in &by_leaf {
        writeln!(out, "## {}", pretty_leaf(leaf))?;
        writeln!(out)?;
        writeln!(
            out,
            "| Function | Status | Test cases | Last verified |"
        )?;
        writeln!(
            out,
            "|----------|--------|-----------:|---------------|"
        )?;
        for r in rows {
            writeln!(
                out,
                "| {} | {} | {} | {} |",
                r.name,
                status_glyph(&r.status),
                r.case_count,
                r.last_verified_at.as_deref().unwrap_or("—"),
            )?;
        }
        writeln!(out)?;
    }

    // Uncovered dump.
    if !uncovered.is_empty() {
        writeln!(out, "## Uncovered")?;
        writeln!(out)?;
        writeln!(
            out,
            "Functions in the catalog with zero test cases. \
             Counts: {} of {} scalars.",
            uncovered.len(),
            total_scalars
        )?;
        writeln!(out)?;
        writeln!(out, "| Function | Status |")?;
        writeln!(out, "|----------|--------|")?;
        // Sort for stability.
        let mut sorted: Vec<&FnRow> = uncovered.clone();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        for r in sorted {
            writeln!(
                out,
                "| {} | {} |",
                r.name,
                status_glyph(&r.status)
            )?;
        }
        writeln!(out)?;
    }

    Ok(())
}

fn extension_title(ext: &str) -> String {
    match ext {
        "postgis" => "PostGIS".to_string(),
        "mobilitydb" => "MobilityDB".to_string(),
        "timescaledb" => "TimescaleDB".to_string(),
        other => other.to_string(),
    }
}

/// Emoji glyphs surfaced in the emitted markdown. Kept as named
/// constants so the legend section and the per-row table stay in
/// lock-step; edit one and both change.
///
/// `broken` is not yet a value in the `scalars.status` enum but we
/// reserve a glyph so the moment the schema grows it, no map edit
/// is needed. `unknown` renders for aggregate/window rows that
/// don't have a corresponding scalar row.
const STATUS_VERIFIED: &str = "\u{2705}";      // ✅
const STATUS_UNVERIFIED: &str = "\u{1F7E1}";   // 🟡
const STATUS_BROKEN: &str = "\u{274C}";        // ❌
const STATUS_DEPRECATED: &str = "\u{26D4}";    // ⛔
const STATUS_UNIMPLEMENTED: &str = "\u{26AA}"; // ⚪
const STATUS_SKIP: &str = "\u{23ED}\u{FE0F}";  // ⏭️
const STATUS_UNKNOWN: &str = "\u{2753}";       // ❓

fn status_glyph(status: &str) -> &'static str {
    match status {
        "implemented_verified" => STATUS_VERIFIED,
        "implemented_unverified" => STATUS_UNVERIFIED,
        "broken" => STATUS_BROKEN,
        "deprecated" => STATUS_DEPRECATED,
        "unimplemented" => STATUS_UNIMPLEMENTED,
        "skip" => STATUS_SKIP,
        _ => STATUS_UNKNOWN,
    }
}

/// Best-effort leaf label. `leaf:foo_bar` -> `foo_bar`; other tags
/// pass through unchanged.
fn pretty_leaf(leaf: &str) -> String {
    leaf.strip_prefix("leaf:").unwrap_or(leaf).to_string()
}

fn current_upstream_version(
    conn: &Connection,
    extension: &str,
) -> Result<Option<String>> {
    // The extractor writes one row per ingested upstream release
    // to upstream_versions. Pick the row with the most recent
    // ingested_at as the "current" tag.
    let v: Option<String> = conn
        .query_row(
            "SELECT version FROM upstream_versions
                WHERE extension = ?1
                ORDER BY ingested_at DESC LIMIT 1",
            params![extension],
            |r| r.get(0),
        )
        .ok();
    if v.is_some() {
        return Ok(v);
    }
    // Fall back to any last_seen_upstream_version we spot on a
    // scalar row. Older DBs may not have populated
    // upstream_versions.
    let v: Option<String> = conn
        .query_row(
            "SELECT DISTINCT last_seen_upstream_version FROM scalars
                WHERE extension = ?1 AND last_seen_upstream_version IS NOT NULL
                LIMIT 1",
            params![extension],
            |r| r.get(0),
        )
        .ok();
    Ok(v)
}

/// Return `(verified, unverified, broken, deprecated, unimplemented,
/// skip, total)` across the scalars table for this extension.
fn scalar_status_counts(
    conn: &Connection,
    extension: &str,
) -> Result<(i64, i64, i64, i64, i64, i64, i64)> {
    let mut stmt = conn.prepare(
        "SELECT status, COUNT(*) FROM scalars WHERE extension = ?1 GROUP BY status",
    )?;
    let mut m: HashMap<String, i64> = HashMap::new();
    let iter = stmt.query_map(params![extension], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    for row in iter {
        let (k, v) = row?;
        m.insert(k, v);
    }
    let verified = *m.get("implemented_verified").unwrap_or(&0);
    let unverified = *m.get("implemented_unverified").unwrap_or(&0);
    let broken = *m.get("broken").unwrap_or(&0);
    let deprecated = *m.get("deprecated").unwrap_or(&0);
    let unimplemented = *m.get("unimplemented").unwrap_or(&0);
    let skip = *m.get("skip").unwrap_or(&0);
    let total = verified + unverified + broken + deprecated + unimplemented + skip;
    Ok((verified, unverified, broken, deprecated, unimplemented, skip, total))
}

fn names_with_cases(conn: &Connection, extension: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT function_name FROM test_cases
            WHERE extension = ?1 ORDER BY function_name",
    )?;
    let out: Vec<String> = stmt
        .query_map(params![extension], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    Ok(out)
}

fn load_scalar_rows_with_case_counts(
    conn: &Connection,
    extension: &str,
) -> Result<HashMap<String, FnRow>> {
    // LEFT JOIN keeps every scalar; the subquery counts test cases
    // for the same (extension, function_name). Grouping by scalar
    // name is safe because `(extension, name)` is the PK.
    let mut stmt = conn.prepare(
        "SELECT s.name, s.status, s.last_verified_at,
                (SELECT COUNT(*) FROM test_cases tc
                    WHERE tc.extension = s.extension
                      AND tc.function_name = s.name) AS case_count
           FROM scalars s
          WHERE s.extension = ?1
          ORDER BY s.name",
    )?;
    let rows = stmt.query_map(params![extension], |r| {
        Ok(FnRow {
            name: r.get(0)?,
            status: r.get(1)?,
            last_verified_at: r.get(2)?,
            case_count: r.get(3)?,
        })
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let r = row?;
        out.insert(r.name.clone(), r);
    }
    Ok(out)
}

/// Group per-function rows by `leaf:*` tag. A function that only
/// appears in test_cases under a non-leaf tag is placed under
/// `<untagged>`. Rows with no test cases are omitted here (they
/// land in the Uncovered dump).
fn group_by_leaf(
    conn: &Connection,
    extension: &str,
    per_fn: &HashMap<String, FnRow>,
) -> Result<BTreeMap<String, Vec<FnRowRef>>> {
    // We build FnRowRef structs (owned copies) so the map can outlive
    // the per_fn borrow. Cheap — one clone per function-with-cases.
    let mut stmt = conn.prepare(
        "SELECT DISTINCT function_name, tags_json FROM test_cases
            WHERE extension = ?1",
    )?;
    let iter = stmt.query_map(params![extension], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    // Map function_name -> set of leaf tags observed on any of its
    // cases. Emit the row into each observed leaf so leaves that
    // share a function get double-counted, mirroring the leaf_coverage
    // view's semantics.
    let mut fn_to_leaves: HashMap<String, Vec<String>> = HashMap::new();
    for row in iter {
        let (name, tags_json) = row?;
        let tags: Vec<String> =
            serde_json::from_str(&tags_json).unwrap_or_default();
        let leaves: Vec<String> = tags
            .into_iter()
            .filter(|t| t.starts_with("leaf:"))
            .collect();
        let entry = fn_to_leaves.entry(name).or_default();
        for l in leaves {
            if !entry.contains(&l) {
                entry.push(l);
            }
        }
    }
    let mut out: BTreeMap<String, Vec<FnRowRef>> = BTreeMap::new();
    for (name, leaves) in fn_to_leaves {
        // If the function has no scalar row, still surface it — this
        // catches aggregate/table/window functions whose verification
        // path isn't wired yet. Their status renders as "?".
        let (status, last_verified_at, case_count) = match per_fn.get(&name) {
            Some(r) => (
                r.status.clone(),
                r.last_verified_at.clone(),
                r.case_count,
            ),
            None => {
                // Look the case count up directly (aggregate/window/…
                // that isn't in scalars).
                let cnt: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM test_cases
                        WHERE extension = ?1 AND function_name = ?2",
                    params![extension, &name],
                    |r| r.get(0),
                )?;
                ("?".to_string(), None, cnt)
            }
        };
        let rr = FnRowRef {
            name: name.clone(),
            status,
            last_verified_at,
            case_count,
        };
        if leaves.is_empty() {
            out.entry("<untagged>".to_string()).or_default().push(rr);
        } else {
            for l in leaves {
                out.entry(l).or_default().push(rr.clone());
            }
        }
    }
    // Sort each leaf's rows by function name for stable diffs.
    for rows in out.values_mut() {
        rows.sort_by(|a, b| a.name.cmp(&b.name));
    }
    Ok(out)
}

#[derive(Clone)]
struct FnRowRef {
    name: String,
    status: String,
    last_verified_at: Option<String>,
    case_count: i64,
}
