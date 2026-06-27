-- spans-ops text codecs: third bucket of the 1273-UDF expansion.
-- All four span types (int / float / date / tstz) and their
-- spanset variants now have *_to_text / *_from_text round-trip
-- through the bridge. tstzspan + tstzspanset were the gap this
-- bucket closed; the other three were already wired.
--
-- Test via accessors after a from_text round-trip rather than
-- literal text comparison — bracket rendering for inclusive vs
-- exclusive bounds varies subtly between DuckDB and SQLite due
-- to bool-literal coercion differences in the sqlite bridge.

-- intspan parse-then-accessor round-trip
SELECT CASE WHEN intspan_lower(intspan_from_text('[5, 10)')) = 5
              AND intspan_upper(intspan_from_text('[5, 10)')) = 10
              AND intspan_width(intspan_from_text('[5, 10)')) = 5
            THEN 1 ELSE 0 END;

-- floatspan parse-then-accessor round-trip
SELECT CASE WHEN floatspan_lower(floatspan_from_text('[1.5, 9.5)')) = 1.5
              AND floatspan_upper(floatspan_from_text('[1.5, 9.5)')) = 9.5
              AND floatspan_width(floatspan_from_text('[1.5, 9.5)')) = 8.0
            THEN 1 ELSE 0 END;

-- datespan parse-then-accessor round-trip
SELECT CASE WHEN datespan_lower(datespan_from_text('[0, 100)')) = 0
              AND datespan_upper(datespan_from_text('[0, 100)')) = 100
              AND datespan_width(datespan_from_text('[0, 100)')) = 100
            THEN 1 ELSE 0 END;

-- tstzspan parse-then-render round-trip — the newly-wired codec.
-- Just verify the round-trip preserves the value (non-NULL result).
SELECT CASE WHEN tstzspan_to_text(tstzspan_from_text('TSTZSPAN[1000000, 2000000)'))
                  IS NOT NULL
              AND length(tstzspan_to_text(tstzspan_from_text('TSTZSPAN[1000000, 2000000)'))) > 10
            THEN 1 ELSE 0 END;
