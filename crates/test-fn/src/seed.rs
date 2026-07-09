//! TOML → `test_cases` upsert. The authoring TOML format matches
//! the design's §3 sketch: one table-array per function, each
//! entry has `name`, `sql`, and `expects` keys, optional `setup`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::params;
use serde::Deserialize;

use crate::db;

#[derive(Debug, Deserialize)]
struct CaseToml {
    name: String,
    sql: String,
    expects: String,
    #[serde(default)]
    setup: Vec<String>,
    #[serde(default)]
    source: Option<String>,
}

pub fn run(
    interface_db: &Path,
    extension: &str,
    from: &Path,
    replace: bool,
) -> Result<()> {
    let text = std::fs::read_to_string(from)
        .with_context(|| format!("read {}", from.display()))?;
    // Parse as a map of function-name -> Vec<CaseToml>. This maps
    // the design's `[[st_makepoint]]` shape verbatim.
    let by_fn: BTreeMap<String, Vec<CaseToml>> =
        toml::from_str(&text).with_context(|| format!("parse {}", from.display()))?;
    let conn = db::open(interface_db)?;
    let source_path = from.to_string_lossy().to_string();
    let mut inserted = 0usize;
    let mut updated = 0usize;
    for (fn_name, cases) in by_fn.iter() {
        for c in cases {
            // MVP: `setup` is folded into the probe SQL; §3 says
            // canonical form is a single probe statement. If any
            // authored case uses setup we materialise it inline.
            let sql = if c.setup.is_empty() {
                c.sql.clone()
            } else {
                let mut buf = String::new();
                for s in &c.setup {
                    buf.push_str(s);
                    if !s.trim_end().ends_with(';') {
                        buf.push(';');
                    }
                    buf.push('\n');
                }
                buf.push_str(&c.sql);
                buf
            };
            let src = c.source.as_deref().unwrap_or("handrolled");
            let mode = if replace { "REPLACE" } else { "IGNORE" };
            let n = conn.execute(
                &format!(
                    "INSERT OR {mode} INTO test_cases
                        (extension, function_name, case_name,
                         source, source_path, sql_inline, expected,
                         tags_json)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,'[]')"
                ),
                params![
                    extension,
                    fn_name,
                    c.name,
                    src,
                    source_path,
                    sql,
                    c.expects,
                ],
            )?;
            if n == 1 {
                inserted += 1;
            } else if replace {
                updated += 1;
            }
        }
    }
    println!(
        "seed: {} inserted, {} skipped-or-updated ({} total in file)",
        inserted,
        updated,
        by_fn.values().map(|v| v.len()).sum::<usize>()
    );
    Ok(())
}
