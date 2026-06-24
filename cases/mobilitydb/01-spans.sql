-- Span types: bracket-shorthand parse + accessors + width
-- Every query returns 1 on success, 0 on failure. Outputs as a
-- column of integers for portable diffing.

SELECT CASE WHEN intspan_lower(intspan_from_text('[5,10)')) = 5
              AND intspan_upper(intspan_from_text('[5,10)')) = 10
              AND intspan_width(intspan_from_text('[5,10)')) = 5
            THEN 1 ELSE 0 END;

SELECT CASE WHEN floatspan_lower(floatspan_from_text('[5.0,10.0)')) = 5.0
              AND floatspan_upper(floatspan_from_text('[5.0,10.0)')) = 10.0
              AND floatspan_width(floatspan_from_text('[5.0,10.0)')) = 5.0
            THEN 1 ELSE 0 END;

-- Predicates: overlap + adjacency
SELECT CASE WHEN floatspan_overlaps(
                   floatspan_from_text('[0.0,5.0)'),
                   floatspan_from_text('[3.0,8.0)'))
            THEN 1 ELSE 0 END;

SELECT CASE WHEN NOT floatspan_overlaps(
                   floatspan_from_text('[0.0,5.0)'),
                   floatspan_from_text('[5.0,10.0)'))
            THEN 1 ELSE 0 END;

-- Round-trip through text: the canonical FLOATSPAN form is
-- emitted by *_to_text and re-parsed.
SELECT CASE WHEN floatspan_to_text(floatspan_from_text('[5.0,10.0)'))
                 = 'FLOATSPAN[5, 10)'
            THEN 1 ELSE 0 END;
