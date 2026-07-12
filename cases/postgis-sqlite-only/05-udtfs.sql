-- UDTFs are sqlink-only today (ducklink Phase 4c is scaffolded
-- but not yet implementing). Single-target cases live under
-- their own per-target directory.

SELECT count(*) FROM st_dumppoints(ST_GeomFromText('LINESTRING(0 0, 1 1, 2 2, 3 3)'));
SELECT count(*) FROM st_dump(ST_GeomFromText('MULTIPOINT(0 0, 1 1, 2 2)'));
SELECT count(*) FROM st_dumpsegments(ST_GeomFromText('LINESTRING(0 0, 1 1, 2 2, 3 3)'));
SELECT count(*) FROM st_squaregrid(1.0, ST_GeomFromText('POLYGON((0 0, 3 0, 3 3, 0 3, 0 0))'));
SELECT count(*) FROM st_dumppoints(ST_GeomFromText('LINESTRING(0 0, 1 1, 2 2, 3 3, 4 4)')) WHERE ST_X(point) > 1;
