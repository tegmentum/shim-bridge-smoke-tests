-- Exercise every arm of the mutating-vtab dispatch surface:
--   * xUpdate INSERT / UPDATE / DELETE via SQL DML
--   * xBegin / xCommit  via BEGIN + COMMIT
--   * xRollback         via BEGIN + ROLLBACK
--
-- Note on visibility: sqlink-host stands up TWO wasm instances of
-- a mutating dynlink bridge — a `Tabular` for reads (dynlink_bridges)
-- and a `TabularMutating` for writes (mutating_bridges). Each has
-- its own linear memory, so a synthetic bridge that holds state
-- in-guest sees them as isolated. Production bridges avoid this by
-- routing both sides through a shared compose:dynlink provider —
-- outside the scope of this smoke coverage. Consequently this case
-- verifies WRITE-SIDE dispatch only (return values + txn success),
-- not the read/write handoff.
--
-- Each `SELECT` returns 1 on success. See `.expected`.

-- 1. Read-side dispatch through the Tabular instance works: empty vtab.
SELECT CASE WHEN (SELECT count(*) FROM kv_store) = 0 THEN 1 ELSE 0 END;

-- 2. INSERT — xUpdate on the mutating instance returns rowid=1.
--    last_insert_rowid() carries that value back into SQL from the
--    connection state, proving the dispatch round-trip landed.
INSERT INTO kv_store(key, value) VALUES('alpha', 'first');
SELECT CASE WHEN last_insert_rowid() = 1 THEN 1 ELSE 0 END;

-- 3. Second INSERT — rowid=2. Proves next-rowid monotonicity in the
--    guest's committed HashMap (in-guest state persists across
--    successive xUpdate calls on the same instance).
INSERT INTO kv_store(key, value) VALUES('beta', 'second');
SELECT CASE WHEN last_insert_rowid() = 2 THEN 1 ELSE 0 END;

-- 4. changes() = 1 — proves SQLite's per-statement row-count sees
--    xUpdate's Ok result.
SELECT CASE WHEN changes() = 1 THEN 1 ELSE 0 END;

-- 5. UPDATE — no rowid change; SQL should not error.
UPDATE kv_store SET value = 'FIRST' WHERE rowid = 1;

-- 6. DELETE — no error.
DELETE FROM kv_store WHERE rowid = 2;

-- 7. Transactional roundtrip: BEGIN + INSERT + COMMIT. Fires
--    xBegin → xUpdate → xSync → xCommit. If any errored the
--    connection would drop back to autocommit and the following
--    SELECT would still run — SQLite's autocommit resumes — but the
--    error would have been reported inline. All 4 SELECT below
--    completing to 1 means no error surfaced.
BEGIN;
INSERT INTO kv_store(key, value) VALUES('gamma', 'third');
COMMIT;
SELECT 1;

-- 8. Rollback path — xBegin → xUpdate → xRollback.
BEGIN;
INSERT INTO kv_store(key, value) VALUES('delta', 'fourth');
ROLLBACK;
SELECT 1;

-- 9. Savepoint path — xSavepoint + xRollbackTo.
SAVEPOINT sp1;
INSERT INTO kv_store(key, value) VALUES('epsilon', 'fifth');
ROLLBACK TO sp1;
RELEASE sp1;
SELECT 1;
