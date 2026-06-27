-- 01-spatial-join — spatial-relation smoke for the mobilitydb wasm
-- bridge.
--
-- The substrate-honest stand-in for `temporal_join_float` (spec'd
-- in W5) lives on the primitive-scalar surface today: the temporal-
-- join UDTFs need sqlink-loader's vtab registration path, which the
-- loader's load.rs explicitly defers (see scripts/run.sh's loader-
-- chain comment + the cases/postgis-sqlite-only/05-udtfs failure).
-- We exercise the spatial-relation flavor instead via primitive-in/
-- primitive-out scalars that the dispatch table wires directly:
--
--   - distance(x1, y1, x2, y2) -> f64    (planar Euclidean)
--   - bearing(x1, y1, x2, y2) -> f64     (compass heading 0..360)
--   - angular_diff(a, b)      -> f64     (signed diff, normalised)
--
-- Every query returns 1 on success, 0 on failure — same portable
-- shape the postgis cases use.

-- Pythagorean: (0,0) to (3,4) has distance 5.
SELECT CASE WHEN distance(0.0, 0.0, 3.0, 4.0) = 5.0 THEN 1 ELSE 0 END;

-- Translation invariance: (1,1) to (4,5) is the same vector.
SELECT CASE WHEN distance(1.0, 1.0, 4.0, 5.0) = 5.0 THEN 1 ELSE 0 END;

-- Bearing 0° east of north convention: due east is 90°.
SELECT CASE WHEN bearing(0.0, 0.0, 1.0, 0.0) = 90.0 THEN 1 ELSE 0 END;

-- 45° NE bisects the first quadrant.
SELECT CASE WHEN bearing(0.0, 0.0, 1.0, 1.0) = 45.0 THEN 1 ELSE 0 END;

-- angular_diff normalises to (-180, 180]: 720° wraps to 0.
SELECT CASE WHEN angular_diff(0.0, 720.0) = 0.0 THEN 1 ELSE 0 END;
