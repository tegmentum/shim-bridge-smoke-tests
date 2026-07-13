-- Provider-envelope variant of cases/synthetic-mutating/01-crud.sql.
--
-- Exercises the same mutating-vtab dispatch surface, but routed
-- through sqlink-host's PROVIDER-side path
-- (`try_provider_invoke("vtab-update.*", ...)`) instead of the
-- bridge-side path (`try_bridge_vtab_update`, ...). The provider
-- component `synthetic_mutating_provider.wasm` exports
-- `compose:dynlink/endpoint` and handles every read + write arm
-- inside a single resident wasm-component provider instance.
--
-- Because the provider runs as ONE resident wasm-component store,
-- read and write sides share the same linear memory — unlike the
-- bridge case which stands up TWO isolated instances (Tabular +
-- TabularMutating). That means this case CAN legitimately assert
-- read-side visibility on top of prior writes, whereas the bridge
-- case (see cases/synthetic-mutating/01-crud.sql note on lines
-- 5-14) has to restrict itself to write-side dispatch only.
--
-- Kept structurally identical to the bridge case so both smoke
-- suites produce the same 7 `1`s; the additional shared-store
-- read-after-write coverage will move into a follow-up case file
-- (02-visibility.sql) once the base 7-arm smoke green-lights.
--
-- Each `SELECT` returns 1 on success. See `.expected`.

-- 1. Read-side dispatch works: empty vtab.
SELECT CASE WHEN (SELECT count(*) FROM kv_store) = 0 THEN 1 ELSE 0 END;

-- 2. INSERT — xUpdate on the provider returns rowid=1.
INSERT INTO kv_store(key, value) VALUES('alpha', 'first');
SELECT CASE WHEN last_insert_rowid() = 1 THEN 1 ELSE 0 END;

-- 3. Second INSERT — rowid=2. Proves next-rowid monotonicity in
--    the provider's committed BTreeMap.
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
--    xBegin → xUpdate → xSync → xCommit through the provider's
--    resident store.
BEGIN;
INSERT INTO kv_store(key, value) VALUES('gamma', 'third');
COMMIT;
SELECT 1;

-- 8. Rollback path — xBegin → xUpdate → xRollback.
BEGIN;
INSERT INTO kv_store(key, value) VALUES('delta', 'fourth');
ROLLBACK;
SELECT 1;

-- 9. Savepoint path — xSavepoint + xRollbackTo + xRelease.
SAVEPOINT sp1;
INSERT INTO kv_store(key, value) VALUES('epsilon', 'fifth');
ROLLBACK TO sp1;
RELEASE sp1;
SELECT 1;
