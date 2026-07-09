//! Output writers: TOML and direct SQLite insertion.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

/// Normalised, sink-agnostic case rowsafe for both TOML and SQLite.
#[derive(Debug, Clone, Serialize)]
pub struct NormalisedCase {
    pub extension: String,
    pub function_name: String,
    pub case_name: String,
    pub source: String,
    pub source_path: String,
    pub sql_inline: String,
    pub expected: String,
    pub tags: Vec<String>,
}

/// TOML shape mirroring `test-fn seed`'s `CaseToml`.
#[derive(Debug, Clone, Serialize)]
struct CaseTomlOut {
    name: String,
    sql: String,
    expects: String,
    source: String,
}

/// Group `NormalisedCase` rows by `function_name` and serialise as
/// TOML matching the seed-input shape.
pub fn write_toml(cases: &[NormalisedCase], out: &Path) -> Result<()> {
    let mut by_fn: BTreeMap<String, Vec<CaseTomlOut>> = BTreeMap::new();
    for c in cases {
        by_fn
            .entry(c.function_name.clone())
            .or_default()
            .push(CaseTomlOut {
                name: c.case_name.clone(),
                sql: c.sql_inline.clone(),
                expects: c.expected.clone(),
                source: c.source.clone(),
            });
    }
    let s = toml::to_string_pretty(&by_fn).context("serialise toml")?;
    if out.as_os_str() == "-" {
        print!("{}", s);
    } else {
        std::fs::write(out, s).with_context(|| format!("write {}", out.display()))?;
    }
    Ok(())
}

/// Insert `NormalisedCase` rows into `test_cases` on the given
/// SQLite DB. Uses UPDATE-then-INSERT-OR-IGNORE per B2 §2.
pub fn insert_into_db(cases: &[NormalisedCase], db_path: &Path) -> Result<(usize, usize)> {
    let conn =
        Connection::open(db_path).with_context(|| format!("open {}", db_path.display()))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let mut inserted = 0usize;
    let mut updated = 0usize;
    let tx = conn.unchecked_transaction()?;
    for c in cases {
        let tags = serde_json::to_string(&c.tags).unwrap_or_else(|_| "[]".to_string());
        let updated_now = tx.execute(
            "UPDATE test_cases SET
                source = ?4,
                source_path = ?5,
                sql_inline = ?6,
                expected = ?7,
                tags_json = ?8
             WHERE extension = ?1
               AND function_name = ?2
               AND case_name = ?3",
            params![
                c.extension,
                c.function_name,
                c.case_name,
                c.source,
                c.source_path,
                c.sql_inline,
                c.expected,
                tags,
            ],
        )?;
        let inserted_now = tx.execute(
            "INSERT OR IGNORE INTO test_cases
                (extension, function_name, case_name,
                 source, source_path, sql_inline, expected,
                 tags_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                c.extension,
                c.function_name,
                c.case_name,
                c.source,
                c.source_path,
                c.sql_inline,
                c.expected,
                tags,
            ],
        )?;
        if inserted_now == 1 {
            inserted += 1;
        } else if updated_now == 1 {
            updated += 1;
        }
    }
    tx.commit()?;
    Ok((inserted, updated))
}
