-- 04-wit-value-roundtrip — record-typed parameter marshaling via
-- JSON-as-TEXT.
--
-- Spec'd in W5 as a kdtree_xy_within UDTF smoke; the UDTF dispatch
-- path needs sqlink-loader vtab wiring (deferred — load.rs:218 lists
-- vtabs in the `skipped` bucket so `cases/postgis-sqlite-only/05-udtfs`
-- also fails today). The substrate-honest alternative exercises the
-- same `parse_json_list_record_*` codec on the SCALAR surface, which
-- IS dispatched: scalars like `date_spanset_contains` /
-- `float_spanset_contains` / `intspanset_contains` take a list of
-- record-typed spans via JSON-as-TEXT.
--
-- This proves W2 Phase 2's `parse_json_list_record_<X>` codec
-- (#553 — complex-element list<X> via wit-value codec) end-to-end:
-- SQL TEXT → serde_json → UPSTREAM record vec → mobilitydb upstream
-- spans-ops → primitive return. JSON keys MUST be snake-case
-- (`lower_inc`, `upper_inc`) because wit-bindgen kebab→snake-cases
-- the WIT-record field names and serde_json decodes against the
-- generated Rust field names.

-- date-span variant: [100, 200) contains 150.
SELECT CASE
  WHEN date_spanset_contains(
    '[{"lower":100,"upper":200,"lower_inc":true,"upper_inc":false}]',
    150
  ) = 1
  THEN 1 ELSE 0 END;

-- date-span variant: [100, 200) does NOT contain 250.
SELECT CASE
  WHEN date_spanset_contains(
    '[{"lower":100,"upper":200,"lower_inc":true,"upper_inc":false}]',
    250
  ) = 0
  THEN 1 ELSE 0 END;

-- Two disjoint spans, count = 2.
SELECT CASE
  WHEN date_spanset_num_spans(
    '[{"lower":100,"upper":200,"lower_inc":true,"upper_inc":false},
       {"lower":300,"upper":400,"lower_inc":true,"upper_inc":false}]'
  ) = 2
  THEN 1 ELSE 0 END;

-- float-span variant: 3.0 ∈ [1.0, 5.0).
SELECT CASE
  WHEN float_spanset_contains(
    '[{"lower":1.0,"upper":5.0,"lower_inc":true,"upper_inc":false}]',
    3.0
  ) = 1
  THEN 1 ELSE 0 END;

-- intspan variant: 3 ∈ [1, 5).
SELECT CASE
  WHEN intspanset_contains(
    '[{"lower":1,"upper":5,"lower_inc":true,"upper_inc":false}]',
    3
  ) = 1
  THEN 1 ELSE 0 END;
