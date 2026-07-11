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
# Postgis dynlink monolithic bridges (Phase 9.3). The postgis surface
# now ships as two artifacts — a small (~400 KB) compose:dynlink bridge
# wasm that exports the host contract, and a shared postgis-monolith-
# provider.wasm (128 MB, vendored) that answers via
# compose:dynlink/endpoint. `make postgis-sqlite` and `make
# postgis-ducklink` route through these by default as of 2026-07-11.
#
# LEGACY (2026-07-10 and earlier, opt-in): the wac-plug monolithic
# loadables at ~/git/postgis-{sqlink,ducklink}-bridge/postgis-{...}-loadable.wasm
# target `duckdb:extension@2.2.0` and were verified BROKEN against the
# current ducklink CLI (@4.0.0) — 5/5 smoke cases FAIL. Override
# POSTGIS_{SQLITE,DUCKLINK}_BRIDGE to those paths on a matching-vintage
# CLI if you need to exercise the retiring path.
POSTGIS_SQLITE_BRIDGE       ?= $(HOME)/git/bridges/monolith/postgis-sqlink-bridge/target/wasm32-wasip2/release/postgis_sqlite_bridge_dynlink.wasm
POSTGIS_DUCKLINK_BRIDGE     ?= $(HOME)/git/bridges/monolith/postgis-ducklink-bridge/target/wasm32-wasip2/release/postgis_duckdb_bridge_dynlink.wasm
MOBILITYDB_DUCKDB_BRIDGE    ?= /tmp/mobilitydb_duckdb_bridge.duckdb_extension
# mobilitydb's wasm bridges need postgis loaded first for the
# GEOMETRY type (D5 load-order convention). The colon-separated
# path is decoded by scripts/run.sh into two `LOAD` statements
# in the listed order — same convention across sqlite (sqlink)
# and ducklink targets. mobilitydb-loadable paths retained pending
# the Phase 9.2 socket-import transition (upstream mobilitydb-wasm
# task); postgis half is dynlink-modern.
MOBILITYDB_SQLITE_BRIDGE    ?= $(POSTGIS_SQLITE_BRIDGE):$(HOME)/git/mobilitydb-sqlink-bridge/mobilitydb-sqlink-loadable.wasm
MOBILITYDB_DUCKLINK_BRIDGE  ?= $(POSTGIS_DUCKLINK_BRIDGE):$(HOME)/git/mobilitydb-ducklink-bridge/mobilitydb-ducklink-loadable.wasm
# Composed provider wasm — datafission-vendored self-contained
# postgis-monolith-provider.wasm (128 MB, exports
# compose:dynlink/endpoint@0.1.0). Phase 9.3 dynlink monolithic
# bridges register this as their composed provider at LOAD time
# via the SUB_EXT_PREBUILT env var chain; per-sub bridges use
# postgis-{core,sfcgal,raster,format-encoders}-provider-composed.wasm.
#
# The pre-Phase-9.3 postgis-composed.wasm (raw compose output,
# no compose:dynlink/endpoint) is retained in the deps dir for
# legacy-loadable regen, but the dynlink smoke targets use the
# monolith-provider variant.
POSTGIS_SHIM                ?= $(HOME)/git/datafission/extensions/postgis/deps/postgis-monolith-provider.wasm
MOBILITYDB_SHIM             ?= $(HOME)/git/mobilitydb-wasm/mobilitydb-composed.wasm

# Optional preprocessor wiring. When SHIM_SQL_PREPROCESS is set,
# scripts/run.sh pipes each case file through it (with the
# corresponding interface DB) before sending to the target CLI.
# Skip per-case via a `<case>.no-preprocess` marker file.
SHIM_SQL_PREPROCESS      ?= $(HOME)/git/shim-sql-preprocess/target/release/shim-sql-preprocess
POSTGIS_INTERFACE_DB     ?= $(HOME)/git/postgis-shim-interface/postgis-interface.sqlite
MOBILITYDB_INTERFACE_DB  ?= $(HOME)/git/mobilitydb-shim-interface/mobilitydb-interface.sqlite
TIMESCALEDB_INTERFACE_DB ?= $(HOME)/git/timescaledb-shim-interface/timescaledb-interface.sqlite

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
# Phase 9.2 mobilitydb per-sub bridge targets. Same shape as the
# postgis Phase B + shared-shim macros: each sub-ext's bridge routes
# through a mobilitydb per-sub composed provider vendored in
# datafission/extensions/mobilitydb/deps/. The catalog sub-ext names
# (mobilitydb_temporal_core etc.) alias to the plan-composed provider
# stems (mobilitydb_core etc.) — the alias map is exported so the
# SUB_EXT_ALIAS branch fires.
# ---------------------------------------------------------------

MDB_DEPS_DIR ?= $(HOME)/git/datafission/extensions/mobilitydb/deps
MDB_ALIAS ?= mobilitydb_temporal_core=mobilitydb_core:mobilitydb_temporal_scalar=mobilitydb_bigint:mobilitydb_temporal_jsonb=mobilitydb_jsonb:mobilitydb_spatiotemporal_core=mobilitydb_spatial:mobilitydb_spatiotemporal=mobilitydb_geometry:mobilitydb_network=mobilitydb_network:mobilitydb_analytics=mobilitydb_analytics

define _mdb_sub_target
$(1)-$(2):
	@echo "=== $(1) × $(2) (mobilitydb per-sub) ==="
	@dir=cases/$(1); if [ ! -d $$$$dir ]; then dir=cases/mobilitydb; fi; \
	 SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	 SHIM_INTERFACE_DB=$(MOBILITYDB_INTERFACE_DB) \
	 SUB_EXT_ALIAS="$(MDB_ALIAS)" \
	 bash scripts/run.sh $(3) \
	   $(BRIDGES_DIR)/$(1)-$(2)-bridge/target/wasm32-wasip2/release/$(1)_$(4)_bridge_dynlink.wasm \
	   $(MDB_DEPS_DIR)/mobilitydb-$(5)-provider-composed.wasm \
	   $$$$dir
endef

$(eval $(call _mdb_sub_target,mobilitydb_temporal_core,sqlink,sqlite,sqlite,core))
$(eval $(call _mdb_sub_target,mobilitydb_temporal_core,ducklink,ducklink,duckdb,core))
$(eval $(call _mdb_sub_target,mobilitydb_temporal_scalar,sqlink,sqlite,sqlite,bigint))
$(eval $(call _mdb_sub_target,mobilitydb_temporal_scalar,ducklink,ducklink,duckdb,bigint))
$(eval $(call _mdb_sub_target,mobilitydb_temporal_jsonb,sqlink,sqlite,sqlite,jsonb))
$(eval $(call _mdb_sub_target,mobilitydb_temporal_jsonb,ducklink,ducklink,duckdb,jsonb))
$(eval $(call _mdb_sub_target,mobilitydb_spatiotemporal_core,sqlink,sqlite,sqlite,spatial))
$(eval $(call _mdb_sub_target,mobilitydb_spatiotemporal_core,ducklink,ducklink,duckdb,spatial))
$(eval $(call _mdb_sub_target,mobilitydb_spatiotemporal,sqlink,sqlite,sqlite,geometry))
$(eval $(call _mdb_sub_target,mobilitydb_spatiotemporal,ducklink,ducklink,duckdb,geometry))
$(eval $(call _mdb_sub_target,mobilitydb_network,sqlink,sqlite,sqlite,network))
$(eval $(call _mdb_sub_target,mobilitydb_network,ducklink,ducklink,duckdb,network))
$(eval $(call _mdb_sub_target,mobilitydb_analytics,sqlink,sqlite,sqlite,analytics))
$(eval $(call _mdb_sub_target,mobilitydb_analytics,ducklink,ducklink,duckdb,analytics))

.PHONY: mobilitydb-per-sub
mobilitydb-per-sub: \
    mobilitydb_temporal_core-sqlink mobilitydb_temporal_core-ducklink \
    mobilitydb_temporal_scalar-sqlink mobilitydb_temporal_scalar-ducklink \
    mobilitydb_temporal_jsonb-sqlink mobilitydb_temporal_jsonb-ducklink \
    mobilitydb_spatiotemporal_core-sqlink mobilitydb_spatiotemporal_core-ducklink \
    mobilitydb_spatiotemporal-sqlink mobilitydb_spatiotemporal-ducklink \
    mobilitydb_network-sqlink mobilitydb_network-ducklink \
    mobilitydb_analytics-sqlink mobilitydb_analytics-ducklink

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

# ---------------------------------------------------------------
# Phase 9.2 timescaledb per-sub bridge targets. 12 catalog sub-exts
# × 2 hosts = 24 targets, aliasing to 3 provider stems (core /
# toolkit / compression). Same structure as mobilitydb-per-sub;
# per-sub SHIM SPLITTING (arm-extraction per catalog sub-ext) is a
# follow-up — all 3 provider stems are byte-identical monoliths
# today, wired for the future when the provider crate grows per-sub
# modules.
# ---------------------------------------------------------------

TSDB_DEPS_DIR ?= $(HOME)/git/datafission/extensions/timescaledb/deps
TSDB_ALIAS ?= timescale_meta=timescale_core:timescale_time_bucket=timescale_core:timescale_gapfill=timescale_core:timescale_hyperfunctions=timescale_core:timescale_hypertable=timescale_core:timescale_continuous_agg=timescale_core:timescale_policy=timescale_core:timescale_toolkit_sketches=timescale_toolkit:timescale_toolkit_stats=timescale_toolkit:timescale_toolkit_categorical=timescale_toolkit:timescale_toolkit_timevector=timescale_toolkit:timescale_compression=timescale_compression

define _tsdb_sub_target
$(1)-$(2):
	@echo "=== $(1) × $(2) (timescaledb per-sub) ==="
	@dir=cases/$(1); if [ ! -d $$$$dir ]; then dir=cases/timescaledb; fi; \
	 SHIM_SQL_PREPROCESS=$(SHIM_SQL_PREPROCESS) \
	 SHIM_INTERFACE_DB=$(TIMESCALEDB_INTERFACE_DB) \
	 SUB_EXT_ALIAS="$(TSDB_ALIAS)" \
	 bash scripts/run.sh $(3) \
	   $(BRIDGES_DIR)/$(1)-$(2)-bridge/target/wasm32-wasip2/release/$(1)_$(4)_bridge_dynlink.wasm \
	   $(TSDB_DEPS_DIR)/timescaledb-$(5)-provider-composed.wasm \
	   $$$$dir
endef

$(eval $(call _tsdb_sub_target,timescale_meta,sqlink,sqlite,sqlite,core))
$(eval $(call _tsdb_sub_target,timescale_meta,ducklink,ducklink,duckdb,core))
$(eval $(call _tsdb_sub_target,timescale_time_bucket,sqlink,sqlite,sqlite,core))
$(eval $(call _tsdb_sub_target,timescale_time_bucket,ducklink,ducklink,duckdb,core))
$(eval $(call _tsdb_sub_target,timescale_gapfill,sqlink,sqlite,sqlite,core))
$(eval $(call _tsdb_sub_target,timescale_gapfill,ducklink,ducklink,duckdb,core))
$(eval $(call _tsdb_sub_target,timescale_hyperfunctions,sqlink,sqlite,sqlite,core))
$(eval $(call _tsdb_sub_target,timescale_hyperfunctions,ducklink,ducklink,duckdb,core))
$(eval $(call _tsdb_sub_target,timescale_hypertable,sqlink,sqlite,sqlite,core))
$(eval $(call _tsdb_sub_target,timescale_hypertable,ducklink,ducklink,duckdb,core))
$(eval $(call _tsdb_sub_target,timescale_continuous_agg,sqlink,sqlite,sqlite,core))
$(eval $(call _tsdb_sub_target,timescale_continuous_agg,ducklink,ducklink,duckdb,core))
$(eval $(call _tsdb_sub_target,timescale_policy,sqlink,sqlite,sqlite,core))
$(eval $(call _tsdb_sub_target,timescale_policy,ducklink,ducklink,duckdb,core))
$(eval $(call _tsdb_sub_target,timescale_toolkit_sketches,sqlink,sqlite,sqlite,toolkit))
$(eval $(call _tsdb_sub_target,timescale_toolkit_sketches,ducklink,ducklink,duckdb,toolkit))
$(eval $(call _tsdb_sub_target,timescale_toolkit_stats,sqlink,sqlite,sqlite,toolkit))
$(eval $(call _tsdb_sub_target,timescale_toolkit_stats,ducklink,ducklink,duckdb,toolkit))
$(eval $(call _tsdb_sub_target,timescale_toolkit_categorical,sqlink,sqlite,sqlite,toolkit))
$(eval $(call _tsdb_sub_target,timescale_toolkit_categorical,ducklink,ducklink,duckdb,toolkit))
$(eval $(call _tsdb_sub_target,timescale_toolkit_timevector,sqlink,sqlite,sqlite,toolkit))
$(eval $(call _tsdb_sub_target,timescale_toolkit_timevector,ducklink,ducklink,duckdb,toolkit))
$(eval $(call _tsdb_sub_target,timescale_compression,sqlink,sqlite,sqlite,compression))
$(eval $(call _tsdb_sub_target,timescale_compression,ducklink,ducklink,duckdb,compression))

.PHONY: timescaledb-per-sub
timescaledb-per-sub: \
    timescale_meta-sqlink timescale_meta-ducklink \
    timescale_time_bucket-sqlink timescale_time_bucket-ducklink \
    timescale_gapfill-sqlink timescale_gapfill-ducklink \
    timescale_hyperfunctions-sqlink timescale_hyperfunctions-ducklink \
    timescale_hypertable-sqlink timescale_hypertable-ducklink \
    timescale_continuous_agg-sqlink timescale_continuous_agg-ducklink \
    timescale_policy-sqlink timescale_policy-ducklink \
    timescale_toolkit_sketches-sqlink timescale_toolkit_sketches-ducklink \
    timescale_toolkit_stats-sqlink timescale_toolkit_stats-ducklink \
    timescale_toolkit_categorical-sqlink timescale_toolkit_categorical-ducklink \
    timescale_toolkit_timevector-sqlink timescale_toolkit_timevector-ducklink \
    timescale_compression-sqlink timescale_compression-ducklink
