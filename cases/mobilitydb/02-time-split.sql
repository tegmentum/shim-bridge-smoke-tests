-- 02-time-split — time-set operations smoke for the mobilitydb wasm
-- bridge.
--
-- Spec'd in W5 as a `tfloat_time_split` UDTF smoke; the time-split
-- family is W4a-deferred upstream (26 missing UDTF WIT declarations,
-- #557), and reaching the UDTF path also requires sqlink-loader's
-- vtab wiring (deferred — load.rs:218). The substrate-honest stand-
-- in is the W2 Phase 1 JSON-list-of-primitive surface:
--
--   - dateset_len(json_dates)            -> i64
--   - dateset_contains(json_dates, d)    -> bool
--   - dateset_nth(json_dates, n)         -> i32  (the "split" semantics:
--                                                 indexed retrieval over
--                                                 the sorted set)
--
-- Each query exercises the JSON-as-TEXT marshaling of a list of
-- primitives, which is one of the two record-marshaling shapes
-- W2 wired through the codegen.

-- Cardinality round-trip: 5 distinct dates parse + count clean.
SELECT CASE WHEN dateset_len('[100, 200, 300, 400, 500]') = 5 THEN 1 ELSE 0 END;

-- Membership: 200 IS in the set.
SELECT CASE WHEN dateset_contains('[100, 200, 300]', 200) = 1 THEN 1 ELSE 0 END;

-- Membership: 999 is NOT in the set.
SELECT CASE WHEN dateset_contains('[100, 200, 300]', 999) = 0 THEN 1 ELSE 0 END;

-- "Split" by index: nth(1) of a sorted set is the second element.
SELECT CASE WHEN dateset_nth('[100, 200, 300]', 1) = 200 THEN 1 ELSE 0 END;

-- Cross-type: intset_len behaves the same way for s32 dates.
SELECT CASE WHEN intset_len('[1, 2, 3, 4, 5, 6]') = 6 THEN 1 ELSE 0 END;
