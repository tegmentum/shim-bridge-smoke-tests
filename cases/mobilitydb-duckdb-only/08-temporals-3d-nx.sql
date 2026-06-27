-- 3D temporals + tnpoint + tfloat analytics: buckets 5, 8, 10.
-- tgeompoint3d / bitemporal_* dispatch is reachable after the
-- _from_text/_to_text Utf8 fallback in metas() (datafission).
-- tnpoint uses _ewkt naming. tfloat analytics needed the lenient
-- parse_ts (mobilitydb-core/src/ewkt.rs).

-- tgeompoint3d round-trip
SELECT CASE WHEN length(tgeompoint3d_to_text(
                  tgeompoint3d_from_text('[POINT(1 2 3)@2024-01-01]'))) > 20
            THEN 1 ELSE 0 END;

-- tnpoint round-trip via EWKT
SELECT CASE WHEN length(tnpoint_to_ewkt(
                  tnpoint_from_ewkt('[NPOINT(1, 0.5)@2024-01-01]'))) > 20
            THEN 1 ELSE 0 END;

-- tfloat analytics over a 2-instant linear sequence
WITH s AS (SELECT tfloat_from_csv('timestamp,value
0,1.0
1000000,3.0') AS x)
SELECT CASE WHEN tfloat_twavg(x) = 2.0
              AND tfloat_min_value(x) = 1.0
              AND tfloat_max_value(x) = 3.0
              AND tfloat_integral(x) = 2.0
            THEN 1 ELSE 0 END FROM s;

-- tfloat aggregate-ish accessors
WITH s AS (SELECT tfloat_from_csv('timestamp,value
0,1.0
1000000,2.0
2000000,3.0') AS x)
SELECT CASE WHEN tfloat_num_instants(x) = 3
              AND tfloat_start_value(x) = 1.0
              AND tfloat_end_value(x) = 3.0
            THEN 1 ELSE 0 END FROM s;
