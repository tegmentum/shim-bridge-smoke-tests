//! Integration coverage for the batch runner's `fixture_bad` skip
//! logic (§7 promotion + skip behaviour). Ensures:
//!
//!   1. Cases tagged `fixture_bad` are skipped: no subprocess is
//!      launched, no `test_runs` row is inserted, no fail counter
//!      increments.
//!   2. A function whose only cases are `fixture_bad` retains its
//!      prior status (no demote, no promote).
//!   3. A function mixing `fixture_bad` + passing cases promotes to
//!      `implemented_verified` on all passes.
//!   4. A function mixing `fixture_bad` + failing cases demotes to
//!      `implemented_unverified` (mark_failed) even when the
//!      failing case count is < planned (fixture_bad excluded from
//!      the denominator).
//!
//! The test drives the real `test-fn` binary via a scratch sqlite
//! DB and a shell-script "ducklink" whose exit code / stdout are
//! parametrised per case by inspecting the SQL text.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::{params, Connection};

/// Build the sqlite schema used by `shim-interface-core`. Kept
/// deliberately minimal — only the columns test-fn reads/writes.
fn init_schema(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE scalars (
             extension TEXT NOT NULL,
             name TEXT NOT NULL,
             param_types_json TEXT NOT NULL DEFAULT '[]',
             return_type TEXT NOT NULL DEFAULT 'text',
             is_deterministic INTEGER NOT NULL DEFAULT 1,
             propagates_null INTEGER NOT NULL DEFAULT 1,
             interface TEXT,
             first_seen_upstream_version TEXT,
             last_seen_upstream_version TEXT,
             deprecated_in_upstream_version TEXT,
             signature_hash TEXT,
             implementation_hash TEXT,
             status TEXT NOT NULL DEFAULT 'implemented_unverified',
             last_verified_upstream_version TEXT,
             last_verified_signature_hash TEXT,
             last_verified_implementation_hash TEXT,
             last_verified_at TEXT,
             notes TEXT,
             PRIMARY KEY (extension, name)
         );
         CREATE TABLE test_cases (
             extension TEXT NOT NULL,
             function_name TEXT NOT NULL,
             case_name TEXT NOT NULL,
             source TEXT NOT NULL,
             source_path TEXT,
             sql_inline TEXT,
             expected TEXT,
             tags_json TEXT NOT NULL DEFAULT '[]',
             PRIMARY KEY (extension, function_name, case_name)
         );
         CREATE TABLE test_runs (
             run_id INTEGER PRIMARY KEY AUTOINCREMENT,
             extension TEXT NOT NULL,
             function_name TEXT NOT NULL,
             case_name TEXT NOT NULL,
             status TEXT NOT NULL,
             actual TEXT,
             duration_ms INTEGER,
             host_version TEXT,
             provider_wasm_hash TEXT,
             bridge_wasm_hash TEXT,
             upstream_version TEXT,
             ran_at TEXT NOT NULL,
             FOREIGN KEY (extension, function_name, case_name)
                 REFERENCES test_cases(extension, function_name, case_name)
         );",
    )
    .unwrap();
}

fn insert_scalar(conn: &Connection, name: &str, status: &str, sig: &str, im: &str) {
    conn.execute(
        "INSERT INTO scalars
            (extension, name, param_types_json, return_type,
             is_deterministic, propagates_null,
             signature_hash, implementation_hash,
             status,
             last_verified_signature_hash,
             last_verified_implementation_hash,
             last_verified_at)
         VALUES (?1, ?2, '[]', 'text', 1, 1, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            "postgis",
            name,
            sig,
            im,
            status,
            // If the row starts already verified we pre-fill the
            // last_verified_* columns so `is_cache_hit` semantics
            // reflect a real prior verified state (though these
            // tests exercise batch mode, which doesn't consult
            // the cache; we still store them for realism).
            if status == "implemented_verified" { Some(sig) } else { None },
            if status == "implemented_verified" { Some(im) } else { None },
            if status == "implemented_verified" {
                Some("2026-07-08T00:00:00Z")
            } else {
                None
            },
        ],
    )
    .unwrap();
}

fn insert_case(
    conn: &Connection,
    function: &str,
    case: &str,
    sql: &str,
    expected: &str,
    tags_json: &str,
) {
    conn.execute(
        "INSERT INTO test_cases
            (extension, function_name, case_name, source,
             source_path, sql_inline, expected, tags_json)
         VALUES ('postgis', ?1, ?2, 'test', 'inline', ?3, ?4, ?5)",
        params![function, case, sql, expected, tags_json],
    )
    .unwrap();
}

/// Write a shell script that mimics `ducklink` for the purposes
/// of the batch runner: it reads SQL from stdin, greps for a
/// tag token that identifies the expected outcome (`OK:<val>` for
/// pass or `BAD:<val>` for fail), and prints the value at the end
/// so the runner's stdout parser picks it up. The runner compares
/// this against `test_cases.expected`.
fn write_fake_ducklink(dir: &Path) -> PathBuf {
    let sh_path = dir.join("fake-ducklink");
    let body = r#"#!/usr/bin/env bash
# Fake ducklink: extract the OK:<v> or BAD:<v> tag from the
# incoming SQL and print `<v>` on stdout so the runner's parser
# treats it as the case's actual output.
set -euo pipefail
# skip flags; last positional arg is the DB path (unused).
input="$(cat)"
val="$(printf '%s' "$input" | grep -oE '(OK|BAD):[a-zA-Z0-9_-]+' | head -1 || true)"
if [ -z "$val" ]; then
    echo "UNKNOWN"
    exit 0
fi
kind="${val%%:*}"
payload="${val##*:}"
if [ "$kind" = "OK" ]; then
    echo "$payload"
else
    # Print an intentionally-wrong value so the runner marks it as
    # a mismatch (`fail`) — the case's `expected` column holds the
    # `OK` payload, and this deliberately doesn't match.
    echo "WRONG-$payload"
fi
"#;
    fs::write(&sh_path, body).unwrap();
    let mut perm = fs::metadata(&sh_path).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perm.set_mode(0o755);
    }
    fs::set_permissions(&sh_path, perm).unwrap();
    sh_path
}

/// Locate the built `test-fn` binary. Cargo puts integration-test
/// dep binaries alongside the test executable under
/// `target/<profile>/deps/`, but the *bin* binary is one level
/// up. Fall back to the workspace target dir if `CARGO_BIN_EXE_test-fn`
/// isn't set (older cargo, or manual invocation).
fn test_fn_binary() -> PathBuf {
    if let Some(p) = option_env!("CARGO_BIN_EXE_test-fn") {
        return PathBuf::from(p);
    }
    // Fallback: assume we ran `cargo build -p test-fn` first.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_target = Path::new(manifest_dir)
        .parent()
        .and_then(Path::parent)
        .map(|p| p.join("target"))
        .expect("no workspace target");
    for profile in &["debug", "release"] {
        let candidate = workspace_target.join(profile).join("test-fn");
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("test-fn binary not found; run `cargo build -p test-fn` first");
}

/// Read the (single) status stored in `scalars` for a given
/// function name.
fn status_of(conn: &Connection, function: &str) -> String {
    conn.query_row(
        "SELECT status FROM scalars WHERE extension = 'postgis' AND name = ?1",
        params![function],
        |r| r.get::<_, String>(0),
    )
    .unwrap()
}

/// Row count in `test_runs` for `(extension, function)`.
fn test_run_count(conn: &Connection, function: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM test_runs
         WHERE extension = 'postgis' AND function_name = ?1",
        params![function],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
}

/// End-to-end batch invocation covering the four scenarios listed
/// in the module docstring. One test instead of four to amortise
/// the (slow) subprocess spawn.
#[test]
fn batch_fixture_bad_skip_matrix() {
    // Skip in CI-ish sandboxes without a temp dir. Cargo integration
    // tests always run with a writable tempdir, but be defensive.
    let tmp = tempfile::tempdir().expect("tempdir");

    // Scratch interface DB.
    let db_path = tmp.path().join("interface.sqlite");
    {
        let conn = Connection::open(&db_path).unwrap();
        init_schema(&conn);

        // fn_all_bad — one case, fixture_bad. Should stay
        // `implemented_verified`.
        insert_scalar(&conn, "fn_all_bad", "implemented_verified", "sig-a", "im-a");
        insert_case(
            &conn,
            "fn_all_bad",
            "c1",
            "-- SQL: OK:apple\nSELECT 'apple';",
            "apple",
            "[\"leaf:test\",\"fixture_bad\"]",
        );

        // fn_mixed_pass — two cases, one fixture_bad + one pass.
        // Should promote to `implemented_verified`.
        insert_scalar(
            &conn,
            "fn_mixed_pass",
            "implemented_unverified",
            "sig-b",
            "im-b",
        );
        insert_case(
            &conn,
            "fn_mixed_pass",
            "c1",
            "-- SQL: BAD:banana\nSELECT 'banana';",
            "banana",
            "[\"leaf:test\",\"fixture_bad\"]",
        );
        insert_case(
            &conn,
            "fn_mixed_pass",
            "c2",
            "-- SQL: OK:banana\nSELECT 'banana';",
            "banana",
            "[\"leaf:test\"]",
        );

        // fn_mixed_fail — two cases, one fixture_bad + one fail.
        // Should demote to `implemented_unverified` and NULL the
        // last_verified_* hashes.
        insert_scalar(
            &conn,
            "fn_mixed_fail",
            "implemented_verified",
            "sig-c",
            "im-c",
        );
        insert_case(
            &conn,
            "fn_mixed_fail",
            "c1",
            "-- SQL: BAD:cherry\nSELECT 'cherry';",
            "cherry",
            "[\"leaf:test\",\"fixture_bad\"]",
        );
        insert_case(
            &conn,
            "fn_mixed_fail",
            "c2",
            "-- SQL: BAD:cherry\nSELECT 'cherry';",
            "cherry",
            "[\"leaf:test\"]",
        );

        // fn_all_pass — two normal cases, both pass. Sanity that
        // existing behaviour is unchanged.
        insert_scalar(
            &conn,
            "fn_all_pass",
            "implemented_unverified",
            "sig-d",
            "im-d",
        );
        insert_case(
            &conn,
            "fn_all_pass",
            "c1",
            "-- SQL: OK:date\nSELECT 'date';",
            "date",
            "[\"leaf:test\"]",
        );
        insert_case(
            &conn,
            "fn_all_pass",
            "c2",
            "-- SQL: OK:date\nSELECT 'date';",
            "date",
            "[\"leaf:test\"]",
        );
    }

    // Fake ducklink shell script.
    let ducklink = write_fake_ducklink(tmp.path());
    // Fake bridge wasm — its content is only hashed by the runner.
    let bridge = tmp.path().join("fake-bridge.wasm");
    fs::write(&bridge, b"stub-bridge-bytes").unwrap();

    // Invoke `test-fn batch --leaf test`.
    let bin = test_fn_binary();
    let out = Command::new(&bin)
        .arg("batch")
        .arg("--interface")
        .arg(&db_path)
        .arg("--extension")
        .arg("postgis")
        .arg("--leaf")
        .arg("test")
        .arg("--bridge")
        .arg(&bridge)
        .arg("--ducklink")
        .arg(&ducklink)
        .arg("--keep-going")
        .output()
        .expect("spawn test-fn");
    assert!(
        out.status.success(),
        "test-fn batch exited non-zero; stderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout),
    );

    // Re-open DB to inspect final state.
    let conn = Connection::open(&db_path).unwrap();

    // 1. All-bad function: no runs recorded, status preserved.
    assert_eq!(
        test_run_count(&conn, "fn_all_bad"),
        0,
        "fixture_bad-only fn should not produce test_runs rows"
    );
    assert_eq!(
        status_of(&conn, "fn_all_bad"),
        "implemented_verified",
        "fixture_bad-only fn must NOT be demoted"
    );

    // 2. Mixed-pass: exactly 1 run (the non-fixture_bad case),
    //    promoted to `implemented_verified`.
    assert_eq!(
        test_run_count(&conn, "fn_mixed_pass"),
        1,
        "fixture_bad case should have been skipped; only 1 run expected"
    );
    assert_eq!(
        status_of(&conn, "fn_mixed_pass"),
        "implemented_verified",
        "all non-skipped pass -> promote"
    );

    // 3. Mixed-fail: exactly 1 run (the non-fixture_bad case),
    //    demoted to `implemented_unverified`.
    assert_eq!(
        test_run_count(&conn, "fn_mixed_fail"),
        1,
        "fixture_bad case should have been skipped; only 1 run expected"
    );
    assert_eq!(
        status_of(&conn, "fn_mixed_fail"),
        "implemented_unverified",
        "any non-skipped fail -> demote"
    );

    // 4. All-pass baseline unchanged.
    assert_eq!(
        test_run_count(&conn, "fn_all_pass"),
        2,
        "both normal cases run"
    );
    assert_eq!(
        status_of(&conn, "fn_all_pass"),
        "implemented_verified",
        "all pass -> promote"
    );
}
