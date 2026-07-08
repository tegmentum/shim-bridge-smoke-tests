# Smoke runner orchestration.
#
# Default `make smoke` runs every shim against every target it
# supports, fail-fast on the first mismatch. Pass overrides
# via env vars if the build artifacts live somewhere unusual.
#
# Required artifacts (override with env vars):
#
#   POSTGIS_DUCKDB_BRIDGE     — legacy native duckdb_extension (archived path)
#   POSTGIS_SQLITE_BRIDGE     — sqlink wasm-component loadable (sqlite via sqlink-host)
#   POSTGIS_DUCKLINK_BRIDGE   — ducklink wasm-component loadable (duckdb via ducklink-host)
#   MOBILITYDB_DUCKDB_BRIDGE
#   MOBILITYDB_SQLITE_BRIDGE
#   MOBILITYDB_DUCKLINK_BRIDGE
#   POSTGIS_SHIM              — postgis composed shim wasm
#   MOBILITYDB_SHIM           — mobilitydb composed shim wasm

POSTGIS_DUCKDB_BRIDGE       ?= /tmp/postgis_duckdb_bridge.duckdb_extension
POSTGIS_SQLITE_BRIDGE       ?= $(HOME)/git/postgis-sqlink-bridge/postgis-sqlink-loadable.wasm
POSTGIS_DUCKLINK_BRIDGE     ?= $(HOME)/git/postgis-ducklink-bridge/postgis-ducklink-loadable.wasm
MOBILITYDB_DUCKDB_BRIDGE    ?= /tmp/mobilitydb_duckdb_bridge.duckdb_extension
# mobilitydb's wasm bridges need postgis loaded first for the
# GEOMETRY type (D5 load-order convention). The colon-separated
# path is decoded by scripts/run.sh into two `LOAD` statements
# in the listed order — same convention across sqlite (sqlink)
# and ducklink targets.
MOBILITYDB_SQLITE_BRIDGE    ?= $(HOME)/git/postgis-sqlink-bridge/postgis-sqlink-loadable.wasm:$(HOME)/git/mobilitydb-sqlink-bridge/mobilitydb-sqlink-loadable.wasm
MOBILITYDB_DUCKLINK_BRIDGE  ?= $(HOME)/git/postgis-ducklink-bridge/postgis-ducklink-loadable.wasm:$(HOME)/git/mobilitydb-ducklink-bridge/mobilitydb-ducklink-loadable.wasm
# Composed shim wasm — the datafission-vendored copies are the
# canonical ones (post-kebab-fix at the wasm extern-name level,
# matching the codegen's `kebab_fix_wit` rewrite of the bridge
# WIT). See SHIM-BRIDGES.md's "Why the datafission-vendored shim"
# section. The raw `~/git/postgis-wasm/postgis-composed.wasm`
# and `~/git/mobilitydb-wasm/mobilitydb-composed.wasm` will fail
# `wac plug` with a resource-identity mismatch.
#
# Only actively used by the sqlite (sqlink) and legacy duckdb
# targets (via `<EXT>_SHIM_WASM` env var). The ducklink target
# doesn't need the shim at runtime — it's already `wac plug`'d
# into the composed loadable — but the vars are set to canonical
# paths for consistency.
POSTGIS_SHIM                ?= $(HOME)/git/datafission/extensions/postgis/deps/postgis-composed.wasm
MOBILITYDB_SHIM             ?= $(HOME)/git/mobilitydb-wasm/mobilitydb-composed.wasm

# Optional preprocessor wiring. When SHIM_SQL_PREPROCESS is set,
# scripts/run.sh pipes each case file through it (with the
# corresponding interface DB) before sending to the target CLI.
# Skip per-case via a `<case>.no-preprocess` marker file.
SHIM_SQL_PREPROCESS      ?= $(HOME)/git/shim-sql-preprocess/target/release/shim-sql-preprocess
POSTGIS_INTERFACE_DB     ?= /tmp/postgis-interface.sqlite
MOBILITYDB_INTERFACE_DB  ?= /tmp/mobilitydb-interface.sqlite

.PHONY: smoke postgis mobilitydb \
    postgis-duckdb postgis-sqlite postgis-ducklink \
    mobilitydb-duckdb mobilitydb-sqlite mobilitydb-ducklink

smoke: postgis mobilitydb
	@echo ""
	@echo "===== ALL SMOKE TESTS PASSED ====="

# By default include the wasm-component targets (sqlite via sqlink,
# duckdb via ducklink). The legacy native `-duckdb-` target is kept
# runnable but no longer part of `make postgis` / `make mobilitydb`
# — invoke it explicitly with `make postgis-duckdb` / `make
# mobilitydb-duckdb` if you still need it.
postgis: postgis-sqlite postgis-ducklink

mobilitydb: mobilitydb-sqlite mobilitydb-ducklink

postgis-duckdb:
	@echo "=== postgis × duckdb ==="
	@SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	 SHIM_INTERFACE_DB=$(POSTGIS_INTERFACE_DB) \
	 bash scripts/run.sh duckdb $(POSTGIS_DUCKDB_BRIDGE) $(POSTGIS_SHIM) cases/postgis
	@if [ -d cases/postgis-duckdb-only ]; then \
	    echo "=== postgis × duckdb (duckdb-only cases) ==="; \
	    SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	    SHIM_INTERFACE_DB=$(POSTGIS_INTERFACE_DB) \
	    bash scripts/run.sh duckdb $(POSTGIS_DUCKDB_BRIDGE) $(POSTGIS_SHIM) cases/postgis-duckdb-only; \
	fi

postgis-sqlite:
	@echo "=== postgis × sqlite ==="
	@SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	 SHIM_INTERFACE_DB=$(POSTGIS_INTERFACE_DB) \
	 bash scripts/run.sh sqlite $(POSTGIS_SQLITE_BRIDGE) $(POSTGIS_SHIM) cases/postgis
	@if [ -d cases/postgis-sqlite-only ]; then \
	    echo "=== postgis × sqlite (sqlite-only cases) ==="; \
	    SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	    SHIM_INTERFACE_DB=$(POSTGIS_INTERFACE_DB) \
	    bash scripts/run.sh sqlite $(POSTGIS_SQLITE_BRIDGE) $(POSTGIS_SHIM) cases/postgis-sqlite-only; \
	fi

mobilitydb-duckdb:
	@echo "=== mobilitydb × duckdb ==="
	@SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	 SHIM_INTERFACE_DB=$(MOBILITYDB_INTERFACE_DB) \
	 bash scripts/run.sh duckdb $(MOBILITYDB_DUCKDB_BRIDGE) $(MOBILITYDB_SHIM) cases/mobilitydb
	@if [ -d cases/mobilitydb-duckdb-only ]; then \
	    echo "=== mobilitydb × duckdb (duckdb-only cases) ==="; \
	    SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	    SHIM_INTERFACE_DB=$(MOBILITYDB_INTERFACE_DB) \
	    bash scripts/run.sh duckdb $(MOBILITYDB_DUCKDB_BRIDGE) $(MOBILITYDB_SHIM) cases/mobilitydb-duckdb-only; \
	fi

mobilitydb-sqlite:
	@echo "=== mobilitydb × sqlite ==="
	@SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	 SHIM_INTERFACE_DB=$(MOBILITYDB_INTERFACE_DB) \
	 bash scripts/run.sh sqlite $(MOBILITYDB_SQLITE_BRIDGE) $(MOBILITYDB_SHIM) cases/mobilitydb

postgis-ducklink:
	@echo "=== postgis × ducklink ==="
	@SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	 SHIM_INTERFACE_DB=$(POSTGIS_INTERFACE_DB) \
	 bash scripts/run.sh ducklink $(POSTGIS_DUCKLINK_BRIDGE) $(POSTGIS_SHIM) cases/postgis
	@if [ -d cases/postgis-duckdb-only ]; then \
	    echo "=== postgis × ducklink (duckdb-only cases) ==="; \
	    SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	    SHIM_INTERFACE_DB=$(POSTGIS_INTERFACE_DB) \
	    bash scripts/run.sh ducklink $(POSTGIS_DUCKLINK_BRIDGE) $(POSTGIS_SHIM) cases/postgis-duckdb-only; \
	fi

mobilitydb-ducklink:
	@echo "=== mobilitydb × ducklink ==="
	@SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	 SHIM_INTERFACE_DB=$(MOBILITYDB_INTERFACE_DB) \
	 bash scripts/run.sh ducklink $(MOBILITYDB_DUCKLINK_BRIDGE) $(MOBILITYDB_SHIM) cases/mobilitydb
	@if [ -d cases/mobilitydb-duckdb-only ]; then \
	    echo "=== mobilitydb × ducklink (duckdb-only cases) ==="; \
	    SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	    SHIM_INTERFACE_DB=$(MOBILITYDB_INTERFACE_DB) \
	    bash scripts/run.sh ducklink $(MOBILITYDB_DUCKLINK_BRIDGE) $(MOBILITYDB_SHIM) cases/mobilitydb-duckdb-only; \
	fi
