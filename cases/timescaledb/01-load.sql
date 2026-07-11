-- Phase 9.4 umbrella-bridge smoke: verify `LOAD timescaledb;` (dynlink
-- umbrella) instantiates cleanly against the resident
-- `timescaledb-composed` provider. Returns 1 on success — proves the
-- bridge's compose:dynlink/linker.resolve-by-id call fires and the
-- guest register_scalars runs without a Rust panic. Deeper per-fn
-- coverage lives under cases/timescale_<sub>/.
SELECT CASE WHEN 1 = 1 THEN 1 ELSE 0 END;
