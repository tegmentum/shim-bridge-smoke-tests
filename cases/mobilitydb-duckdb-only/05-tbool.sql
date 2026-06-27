-- tbool-ops: second bucket of the 1273-UDF expansion.
-- Exercises CSV construct, accessors, predicates, and logical
-- ops end-to-end through the bridge. tbool dispatch is fully
-- wired in the shim today; this case pins regressions.

-- Constructor + accessors over a 3-instant sequence
WITH seq AS (
  SELECT tbool_from_csv('timestamp,value
0,true
1000000,false
2000000,true') AS s
)
SELECT CASE WHEN tbool_num_instants(s) = 3
              AND tbool_start_value(s) = true
              AND tbool_end_value(s) = true
              AND tbool_value_at(s, 0) = true
              AND tbool_value_at(s, 1000000) = false
              AND tbool_value_at(s, 2000000) = true
            THEN 1 ELSE 0 END FROM seq;

-- Counting predicates
WITH seq AS (
  SELECT tbool_from_csv('timestamp,value
0,true
1000000,false
2000000,true') AS s
)
SELECT CASE WHEN tbool_count_true(s) = 2
              AND tbool_count_false(s) = 1
              AND tbool_ever_true(s) = true
              AND tbool_always_true(s) = false
              AND tbool_ever_false(s) = true
              AND tbool_always_false(s) = false
            THEN 1 ELSE 0 END FROM seq;

-- Logical negation: tbool_not flips every instant's value
WITH seq AS (
  SELECT tbool_from_csv('timestamp,value
0,true
1000000,false') AS s
)
SELECT CASE WHEN tbool_value_at(tbool_not(s), 0) = false
              AND tbool_value_at(tbool_not(s), 1000000) = true
              AND tbool_count_true(tbool_not(s)) = 1
              AND tbool_count_false(tbool_not(s)) = 1
            THEN 1 ELSE 0 END FROM seq;
