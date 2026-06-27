-- Spatial-index build + query through the real shim STRtree.
--
-- The bridge exposes the shim's tested spatial-index path as a
-- build AGGREGATE (postgis_spatial_index_build) that returns a
-- session handle, plus a query TABLE FUNCTION
-- (postgis_spatial_index_query_envelope) keyed by that handle.
-- DuckDB rejects a scalar subquery as a table-function argument
-- ("Table function cannot contain subqueries"), so we capture the
-- handle in a SET VARIABLE first (documented in spatial_indexes.rs).
--
-- Two points are indexed; the unit-square envelope (0,0)-(1,1)
-- must return ONLY item_id 10 (the (0.5,0.5) point), proving the
-- real R-tree filters out item_id 20 at (5,5).
CREATE TABLE pts AS SELECT * FROM (VALUES
  (10, ST_AsBinary(ST_GeomFromText('POINT(0.5 0.5)'))),
  (20, ST_AsBinary(ST_GeomFromText('POINT(5 5)')))
) t(id, wkb);
SET VARIABLE h = (SELECT postgis_spatial_index_build(id, wkb) FROM pts);
SELECT q.item_id FROM postgis_spatial_index_query_envelope(getvariable('h'), 0.0, 0.0, 1.0, 1.0) q;
