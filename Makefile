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
    mobilitydb-duckdb mobilitydb-sqlite mobilitydb-ducklink \
    postgis-per-sub \
    postgis_core-sqlink postgis_core-ducklink \
    postgis_sfcgal-sqlink postgis_sfcgal-ducklink \
    postgis_raster-sqlink postgis_raster-ducklink \
    postgis_format_encoders-sqlink postgis_format_encoders-ducklink

smoke: postgis mobilitydb
	@echo ""
	@echo "===== ALL SMOKE TESTS PASSED ====="

# Phase B compose:dynlink per-sub bridges. Each target loads a
# per-sub bridge wasm against the vendored per-sub prebuilt provider
# via SQLINK_SUB_EXT_* / DUCKLINK_SUB_EXT_* env vars. Bridge paths
# come from ~/git/bridges/per-sub/<sub>-<host>-bridge/target/... and
# prebuilt paths from datafission/extensions/postgis/deps/.
# The default `smoke` target does NOT include these because they
# depend on the per-sub bridge fleet being pre-built; run
# `make postgis-per-sub` explicitly after regen.
postgis-per-sub: \
    postgis_core-sqlink postgis_core-ducklink \
    postgis_sfcgal-sqlink postgis_sfcgal-ducklink \
    postgis_raster-sqlink postgis_raster-ducklink \
    postgis_format_encoders-sqlink postgis_format_encoders-ducklink

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

# ---------------------------------------------------------------
# Phase B per-sub bridge smoke targets. Each pattern:
#   1. Points scripts/run.sh at the per-sub bridge wasm as BRIDGE.
#   2. Points at the per-sub vendored composed prebuilt as SHIM.
#   3. Uses cases/postgis-<sub>/ if it exists, otherwise falls back
#      to cases/postgis/ (the top-level corpus) so operators can
#      progressively split cases without breaking the Makefile.
# ---------------------------------------------------------------

BRIDGES_DIR ?= $(HOME)/git/bridges/per-sub
DEPS_DIR    ?= $(HOME)/git/datafission/extensions/postgis/deps

define _per_sub_target
$(1)-$(2):
	@echo "=== $(1) × $(2) ==="
	@dir=cases/$(1); if [ ! -d $$$$dir ]; then dir=cases/postgis; fi; \
	 SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	 SHIM_INTERFACE_DB=$(POSTGIS_INTERFACE_DB) \
	 bash scripts/run.sh $(3) \
	   $(BRIDGES_DIR)/$(1)-$(2)-bridge/target/wasm32-wasip2/release/$(1)_$(4)_bridge_dynlink.wasm \
	   $(DEPS_DIR)/$(shell echo $(1) | tr _ -)-composed.wasm \
	   $$$$dir
endef

$(eval $(call _per_sub_target,postgis_core,sqlink,sqlite,sqlite))
$(eval $(call _per_sub_target,postgis_core,ducklink,ducklink,duckdb))
$(eval $(call _per_sub_target,postgis_sfcgal,sqlink,sqlite,sqlite))
$(eval $(call _per_sub_target,postgis_sfcgal,ducklink,ducklink,duckdb))
$(eval $(call _per_sub_target,postgis_raster,sqlink,sqlite,sqlite))
$(eval $(call _per_sub_target,postgis_raster,ducklink,ducklink,duckdb))
$(eval $(call _per_sub_target,postgis_format_encoders,sqlink,sqlite,sqlite))
$(eval $(call _per_sub_target,postgis_format_encoders,ducklink,ducklink,duckdb))

# ---------------------------------------------------------------
# Phase 9.1 shared-shim bridge targets. These sub-exts share the
# postgis-core-composed.wasm provider; the run.sh env var wiring
# adds SUB_EXT_ALIAS so `LOAD postgis_topology` resolves to the
# same shared provider `postgis_core` registers.
# ---------------------------------------------------------------

# Alias map exported to both hosts so `LOAD postgis_topology` etc.
# route through the postgis_core-composed provider. `run.sh` reads
# this and forwards to {SQLINK,DUCKLINK}_SUB_EXT_ALIAS in the
# spawned CLI environment.
POSTGIS_SHARED_SHIM_ALIAS ?= postgis_metadata=postgis_core:postgis_3d=postgis_core:postgis_topology=postgis_core:postgis_clustering=postgis_core

define _shared_sub_target
$(1)-$(2):
	@echo "=== $(1) × $(2) (shared-shim) ==="
	@dir=cases/$(1); if [ ! -d $$$$dir ]; then dir=cases/postgis; fi; \
	 SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	 SHIM_INTERFACE_DB=$(POSTGIS_INTERFACE_DB) \
	 SUB_EXT_ALIAS="$(POSTGIS_SHARED_SHIM_ALIAS)" \
	 bash scripts/run.sh $(3) \
	   $(BRIDGES_DIR)/$(1)-$(2)-bridge/target/wasm32-wasip2/release/$(1)_$(4)_bridge_dynlink.wasm \
	   $(DEPS_DIR)/postgis-core-composed.wasm \
	   $$$$dir
endef

$(eval $(call _shared_sub_target,postgis_metadata,sqlink,sqlite,sqlite))
$(eval $(call _shared_sub_target,postgis_metadata,ducklink,ducklink,duckdb))
$(eval $(call _shared_sub_target,postgis_3d,sqlink,sqlite,sqlite))
$(eval $(call _shared_sub_target,postgis_3d,ducklink,ducklink,duckdb))
$(eval $(call _shared_sub_target,postgis_topology,sqlink,sqlite,sqlite))
$(eval $(call _shared_sub_target,postgis_topology,ducklink,ducklink,duckdb))
$(eval $(call _shared_sub_target,postgis_clustering,sqlink,sqlite,sqlite))
$(eval $(call _shared_sub_target,postgis_clustering,ducklink,ducklink,duckdb))

.PHONY: postgis-shared-shim
postgis-shared-shim: \
    postgis_metadata-sqlink postgis_metadata-ducklink \
    postgis_3d-sqlink postgis_3d-ducklink \
    postgis_topology-sqlink postgis_topology-ducklink \
    postgis_clustering-sqlink postgis_clustering-ducklink

# ---------------------------------------------------------------
# Phase 9.3: dynlink-mode monolithic postgis bridge (side-by-side
# with the legacy `postgis-{sqlink,ducklink}` targets during the
# parity soak). Two artifacts replace the two 122MB `wac plug` loadables:
#   - postgis_{sqlite,duckdb}_bridge_dynlink.wasm (~300-800KB)
#   - postgis-monolith-provider.wasm (128MB, vendored)
# ---------------------------------------------------------------

MONOLITH_BRIDGES_DIR ?= $(HOME)/git/bridges/monolith
POSTGIS_MONOLITH_PROVIDER ?= $(DEPS_DIR)/postgis-monolith-provider.wasm

define _monolith_target
$(1)-monolith-dynlink:
	@echo "=== $(1) postgis monolith × dynlink ==="
	@SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	 SHIM_INTERFACE_DB=$(POSTGIS_INTERFACE_DB) \
	 bash scripts/run.sh $(2) \
	   $(MONOLITH_BRIDGES_DIR)/postgis-$(1)-bridge/target/wasm32-wasip2/release/postgis_$(3)_bridge_dynlink.wasm \
	   $(POSTGIS_MONOLITH_PROVIDER) \
	   cases/postgis
endef

$(eval $(call _monolith_target,sqlink,sqlite,sqlite))
$(eval $(call _monolith_target,ducklink,ducklink,duckdb))

.PHONY: postgis-monolith-dynlink sqlink-monolith-dynlink ducklink-monolith-dynlink
postgis-monolith-dynlink: sqlink-monolith-dynlink ducklink-monolith-dynlink
