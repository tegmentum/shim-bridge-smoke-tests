LOAD synthetic_mutating;
ATTACH ':memory:' AS kv (TYPE synthetic_mutating);
CREATE TABLE kv.kv_store(key TEXT, value TEXT);
SELECT CASE WHEN (SELECT count(value) FROM kv.kv_store) = 0 THEN 1 ELSE 0 END;
INSERT INTO kv.kv_store(key, value) VALUES('alpha', 'first');
SELECT CASE WHEN (SELECT count(value) FROM kv.kv_store) = 1 THEN 1 ELSE 0 END;
SELECT CASE WHEN (SELECT value FROM kv.kv_store WHERE key = 'alpha') = 'first' THEN 1 ELSE 0 END;
INSERT INTO kv.kv_store(key, value) VALUES('beta', 'second');
SELECT CASE WHEN (SELECT count(value) FROM kv.kv_store) = 2 THEN 1 ELSE 0 END;
SELECT CASE WHEN (SELECT value FROM kv.kv_store WHERE key = 'beta') = 'second' THEN 1 ELSE 0 END;
BEGIN;
INSERT INTO kv.kv_store(key, value) VALUES('gamma', 'third');
SELECT CASE WHEN (SELECT count(value) FROM kv.kv_store) = 3 THEN 1 ELSE 0 END;
SELECT CASE WHEN (SELECT value FROM kv.kv_store WHERE key = 'gamma') = 'third' THEN 1 ELSE 0 END;
COMMIT;
SELECT CASE WHEN (SELECT count(value) FROM kv.kv_store) = 3 THEN 1 ELSE 0 END;
SELECT CASE WHEN (SELECT value FROM kv.kv_store WHERE key = 'gamma') = 'third' THEN 1 ELSE 0 END;
BEGIN;
INSERT INTO kv.kv_store(key, value) VALUES('delta', 'fourth');
SELECT CASE WHEN (SELECT count(value) FROM kv.kv_store) = 4 THEN 1 ELSE 0 END;
ROLLBACK;
SELECT CASE WHEN (SELECT count(value) FROM kv.kv_store) = 3 THEN 1 ELSE 0 END;
SELECT CASE WHEN NOT EXISTS (SELECT value FROM kv.kv_store WHERE key = 'delta') THEN 1 ELSE 0 END;
-- count(*) with zero-column projection: DuckDB requests only cardinality
-- from the storage scan (projection = []). Previously tripped
-- `Expected vector of type VARCHAR, but found vector of type INT64` at
-- duckdb-wasm's empty-projection scan-fill loop; now honored (empty
-- projection -> zero column_types, fill bounded by
-- duckdb_data_chunk_get_column_count).
SELECT CASE WHEN (SELECT count(*) FROM kv.kv_store) = 3 THEN 1 ELSE 0 END;
-- UPDATE by predicate: the DuckDB physical plan projects
-- COLUMN_IDENTIFIER_ROW_ID alongside the SET columns; the scan supplies
-- rowids as the trailing s64 cell of each row (`wants-rowid` on
-- scan-request), and WasmPhysicalUpdate::Sink forwards those rowids +
-- the updated-columns list to storage-write-dispatch.update-rows.
UPDATE kv.kv_store SET value = 'FIRST' WHERE key = 'alpha';
SELECT CASE WHEN (SELECT value FROM kv.kv_store WHERE key = 'alpha') = 'FIRST' THEN 1 ELSE 0 END;
SELECT CASE WHEN (SELECT count(value) FROM kv.kv_store) = 3 THEN 1 ELSE 0 END;
-- DELETE by predicate: same rowid-projection path drives
-- WasmPhysicalDelete::Sink -> storage-write-dispatch.delete-rows.
DELETE FROM kv.kv_store WHERE key = 'beta';
SELECT CASE WHEN NOT EXISTS (SELECT value FROM kv.kv_store WHERE key = 'beta') THEN 1 ELSE 0 END;
SELECT CASE WHEN (SELECT count(value) FROM kv.kv_store) = 2 THEN 1 ELSE 0 END;
-- Transactional UPDATE + ROLLBACK: the shadow view captures the update
-- pre-image; rollback discards it and the visible view resets to the
-- committed baseline. Proves scan-open + update-rows share the shadow.
BEGIN;
UPDATE kv.kv_store SET value = 'THIRD' WHERE key = 'gamma';
SELECT CASE WHEN (SELECT value FROM kv.kv_store WHERE key = 'gamma') = 'THIRD' THEN 1 ELSE 0 END;
ROLLBACK;
SELECT CASE WHEN (SELECT value FROM kv.kv_store WHERE key = 'gamma') = 'third' THEN 1 ELSE 0 END;
