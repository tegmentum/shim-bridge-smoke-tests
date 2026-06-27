-- Spatial-index access methods (USING rtree / gist / ...) are
-- NOT registerable through DuckDB's loadable-extension C API:
-- custom index types live in an internal C++ registry a C-ABI
-- extension cannot extend (see the bridge's spatial_indexes.rs).
-- The documented, truthful outcome is DuckDB's own
-- "Unknown index type" binder error. This case pins that
-- behaviour so a future DuckDB C index-type API (which would let
-- us wire these up) is noticed as a change.
--
-- The DuckDB CLI aborts the batch on the first error, so this is
-- the sole line of output.
CREATE TABLE g (id INTEGER, geom GEOMETRY);
CREATE INDEX idx ON g USING rtree (geom);
