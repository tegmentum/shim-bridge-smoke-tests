-- spans-ops expansion + span-sets: buckets 3-4 of the 1273-UDF arc.
-- intspan ops were already wired; just need regression coverage.

-- intspan binary ops
SELECT CASE WHEN intspan_lower(intspan_intersection(intspan_make(0,10,true,false), intspan_make(5,15,true,false))) = 5
              AND intspan_upper(intspan_intersection(intspan_make(0,10,true,false), intspan_make(5,15,true,false))) = 10
              AND intspan_contains_span(intspan_make(0,100,true,false), intspan_make(5,10,true,false)) = true
            THEN 1 ELSE 0 END;

-- intspan transforms
SELECT CASE WHEN intspan_lower(intspan_shift(intspan_make(5, 10, true, false), 100)) = 105
              AND intspan_lower(intspan_expand(intspan_make(5, 10, true, false), 2)) = 3
            THEN 1 ELSE 0 END;

-- intspanset from text (requires prefix)
SELECT CASE WHEN intspanset_num_spans(intspanset_from_text('INTSPANSET{[0, 5), [10, 15)}')) = 2
            THEN 1 ELSE 0 END;

-- datespanset constructor from single span
SELECT CASE WHEN datespanset_num_spans(datespanset_from_span(datespan_make(0, 100, true, false))) = 1
            THEN 1 ELSE 0 END;
