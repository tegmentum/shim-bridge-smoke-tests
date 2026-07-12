CREATE TABLE pts AS SELECT * FROM (VALUES (10, from_hex('0101000000000000000000e03f000000000000e03f')), (20, from_hex('010100000000000000000014400000000000001440'))) t(id, wkb);
SET VARIABLE h = (SELECT mobilitydb_spatial_index_build(id, wkb) FROM pts);
SELECT q.item_id FROM mobilitydb_spatial_index_query_envelope(getvariable('h'), 0.0, 0.0, 1.0, 1.0) q;
