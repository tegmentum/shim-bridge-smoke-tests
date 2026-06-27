-- Span constructors: exercise Phase 4f's (i32,i32,bool,bool)
-- -> blob and (i64,i64,bool,bool) -> blob shape helpers.

-- datespan_make: i32 lower + i32 upper + 2× bool inclusivity
SELECT CASE WHEN datespan_lower(datespan_make(0, 100, true, false)) = 0
              AND datespan_upper(datespan_make(0, 100, true, false)) = 100
              AND datespan_width(datespan_make(0, 100, true, false)) = 100
            THEN 1 ELSE 0 END;

-- intspan_make: i64 lower + i64 upper + 2× bool inclusivity
-- Verifies (i64, i64, bool, bool) -> blob path
SELECT CASE WHEN intspan_lower(intspan_make(5, 10, true, false)) = 5
              AND intspan_upper(intspan_make(5, 10, true, false)) = 10
              AND intspan_width(intspan_make(5, 10, true, false)) = 5
            THEN 1 ELSE 0 END;
