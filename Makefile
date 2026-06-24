# Smoke runner orchestration.
#
# Default `make smoke` runs every shim against every target it
# supports, fail-fast on the first mismatch. Pass overrides
# via env vars if the build artifacts live somewhere unusual.
#
# Required artifacts (override with env vars):
#
#   POSTGIS_DUCKDB_BRIDGE  — postgis_duckdb_bridge.duckdb_extension
#   POSTGIS_SQLITE_BRIDGE  — libpostgis_sqlite_bridge.dylib
#   MOBILITYDB_DUCKDB_BRIDGE
#   MOBILITYDB_SQLITE_BRIDGE
#   POSTGIS_SHIM            — postgis composed shim wasm
#   MOBILITYDB_SHIM         — mobilitydb composed shim wasm

POSTGIS_DUCKDB_BRIDGE    ?= /tmp/postgis_duckdb_bridge.duckdb_extension
POSTGIS_SQLITE_BRIDGE    ?= $(HOME)/git/postgis-sqlite-bridge/target/release/libpostgis_sqlite_bridge.dylib
MOBILITYDB_DUCKDB_BRIDGE ?= /tmp/mobilitydb_duckdb_bridge.duckdb_extension
MOBILITYDB_SQLITE_BRIDGE ?= $(HOME)/git/mobilitydb-sqlite-bridge/target/release/libmobilitydb_sqlite_bridge.dylib
POSTGIS_SHIM             ?= /tmp/postgis-shim-composed.wasm
MOBILITYDB_SHIM          ?= /tmp/mobilitydb-composed.wasm

.PHONY: smoke postgis mobilitydb postgis-duckdb postgis-sqlite mobilitydb-duckdb mobilitydb-sqlite

smoke: postgis mobilitydb
	@echo ""
	@echo "===== ALL SMOKE TESTS PASSED ====="

postgis: postgis-duckdb postgis-sqlite

mobilitydb: mobilitydb-duckdb mobilitydb-sqlite

postgis-duckdb:
	@echo "=== postgis × duckdb ==="
	@bash scripts/run.sh duckdb $(POSTGIS_DUCKDB_BRIDGE) $(POSTGIS_SHIM) cases/postgis

postgis-sqlite:
	@echo "=== postgis × sqlite ==="
	@bash scripts/run.sh sqlite $(POSTGIS_SQLITE_BRIDGE) $(POSTGIS_SHIM) cases/postgis
	@if [ -d cases/postgis-sqlite-only ]; then \
	    echo "=== postgis × sqlite (sqlite-only cases) ==="; \
	    bash scripts/run.sh sqlite $(POSTGIS_SQLITE_BRIDGE) $(POSTGIS_SHIM) cases/postgis-sqlite-only; \
	fi

mobilitydb-duckdb:
	@echo "=== mobilitydb × duckdb ==="
	@bash scripts/run.sh duckdb $(MOBILITYDB_DUCKDB_BRIDGE) $(MOBILITYDB_SHIM) cases/mobilitydb

mobilitydb-sqlite:
	@echo "=== mobilitydb × sqlite ==="
	@bash scripts/run.sh sqlite $(MOBILITYDB_SQLITE_BRIDGE) $(MOBILITYDB_SHIM) cases/mobilitydb
