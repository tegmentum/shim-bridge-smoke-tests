-- 03-type-roundtrip — primitive type-marshaling smoke for the
-- mobilitydb wasm bridge.
--
-- Spec'd in W5 as a `tfloat_min_value` wit-value-shaped scalar
-- smoke. The wit-value scalar surface (`arg_witvalue_*` helpers)
-- requires `SqlValue::WitValue` on the dispatch boundary, but
-- SQLite cells only carry NULL/INT/REAL/TEXT/BLOB — the loader's
-- `read_value` (sqlink-loader/src/value.rs) has no Blob → WitValue
-- recovery path. A wit-value returned by one scalar lands as a
-- BLOB in SQLite, and the next scalar's `arg_witvalue_*` rejects it
-- with "must be WIT-VALUE". Chaining requires either a Blob-recovery
-- lifter on the loader side, or a SQL-callable wit-value constructor
-- — neither shipped yet.
--
-- The substrate-honest alternative: exercise the primitive type-
-- marshaling boundary end-to-end through three families
-- (f64, s32/i32, text) so a codec drift surfaces here.

-- f64 list → f64 nth, two complementary indices.
SELECT CASE WHEN floatset_nth('[1.5, 2.5, 3.5]', 0) = 1.5 THEN 1 ELSE 0 END;
SELECT CASE WHEN floatset_nth('[1.5, 2.5, 3.5]', 2) = 3.5 THEN 1 ELSE 0 END;

-- s32 list → s32 membership.
SELECT CASE WHEN intset_contains('[10, 20, 30]', 20) = 1 THEN 1 ELSE 0 END;
SELECT CASE WHEN intset_contains('[10, 20, 30]', 99) = 0 THEN 1 ELSE 0 END;

-- text list → text membership.
SELECT CASE WHEN textset_contains('["alpha", "beta", "gamma"]', 'beta') = 1
              AND textset_contains('["alpha", "beta", "gamma"]', 'delta') = 0
            THEN 1 ELSE 0 END;
