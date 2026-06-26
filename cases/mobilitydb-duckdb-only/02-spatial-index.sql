-- Spatial-index build + query through the real shim STRtree
-- (mobilitydb-strtree). Same shape as the postgis case: a build
-- AGGREGATE (mobilitydb_spatial_index_build) returns a session
-- handle, and a query TABLE FUNCTION
-- (mobilitydb_spatial_index_query_envelope) keyed by that handle
-- runs the index-accelerated lookup.
--
-- MobilityDB's shim ships no geometry constructors, so the two
-- indexed points are supplied as raw little-endian WKB POINTs:
--   0101000000000000000000e03f000000000000e03f = POINT(0.5 0.5)
--   010100000000000000000014400000000000001440 = POINT(5 5)
-- The unit-square envelope (0,0)-(1,1) must return ONLY item_id
-- 10, proving the real STRtree filters out item_id 20 at (5,5).
CREATE TABLE pts AS SELECT * FROM (VALUES
  (10, from_hex('0101000000000000000000e03f000000000000e03f')),
  (20, from_hex('010100000000000000000014400000000000001440'))
) t(id, wkb);
SET VARIABLE h = (SELECT mobilitydb_spatial_index_build(id, wkb) FROM pts);
SELECT q.item_id FROM mobilitydb_spatial_index_query_envelope(getvariable('h'), 0.0, 0.0, 1.0, 1.0) q;
