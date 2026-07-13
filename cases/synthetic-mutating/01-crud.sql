-- Exercise every arm of the mutating-vtab dispatch surface AND
-- read-after-write visibility across the read/write dispatch tiers:
--   * xUpdate INSERT / UPDATE / DELETE via SQL DML
--   * xBegin / xCommit  via BEGIN + COMMIT
--   * xRollback         via BEGIN + ROLLBACK
--   * xConnect / xBestIndex / xFilter / xNext / xColumn / xRowid /
--     xFetchBatch through the SAME wasm instance that services
--     xUpdate — so writes are observable to subsequent reads within
--     the same statement / transaction.
--
-- Unified-instance model: sqlink-host now routes the read-side
-- vtab dispatch (`try_bridge_vtab_{connect, best_index, filter,
-- next, ...}`) through the sibling `mutating_bridges` map when an
-- entry exists (`host/src/lib.rs:7067-7370`). That collapses reads
-- and writes onto one wasm store (single linear memory) so a
-- bridge that keeps state in-guest (as ~/git/synthetic-mutating-
-- vtab-bridge does — a global `Mutex<KvState>`) observes prior
-- writes on the very next read. Before that routing change, the
-- read instance was a separate `Tabular` component with its own
-- linear memory and could not see the writes; this file's `SELECT
-- count(*)` / `SELECT value WHERE ...` assertions would have
-- returned zero rows.
--
-- Note: SAVEPOINT / RELEASE / ROLLBACK TO coverage is
-- intentionally OMITTED here despite the mutating world exporting
-- xSavepoint / xRelease / xRollbackTo. sqlink-extension's
-- `MODULE_MUTABLE` / `MODULE_MUTABLE_EPONYMOUS` module templates
-- declare `iVersion = 1` (sqlink-extension/src/vtab.rs:969,995).
-- SQLite dispatches xSavepoint / xRelease / xRollbackTo only when
-- `iVersion >= 2`, so those callbacks never reach the guest today
-- and their smoke coverage would test SQLite's implicit-txn
-- fallback rather than the intended dispatch tier. Follow-up:
-- bump iVersion to 2 and add read-after-write savepoint
-- assertions here.
--
-- Each `SELECT` returns 1 on success. See `.expected`.

-- 1. Empty vtab on connect.
SELECT CASE WHEN (SELECT count(*) FROM kv_store) = 0 THEN 1 ELSE 0 END;

-- 2. INSERT — xUpdate returns rowid=1 and the write is IMMEDIATELY
--    visible to a subsequent read through the same instance.
INSERT INTO kv_store(key, value) VALUES('alpha', 'first');
SELECT CASE WHEN last_insert_rowid() = 1 THEN 1 ELSE 0 END;
SELECT CASE WHEN (SELECT count(*) FROM kv_store) = 1 THEN 1 ELSE 0 END;
SELECT CASE WHEN (SELECT value FROM kv_store WHERE key = 'alpha') = 'first' THEN 1 ELSE 0 END;

-- 3. Second INSERT — read-side sees both rows.
INSERT INTO kv_store(key, value) VALUES('beta', 'second');
SELECT CASE WHEN last_insert_rowid() = 2 THEN 1 ELSE 0 END;
SELECT CASE WHEN (SELECT count(*) FROM kv_store) = 2 THEN 1 ELSE 0 END;
SELECT CASE WHEN (SELECT value FROM kv_store WHERE key = 'beta') = 'second' THEN 1 ELSE 0 END;

-- 4. UPDATE — new value observable on next read.
UPDATE kv_store SET value = 'FIRST' WHERE key = 'alpha';
SELECT CASE WHEN (SELECT value FROM kv_store WHERE key = 'alpha') = 'FIRST' THEN 1 ELSE 0 END;

-- 5. DELETE — row gone on next read.
DELETE FROM kv_store WHERE key = 'beta';
SELECT CASE WHEN (SELECT count(*) FROM kv_store) = 1 THEN 1 ELSE 0 END;
SELECT CASE WHEN NOT EXISTS (SELECT 1 FROM kv_store WHERE key = 'beta') THEN 1 ELSE 0 END;

-- 6. Transactional roundtrip: BEGIN + INSERT + read-in-txn + COMMIT.
--    In-txn reads see the shadow view (contains the uncommitted
--    row); the row survives COMMIT and is visible after.
BEGIN;
INSERT INTO kv_store(key, value) VALUES('gamma', 'third');
SELECT CASE WHEN (SELECT count(*) FROM kv_store) = 2 THEN 1 ELSE 0 END;
SELECT CASE WHEN (SELECT value FROM kv_store WHERE key = 'gamma') = 'third' THEN 1 ELSE 0 END;
COMMIT;
SELECT CASE WHEN (SELECT count(*) FROM kv_store) = 2 THEN 1 ELSE 0 END;
SELECT CASE WHEN (SELECT value FROM kv_store WHERE key = 'gamma') = 'third' THEN 1 ELSE 0 END;

-- 7. Rollback path — in-txn read sees the uncommitted insert;
--    post-ROLLBACK read must NOT see it.
BEGIN;
INSERT INTO kv_store(key, value) VALUES('delta', 'fourth');
SELECT CASE WHEN (SELECT count(*) FROM kv_store) = 3 THEN 1 ELSE 0 END;
ROLLBACK;
SELECT CASE WHEN (SELECT count(*) FROM kv_store) = 2 THEN 1 ELSE 0 END;
SELECT CASE WHEN NOT EXISTS (SELECT 1 FROM kv_store WHERE key = 'delta') THEN 1 ELSE 0 END;
