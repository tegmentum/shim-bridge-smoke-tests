-- Table functions (UDTFs). Each query returns integers so the
-- output is portable. as_of_join_* takes two temporal sequences
-- (built with tfloat_from_ewkt / tint_from_ewkt) and emits one
-- joined row per matching instant with a 4-column schema
-- (left_timestamp, right_timestamp, left_value, right_value).

-- Two coincident 2-instant float sequences → 2 joined rows.
SELECT COUNT(*) FROM as_of_join_float(
    tfloat_from_ewkt('[1.0@2000-01-01 00:00:00, 5.0@2000-01-01 00:00:10]'),
    tfloat_from_ewkt('[2.0@2000-01-01 00:00:00, 6.0@2000-01-01 00:00:10]'));

-- First joined row carries the expected typed column values
-- (double left/right value columns).
SELECT CASE WHEN left_value = 1.0 AND right_value = 2.0 THEN 1 ELSE 0 END
FROM as_of_join_float(
    tfloat_from_ewkt('[1.0@2000-01-01 00:00:00, 5.0@2000-01-01 00:00:10]'),
    tfloat_from_ewkt('[2.0@2000-01-01 00:00:00, 6.0@2000-01-01 00:00:10]'))
LIMIT 1;

-- Integer variant of the join.
SELECT COUNT(*) FROM as_of_join_int(
    tint_from_ewkt('[1@2000-01-01 00:00:00, 5@2000-01-01 00:00:10]'),
    tint_from_ewkt('[2@2000-01-01 00:00:00, 6@2000-01-01 00:00:10]'));
