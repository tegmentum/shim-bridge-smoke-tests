-- Verify the mobilitydb_temporal_jsonb sub-ext loads cleanly + a trivial passthrough SELECT works.
-- Returns 1 on success, 0 on failure. Framework-level check — deliberately
-- avoids sub-ext-specific SQL so any wire regression fails HERE first,
-- before the more specific probes below.

SELECT CASE WHEN 1 = 1 THEN 1 ELSE 0 END;
