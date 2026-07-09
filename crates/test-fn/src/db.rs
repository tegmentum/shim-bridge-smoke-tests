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
    let mut sql = String::from(
        "SELECT extension, function_name, case_name,
                COALESCE(sql_inline, ''), COALESCE(expected, ''),
                source
         FROM test_cases
         WHERE extension = ?1 AND function_name = ?2",
    );
    if only.is_some() {
        sql.push_str(" AND case_name = ?3");
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
