-- Table functions (UDTFs). Each query returns integers (counts
-- or 0/1 success flags) so the output is portable across the
-- DuckDB and SQLite targets.

-- ST_DumpPoints expands a LINESTRING into one row per vertex.
SELECT COUNT(*) FROM ST_DumpPoints(ST_GeomFromText('LINESTRING(0 0,1 1,2 2)'));

-- ...and the dumped points round-trip to the expected WKT.
SELECT CASE WHEN ST_AsText(point) = 'POINT(1 1)' THEN 1 ELSE 0 END
FROM ST_DumpPoints(ST_GeomFromText('LINESTRING(0 0,1 1,2 2)'))
LIMIT 1 OFFSET 1;

-- ST_Dump decomposes a MULTIPOINT into its component geometries
-- (multi-column output: geom + path).
SELECT COUNT(*) FROM ST_Dump(ST_GeomFromText('MULTIPOINT(0 0,1 1,2 2)'));

-- ST_Subdivide returns at least one piece for a simple polygon.
SELECT CASE WHEN COUNT(*) >= 1 THEN 1 ELSE 0 END
FROM ST_Subdivide(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'));
