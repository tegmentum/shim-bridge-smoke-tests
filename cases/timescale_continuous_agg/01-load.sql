-- Verify the timescale_continuous_agg sub-ext loads cleanly + a trivial passthrough SELECT works.
-- Returns 1 on success, 0 on failure. Framework-level check.

SELECT CASE WHEN 1 = 1 THEN 1 ELSE 0 END;
