//! `regress-scraper` -- scrape upstream regress corpora into
//! `test_cases` rows (B2).
//!
//! MVP scope, per B2 design:
//!   - regex-first parse (comment strip, statement split, top-level
//!     function-call extraction)
//!   - label-first expected alignment (positional fallback disabled
//!     in the MVP; unlabeled SELECTs are skipped rather than aligned
//!     positionally, since desyncs are silent and expensive to
//!     debug -- see design §5)
//!   - two output modes: TOML (`--out`) and direct-DB
//!     (`--insert-into`)
//!   - filename bulk-tag whitelist per design §4.A
//!   - pg-only skip patterns
//!
//! Not yet implemented (deferred): positional alignment with
//! agreement check, `test_cases_skipped` sibling table, MobilityDB
//! aligned-psql multi-line parser (a lightweight best-effort
//! extractor is in place).

mod emit;
mod normalise;
mod parse;
mod preprocess;
mod skiplist;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use walkdir::WalkDir;

use emit::NormalisedCase;

#[derive(Parser, Debug)]
#[command(
    name = "regress-scraper",
    about = "Scrape upstream regress corpora into shim-interface test_cases rows (B2)."
)]
struct Cli {
    /// Root of the regress corpus. PostGIS: the `regress/` subtree.
    /// MobilityDB: `mobilitydb/test/`.
    #[arg(long)]
    regress_dir: PathBuf,

    /// Extension. Sets the FN prefix, expected-suffix rule, and
    /// layout profile.
    #[arg(long, value_enum)]
    extension: Extension,

    /// TOML output.
    #[arg(long)]
    out: Option<PathBuf>,

    /// Direct-DB sink. Mutually exclusive with `--out`.
    #[arg(long)]
    insert_into: Option<PathBuf>,

    /// Provenance label; defaults to `<extension>_regress`.
    #[arg(long)]
    source_tag: Option<String>,

    /// Parse+report, write nothing.
    #[arg(long)]
    dry_run: bool,

    /// Cap cases per function.
    #[arg(long)]
    limit: Option<usize>,

    /// Drop cases whose expected looks like WKB hex.
    #[arg(long, default_value_t = true)]
    skip_binary: bool,

    /// Skip files matching this substring (repeatable).
    #[arg(long)]
    exclude: Vec<String>,

    /// Only include files matching this substring.
    #[arg(long)]
    include: Option<String>,

    /// Path to the extension's `*-catalog.toml`. When set, every
    /// emitted case is tagged with the owning `leaf:<leaf>` (or
    /// `leaf:orphan` if the function is not owned by any leaf).
    /// Defaults to `~/git/<extension>-shim-interface/<extension>-catalog.toml`.
    #[arg(long)]
    catalog: Option<PathBuf>,

    /// If set, emit pg_only-matching cases with a `pg_only` tag
    /// instead of dropping them. `test-fn` skips these in batch
    /// mode by default; use `--show-pg-only` in coverage reports
    /// to inspect the bucket.
    #[arg(long, default_value_t = true)]
    emit_pg_only: bool,

    /// Verbose logging.
    #[arg(long, short)]
    verbose: bool,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Extension {
    Postgis,
    Mobilitydb,
}

impl Extension {
    fn as_str(&self) -> &'static str {
        match self {
            Extension::Postgis => "postgis",
            Extension::Mobilitydb => "mobilitydb",
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if !cli.dry_run && cli.out.is_none() && cli.insert_into.is_none() {
        bail!("one of --out, --insert-into, or --dry-run is required");
    }
    if cli.out.is_some() && cli.insert_into.is_some() {
        bail!("--out and --insert-into are mutually exclusive");
    }

    let ext_name = cli.extension.as_str().to_string();
    let source_tag = cli
        .source_tag
        .clone()
        .unwrap_or_else(|| format!("{}_regress", ext_name));

    // Union of self-hosted inventory (if we have a DB) and prefix
    // regex. Inventory catches WIT-mangled internal names; prefix
    // catches user-facing SQL identifiers that the WIT surface
    // doesn't advertise verbatim (this is common for MobilityDB
    // where regress uses `asText`, `set(...)`, `intset '..'` etc.
    // while the DB stores things like `bitemporal_text_ever_...`).
    let inventory_names = if let Some(db) = &cli.insert_into {
        load_function_inventory(db, &ext_name).unwrap_or_default()
    } else {
        Default::default()
    };
    if cli.verbose {
        eprintln!(
            "inventory: {} names loaded (union with prefix predicate)",
            inventory_names.len()
        );
    }
    let prefix_pred = parse::default_prefix_predicate(&ext_name);
    let predicate: Box<dyn Fn(&str) -> bool + Send + Sync> =
        Box::new(move |ident: &str| inventory_names.contains(ident) || prefix_pred(ident));

    // Catalog-driven leaf tagging (Fix 3). If --catalog is not
    // explicitly provided, guess the canonical path derived from
    // --insert-into's parent directory, then fall back to the
    // conventional `~/git/<extension>-shim-interface/<extension>-catalog.toml`.
    let catalog_path = cli.catalog.clone().or_else(|| {
        cli.insert_into.as_ref().and_then(|db| {
            let parent = db.parent()?;
            let name = format!("{ext_name}-catalog.toml");
            let p = parent.join(&name);
            if p.exists() {
                Some(p)
            } else {
                None
            }
        })
    });
    let leaf_map: HashMap<String, String> = match &catalog_path {
        Some(p) => match preprocess::load_function_leaf_map(p) {
            Ok(m) => {
                if cli.verbose {
                    eprintln!(
                        "catalog: {} functions mapped to leaves from {}",
                        m.len(),
                        p.display()
                    );
                }
                m
            }
            Err(e) => {
                eprintln!("warn: catalog load failed ({}): {}", p.display(), e);
                HashMap::new()
            }
        },
        None => {
            if cli.verbose {
                eprintln!("no catalog found; leaf tagging disabled");
            }
            HashMap::new()
        }
    };

    let sql_files = collect_sql_files(&cli.regress_dir, cli.extension, &cli.include, &cli.exclude)?;
    if cli.verbose {
        eprintln!("found {} sql files", sql_files.len());
    }

    let mut cases: Vec<NormalisedCase> = Vec::new();
    let mut per_fn_counts: HashMap<String, usize> = HashMap::new();
    let mut skipped_pg_only = 0usize;
    let mut skipped_no_top = 0usize;
    let mut skipped_no_label = 0usize;
    let mut skipped_no_expected = 0usize;
    let mut skipped_binary = 0usize;
    let mut files_no_expected = 0usize;

    for sql_path in &sql_files {
        let raw = match std::fs::read_to_string(sql_path) {
            Ok(s) => s,
            Err(e) => {
                if cli.verbose {
                    eprintln!("skip {}: {}", sql_path.display(), e);
                }
                continue;
            }
        };
        // Resolve expected file.
        let Some(expected_path) = resolve_expected(sql_path, cli.extension) else {
            files_no_expected += 1;
            if cli.verbose {
                eprintln!("no expected file for {}", sql_path.display());
            }
            continue;
        };
        let expected_raw = match std::fs::read_to_string(&expected_path) {
            Ok(s) => s,
            Err(_) => {
                files_no_expected += 1;
                continue;
            }
        };
        let expected_by_label =
            build_label_index(&expected_raw, cli.extension);

        let stripped = parse::strip_comments(&raw);
        let stmts = parse::split_statements(&stripped);

        // MobilityDB has no per-SELECT labels; instead the aligned
        // .out file echoes each SELECT verbatim followed by its
        // data row(s). Build a `stmt_text -> data_row` index that
        // captures the row after each echoed `SELECT ...;` line.
        let expected_by_echo: HashMap<String, String> = if matches!(cli.extension, Extension::Mobilitydb) {
            build_echo_index(&expected_raw)
        } else {
            HashMap::new()
        };

        let bulk_tag = bulk_tag_for(sql_path, cli.extension);
        let file_basename = sql_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        for stmt in &stmts {
            let lc = stmt.text.to_ascii_lowercase();
            if !lc.trim_start().starts_with("select") {
                continue;
            }
            // pg_only detection now flags the case instead of
            // dropping it (Fix 5). Passing `is_pg_only` down keeps
            // the flag close to the emit site.
            let pg_only_pattern = skiplist::matches_pg_only(&lc);
            let is_pg_only = pg_only_pattern.is_some();
            if is_pg_only && !cli.emit_pg_only {
                skipped_pg_only += 1;
                continue;
            }
            let top_calls = parse::extract_top_calls(&stmt.text, &*predicate);
            if top_calls.is_empty() {
                skipped_no_top += 1;
                continue;
            }
            let expected_row_owned: String;
            let expected_row: &str = match cli.extension {
                Extension::Postgis => {
                    let Some(label) = parse::extract_label(&stmt.text) else {
                        skipped_no_label += 1;
                        continue;
                    };
                    let Some(row) = expected_by_label.get(&label) else {
                        skipped_no_expected += 1;
                        continue;
                    };
                    row.as_str()
                }
                Extension::Mobilitydb => {
                    // Normalize whitespace and lookup.
                    let key = normalise_echo_key(&stmt.text);
                    let Some(row) = expected_by_echo.get(&key) else {
                        skipped_no_expected += 1;
                        continue;
                    };
                    expected_row_owned = row.clone();
                    expected_row_owned.as_str()
                }
            };
            if cli.skip_binary && normalise::looks_like_binary(expected_row) {
                skipped_binary += 1;
                continue;
            }
            let expected_norm = normalise::normalise_psql_to_duckdb(expected_row);

            // Emit one case per top-level function call.
            for (idx, tc) in top_calls.iter().enumerate() {
                let disamb = if top_calls.len() > 1 {
                    format!("_{}", char::from(b'a' + idx as u8))
                } else {
                    String::new()
                };
                let case_name = format!(
                    "{}_{}{}",
                    sanitise_name(&file_basename),
                    stmt.start_line,
                    disamb
                );
                let cur = per_fn_counts.entry(tc.function.clone()).or_insert(0);
                if let Some(limit) = cli.limit {
                    if *cur >= limit {
                        continue;
                    }
                }
                *cur += 1;

                let mut tags: Vec<String> =
                    vec![source_tag.clone()];
                // Catalog-driven leaf tag (Fix 3). Prefer the
                // canonical catalog assignment; fall back to the
                // scraper's per-filename bulk-tag when the catalog
                // has no mapping (unknown function or catalog not
                // loaded). Guarantees every emitted row carries at
                // least one `leaf:*` tag so coverage-by-leaf never
                // dumps rows into `<untagged>`.
                let catalog_leaf =
                    leaf_map.get(&tc.function.to_ascii_lowercase()).cloned();
                match (&catalog_leaf, &bulk_tag) {
                    (Some(leaf), _) => tags.push(format!("leaf:{}", leaf)),
                    (None, Some(bt)) => tags.push(bt.clone()),
                    (None, None) => tags.push("leaf:orphan".to_string()),
                }
                if is_pg_only {
                    tags.push("pg_only".to_string());
                    if let Some(pat) = pg_only_pattern {
                        tags.push(format!("pg_only_pattern:{}", pat));
                    }
                }
                for other in &top_calls {
                    if other.function != tc.function {
                        tags.push(format!("inner:{}", other.function));
                    }
                }

                // Strip the leading label literal so the probe
                // returns only the expression under test (not the
                // hand-authored label as an extra CSV column).
                let stripped = parse::strip_leading_label(&stmt.text);
                // Inject `::GEOMETRY` casts on WKT string literals
                // so the DuckDB binder matches the shim's
                // `(postgis.geometry, ...)` signatures (Fix 1).
                let probe_sql = preprocess::inject_geometry_casts(&stripped);
                cases.push(NormalisedCase {
                    extension: ext_name.clone(),
                    function_name: tc.function.clone(),
                    case_name,
                    source: source_tag.clone(),
                    source_path: sql_path.to_string_lossy().to_string(),
                    sql_inline: probe_sql,
                    expected: expected_norm.clone(),
                    tags,
                });
            }
        }
    }

    println!("scrape summary for {}:", ext_name);
    println!("  files scanned:                {}", sql_files.len());
    println!("  files with no expected file:  {}", files_no_expected);
    println!("  cases produced:               {}", cases.len());
    println!("  distinct functions covered:   {}", distinct_fn_count(&cases));
    println!("  skipped (pg_only):            {}", skipped_pg_only);
    println!("  skipped (no top-level call):  {}", skipped_no_top);
    println!("  skipped (no label):           {}", skipped_no_label);
    println!("  skipped (no expected row):    {}", skipped_no_expected);
    println!("  skipped (binary):             {}", skipped_binary);

    if cli.dry_run {
        return Ok(());
    }
    if let Some(out) = &cli.out {
        emit::write_toml(&cases, out)?;
        eprintln!("wrote toml to {}", out.display());
    } else if let Some(db) = &cli.insert_into {
        let (ins, upd) = emit::insert_into_db(&cases, db)?;
        eprintln!(
            "wrote {} inserted, {} updated to {}",
            ins,
            upd,
            db.display()
        );
    }
    Ok(())
}

fn distinct_fn_count(cases: &[NormalisedCase]) -> usize {
    let s: HashSet<&str> = cases.iter().map(|c| c.function_name.as_str()).collect();
    s.len()
}

fn sanitise_name(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn collect_sql_files(
    root: &Path,
    ext: Extension,
    include: &Option<String>,
    exclude: &[String],
) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in WalkDir::new(root).follow_links(false).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.into_path();
        let is_sql = match ext {
            Extension::Postgis => name.ends_with(".sql"),
            Extension::Mobilitydb => name.ends_with(".sql") || name.ends_with(".test.sql"),
        };
        if !is_sql {
            continue;
        }
        let full = path.to_string_lossy().to_string();
        if let Some(pat) = include {
            if !full.contains(pat) {
                continue;
            }
        }
        if exclude.iter().any(|e| full.contains(e)) {
            continue;
        }
        out.push(path);
    }
    out.sort();
    Ok(out)
}

fn resolve_expected(sql_path: &Path, ext: Extension) -> Option<PathBuf> {
    match ext {
        Extension::Postgis => {
            let parent = sql_path.parent()?;
            let stem = sql_path.file_stem()?.to_string_lossy().to_string();
            let candidates = [
                format!("{}_expected", stem),
                format!("{}_expected.geos312", stem),
                format!("{}.expected", stem),
            ];
            for c in &candidates {
                let p = parent.join(c);
                if p.exists() {
                    return Some(p);
                }
            }
            None
        }
        Extension::Mobilitydb => {
            // Path shape: .../queries/NN_dir/NNN_name.test.sql
            //          -> .../expected/NN_dir/NNN_name.test.out
            // But MobilityDB layout observed on-disk was
            //          .../temporal/queries/NNN_name.test.sql
            //       -> .../temporal/expected/NNN_name.test.out
            let stem = sql_path.file_name()?.to_string_lossy();
            let out_name = stem.strip_suffix(".sql")?.to_string() + ".out";
            let path_str = sql_path.to_string_lossy().to_string();
            let candidate = path_str.replace("/queries/", "/expected/");
            let mut p = PathBuf::from(candidate);
            p.set_file_name(out_name);
            if p.exists() {
                Some(p)
            } else {
                None
            }
        }
    }
}

fn bulk_tag_for(sql_path: &Path, ext: Extension) -> Option<String> {
    let name = sql_path.file_name()?.to_string_lossy().to_string();
    let parent_name = sql_path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    match ext {
        Extension::Postgis => {
            let table: &[(&str, &str)] = &[
                ("constructors.sql", "constructors"),
                ("ctors.sql", "constructors"),
                ("regress_ogc.sql", "constructors"),
                ("wkt.sql", "wkt_io"),
                ("wkb.sql", "wkb_io"),
                ("ewkt.sql", "wkt_io"),
                ("measures.sql", "measurement"),
                ("distance.sql", "measurement"),
                ("regress_lrs.sql", "linear_ref"),
                ("lrs.sql", "linear_ref"),
                ("affine.sql", "affine"),
                ("bbox_ops.sql", "bbox"),
                ("regress_bbox.sql", "bbox"),
                ("cluster.sql", "clustering"),
                ("regress_geography.sql", "geography"),
                ("geography_measure.sql", "geography"),
                ("trajectory.sql", "trajectory"),
                ("boundary.sql", "predicates"),
                ("relate.sql", "predicates"),
                ("simplify.sql", "simplification"),
                ("split.sql", "editors"),
                ("in_geohash.sql", "geohash_io"),
                ("out_geojson.sql", "geojson_io"),
                ("in_geojson.sql", "geojson_io"),
                ("out_svg.sql", "svg_io"),
                ("out_x3d.sql", "x3d_io"),
                ("in_kml.sql", "kml_io"),
                ("out_kml.sql", "kml_io"),
                ("in_gml.sql", "gml_io"),
                ("out_gml.sql", "gml_io"),
            ];
            for (fname, tag) in table {
                if &name == fname {
                    return Some(format!("leaf:{}", tag));
                }
            }
            None
        }
        Extension::Mobilitydb => {
            let dir_table: &[(&str, &str)] = &[
                ("temporal", "temporal"),
                ("point", "tpoint"),
                ("rgeo", "tpoint"),
                ("npoint", "npoint"),
                ("h3", "h3"),
                ("cbuffer", "cbuffer"),
                ("pose", "pose"),
                ("static_analysis", "static_analysis"),
            ];
            for (dir, tag) in dir_table {
                if parent_name.contains(dir) {
                    return Some(format!("leaf:{}", tag));
                }
            }
            None
        }
    }
}

/// Build a label -> expected-row-string index. For PostGIS the
/// expected file is `psql -tA` output, so each row is a single
/// `label|v1|v2|...` line and the label maps to the pipe-joined
/// tail. For MobilityDB, the expected is aligned psql output with
/// `SELECT ...;` echoes; we key by the label extracted from the
/// echoed SELECT statement.
fn build_label_index(expected_raw: &str, ext: Extension) -> HashMap<String, String> {
    let mut idx = HashMap::new();
    match ext {
        Extension::Postgis => {
            for line in expected_raw.lines() {
                let l = line.trim_end();
                if l.is_empty() {
                    continue;
                }
                if let Some(sep) = l.find('|') {
                    let label = l[..sep].trim().to_string();
                    let rest = l[sep + 1..].trim().to_string();
                    // First-wins: don't overwrite if label repeats.
                    idx.entry(label).or_insert(rest);
                }
            }
        }
        Extension::Mobilitydb => {
            // Best-effort: MobilityDB expected files are aligned
            // psql output. Look for the pattern:
            //   SELECT 'label', ...;
            //   ...
            //    row_data
            //   (1 row)
            // and map label -> row_data.
            let mut current_label: Option<String> = None;
            let mut collecting_body = false;
            let mut body_buf: Vec<String> = Vec::new();
            for line in expected_raw.lines() {
                if line.trim_start().starts_with("SELECT ") {
                    // Save any prior body against its label.
                    if let Some(lbl) = current_label.take() {
                        if !body_buf.is_empty() {
                            idx.entry(lbl)
                                .or_insert(body_buf.join(" ").trim().to_string());
                        }
                    }
                    body_buf.clear();
                    // Extract label after `SELECT '`.
                    if let Some(open) = line.find('\'') {
                        if let Some(close) = line[open + 1..].find('\'') {
                            let lbl = &line[open + 1..open + 1 + close];
                            current_label = Some(lbl.to_string());
                            collecting_body = true;
                            continue;
                        }
                    }
                    collecting_body = false;
                    continue;
                }
                if !collecting_body || current_label.is_none() {
                    continue;
                }
                let t = line.trim();
                if t.starts_with("(1 row)") || t.starts_with("(0 rows)") {
                    if let Some(lbl) = current_label.take() {
                        idx.entry(lbl)
                            .or_insert(body_buf.join(" ").trim().to_string());
                    }
                    body_buf.clear();
                    collecting_body = false;
                    continue;
                }
                // Skip header/separator lines and column-name row.
                if t.chars().all(|c| c == '-' || c.is_whitespace()) {
                    continue;
                }
                body_buf.push(t.to_string());
            }
        }
    }
    idx
}

/// Normalise whitespace in a SQL statement to a single-space form
/// so `SELECT   a\n,b;` and `SELECT a, b` hash the same in the
/// echo-alignment index.
fn normalise_echo_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = true;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
        } else {
            out.push(c);
            prev_ws = false;
        }
    }
    let mut t = out.trim().to_string();
    // Strip trailing ;
    if t.ends_with(';') {
        t.pop();
    }
    t.trim().to_string()
}

/// Aligned-psql expected file -> map keyed by normalised echoed
/// SELECT text, valued by the joined data-row body. Handles the
/// psql pattern:
///     SELECT expr;
///     <col header>
///     ------
///      <data>
///     (1 row)
fn build_echo_index(expected_raw: &str) -> HashMap<String, String> {
    let mut idx: HashMap<String, String> = HashMap::new();
    let lines: Vec<&str> = expected_raw.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        // Look for a psql echo (SELECT ... ending in ;).
        if trimmed.starts_with("SELECT ") || trimmed.starts_with("SELECT\n") {
            // Collect the echo across possible continuation lines
            // until we see the terminating `;` at end-of-line.
            let mut echo = trimmed.to_string();
            let mut j = i;
            while j < lines.len() && !lines[j].trim_end().ends_with(';') {
                j += 1;
                if j < lines.len() {
                    echo.push(' ');
                    echo.push_str(lines[j].trim_start());
                }
            }
            let key = normalise_echo_key(&echo);
            // Advance past the echo.
            let mut k = j + 1;
            // Skip the column-name header row (may be blank).
            while k < lines.len() && lines[k].trim().is_empty() {
                k += 1;
            }
            if k < lines.len() {
                // The header. Skip.
                k += 1;
                // Skip the separator dashes.
                if k < lines.len()
                    && lines[k].trim_start().chars().all(|c| c == '-' || c.is_whitespace())
                {
                    k += 1;
                }
                // Collect data-row lines until `(N rows)` marker.
                let mut body: Vec<String> = Vec::new();
                while k < lines.len() {
                    let t = lines[k].trim();
                    if t.starts_with('(') && t.ends_with("row)") {
                        break;
                    }
                    if t.starts_with('(') && t.ends_with("rows)") {
                        break;
                    }
                    if t.starts_with("ERROR:") {
                        // Skip errors -- not testable.
                        break;
                    }
                    if !t.is_empty() {
                        body.push(t.to_string());
                    }
                    k += 1;
                }
                // Only emit single-row bodies to align with the
                // downstream runner's last-line heuristic.
                if body.len() == 1 {
                    idx.entry(key).or_insert_with(|| body[0].clone());
                }
            }
            i = j + 1;
            continue;
        }
        i += 1;
    }
    idx
}

fn load_function_inventory(db: &Path, extension: &str) -> Result<HashSet<String>> {
    if !db.exists() {
        return Ok(HashSet::new());
    }
    let conn = rusqlite::Connection::open(db).with_context(|| format!("open {}", db.display()))?;
    let mut stmt =
        conn.prepare("SELECT lower(name) FROM scalars WHERE extension = ?1")?;
    let names: HashSet<String> = stmt
        .query_map(rusqlite::params![extension], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(names)
}
