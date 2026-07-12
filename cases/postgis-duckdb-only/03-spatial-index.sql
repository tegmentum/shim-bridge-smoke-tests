CREATE TABLE pts AS SELECT * FROM (VALUES (10, ST_AsBinary(ST_GeomFromText('POINT(0.5 0.5)'))), (20, ST_AsBinary(ST_GeomFromText('POINT(5 5)')))) t(id, wkb);
SET VARIABLE h = (SELECT postgis_spatial_index_build(id, wkb) FROM pts);
SELECT q.item_id FROM postgis_spatial_index_query_envelope(getvariable('h'), 0.0, 0.0, 1.0, 1.0) q;
