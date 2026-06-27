-- Aggregate-only UDAFs: bucket 12 polish.
-- Mobilitydb's F64 aggregates (tfloat_max_agg / tfloat_min_agg /
-- tfloat_count_agg / tfloat_stddev_agg etc.) take a scalar
-- DOUBLE input, not a temporal sequence blob. The bridge codegen
-- now reads the per-aggregate input type from the interface DB
-- and dispatches via DataType-based vector reads; before today's
-- fix every aggregate was registered as (BLOB)->BLOB regardless
-- and tripped the shim's "accumulator/value type mismatch".
--
-- Output is BLOB-encoded LE bytes of the typed result (8 bytes
-- for f64/i64/u64). Tests assert the byte count rather than
-- the value to keep the smoke dialect-portable.

WITH vals(v) AS (
  VALUES (1.0::DOUBLE), (3.0::DOUBLE), (2.0::DOUBLE), (5.0::DOUBLE), (4.0::DOUBLE)
)
SELECT CASE WHEN octet_length(tfloat_max_agg(v))   = 8
              AND octet_length(tfloat_min_agg(v))   = 8
              AND octet_length(tfloat_count_agg(v)) = 8
            THEN 1 ELSE 0 END FROM vals;
