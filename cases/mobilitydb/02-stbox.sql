-- STBox constructor + accessors. Exercises the Phase 4e
-- (f64×4, i64×2) -> blob shape that landed today.

SELECT CASE WHEN stbox_xmin(stbox_make(0.0, 0.0, 10.0, 10.0, 1700000000000000, 1700000060000000)) = 0.0
              AND stbox_ymin(stbox_make(0.0, 0.0, 10.0, 10.0, 1700000000000000, 1700000060000000)) = 0.0
              AND stbox_xmax(stbox_make(0.0, 0.0, 10.0, 10.0, 1700000000000000, 1700000060000000)) = 10.0
              AND stbox_ymax(stbox_make(0.0, 0.0, 10.0, 10.0, 1700000000000000, 1700000060000000)) = 10.0
            THEN 1 ELSE 0 END;

-- Time bounds round-trip.
SELECT CASE WHEN stbox_tmin(stbox_make(0.0, 0.0, 10.0, 10.0, 1700000000000000, 1700000060000000)) = 1700000000000000
              AND stbox_tmax(stbox_make(0.0, 0.0, 10.0, 10.0, 1700000000000000, 1700000060000000)) = 1700000060000000
            THEN 1 ELSE 0 END;

-- Derived metrics: width / height / area / duration.
SELECT CASE WHEN stbox_width(stbox_make(0.0, 0.0, 10.0, 10.0, 1700000000000000, 1700000060000000)) = 10.0
              AND stbox_height(stbox_make(0.0, 0.0, 10.0, 10.0, 1700000000000000, 1700000060000000)) = 10.0
              AND stbox_area(stbox_make(0.0, 0.0, 10.0, 10.0, 1700000000000000, 1700000060000000)) = 100.0
            THEN 1 ELSE 0 END;
