# DataFission shim → SQL bridges fleet

Index of the repos that turn a DataFission wasm shim (PostGIS,
MobilityDB, address-standardizer, tiger-geocoder, …) into a
loadable **wasm-component** extension for a wasm-runtime SQL host
(sqlink for SQLite, ducklink for DuckDB) — and the codegen that
produces them.

**Everything is `wasm32-wasip2`.** No native cdylibs. Both hosts
(`sqlink` and `ducklink`) embed a wasmtime runtime and load the
composed loadable via the appropriate WIT contract.

All repos live as siblings under `~/git/` in the **`tegmentum`**
GitHub org. A few legacy `zacharywhitley/*` and `tegmentum/*`
repos are archived — see "Archived legacy" below.

## Pipeline

```
 upstream shim (wasm component)           runtime host (wasmtime-embedded)
  ├─ postgis-wasm                          ├─ sqlink       (SQLite host)
  ├─ mobilitydb-wasm                       └─ ducklink     (DuckDB host)
  ├─ address-standardizer-wasm
  └─ tiger-geocoder-wasm
              │
              ▼
   ┌───────────────────────────┐          shim-interface-core   ← engine
   │ <ext>-shim-interface       │ ──uses──▶ (walks any wasm shim,
   │  (thin driver binary)      │           writes SQL surface to sqlite)
   └───────────┬───────────────┘
               │ emits
               ▼
     <ext>-interface.sqlite
        (portable contract — the "BridgePlan" source of truth)
               │
               │ consumed by
               ▼
   ┌───────────────────────────┐          sqlink-shim-codegen (CLI)
   │ sqlink-shim-codegen        │ ──uses──▶ datalink-shim-sqlite-emit    (--target sqlite)
   │  --target ⟨sqlite|duckdb   │           datalink-shim-duckdb-emit    (--target duckdb)
   │           |datafission⟩    │           datalink-shim-datafission-emit
   │                            │           (all in ~/git/datalink)
   └───────────┬───────────────┘
               │ emits
               ▼
     <ext>-⟨sqlink|ducklink⟩-bridge crate
        (wasm32-wasip2, `crate-type = ["cdylib"]`, wit-bindgen)
               │
               │ cargo build --target wasm32-wasip2 --release
               │ wac plug ⟨bridge⟩.wasm --plug ⟨shim-composed⟩.wasm
               ▼
     <ext>-⟨sqlink|ducklink⟩-loadable.wasm
               │
               │ loaded by the runtime host
               ▼
   shim-bridge-smoke-tests exercises the composed loadable
   through the sqlink/ducklink CLI against .sql / .expected pairs.
```

## Layer 1 — Codegen engine (shared infra)

Generic. Extension-agnostic. Target-agnostic.

| Repo | Role |
| --- | --- |
| `shim-interface-core` | Walks any DataFission shim `.wasm` (`datafission:df-plugin-api/extension@1.0.0`) and writes its SQL surface to a SQLite DB. Used by every `<ext>-shim-interface` driver. |
| `shim-bridge-codegen-core` | Original `BridgePlan` loader + marshal primitives. Still git-dep'd at rev `1bcc5bb`; partially superseded by `datalink-shim-codegen-core` inside the datalink workspace. |
| `shim-sql-preprocess` | Parser-based SQL rewrite (`sqlparser-rs`) for operator / cast / preprocessor support the loadable-extension boundary can't intercept. Used at query-execution time. |
| `shim-bridge-smoke-tests` | E2E runner: for each (target, bridge, composed shim) tuple, executes `.sql` fixtures through the sqlink/ducklink CLI and diffs against `.expected`. |

## Layer 2 — Codegen CLI

**One codegen**, four output targets. The local `sqlink-shim-codegen`
repo is now a thin CLI shell that dispatches to emitter crates
in the `~/git/datalink` workspace (git-dep'd at rev `8211a75`).

| Repo | Role |
| --- | --- |
| `sqlink-shim-codegen` | CLI. Reads an interface `.sqlite`, dispatches to a `datalink-shim-*-emit` crate based on `--target`, writes a bridge crate to `--out`. |

Emitter crates (live in `tegmentum/datalink/crates/`, consumed as git deps):

- `datalink-shim-codegen-core` — shared BridgePlan augmentation + WIT reclassification
- `datalink-shim-sqlite-emit`  — sqlite:extension WIT (targets sqlink)
- `datalink-shim-duckdb-emit`  — duckdb:extension WIT (targets ducklink)
- `datalink-shim-datafission-emit` — datafission:extension@1.0.0 composite world

### `--target` flag matrix

| flag | emits | host | status |
| --- | --- | --- | --- |
| `sqlite` (default) | `wasm32-wasip2` component exporting `sqlite:extension` | sqlink | full surface |
| `wasm-component` | historical alias for `sqlite` | sqlink | full surface |
| `duckdb` | `wasm32-wasip2` component exporting `duckdb:extension@2.2.0` | ducklink | scalar-first; aggregates/UDTFs/casts return `Duckerror::Unsupported` until follow-up datalink work lands |
| `datafission` | `wasm32-wasip2` component exporting `datafission:extension@1.0.0` composite world | either (via datafission composite) | scalar-first |
| `native-dylib` | native `cdylib` embedding wasmtime | — | **legacy**, slated for removal per `PLAN-codegen-retarget.md` Phase 5 |

## Layer 3 — Per-extension shim interfaces

Thin drivers that wrap `shim-interface-core` with an
extension-specific starter query set + a per-extension binary
name (`extract-<ext>-interface`).

| Repo | Extractor binary |
| --- | --- |
| `postgis-shim-interface` | `extract-postgis-interface` |
| `mobilitydb-shim-interface` | `extract-mobilitydb-interface` |

Both require sibling checkouts of `shim-interface-core` and
`datafission` (path deps).

## Layer 4 — Generated bridges (extension × host matrix)

**Do not hand-edit** — regenerate from the interface DB via the
Layer 2 codegen. All bridges are `wasm32-wasip2` components.

### Umbrella bridges (`compose:dynlink` runtime, Phase 9.4)

The umbrella bridges resolve the corresponding `<ext>-composed`
provider at LOAD time via `compose:dynlink/linker.resolve-by-id`;
the host's `{SQLINK,DUCKLINK}_SUB_EXT_PREBUILT=<ext>=<path>` chain
points at a vendored `<ext>-monolith-provider.wasm`. Emitted by
`sqlink-shim-codegen --dynlink --target <ext>` off
`<ext>-catalog.toml`'s `<ext>` umbrella id (every leaf the umbrella
expands to contributes scalars + aggregates).

|  | sqlink (SQLite host) | ducklink (DuckDB host) | provider id |
| --- | --- | --- | --- |
| **postgis** | `postgis-sqlink-bridge` | `postgis-ducklink-bridge` | `postgis-composed` |
| **mobilitydb** | `mobilitydb-sqlink-bridge` | `mobilitydb-ducklink-bridge` | `mobilitydb-composed` |
| **timescaledb** | `timescaledb-sqlink-bridge` | `timescaledb-ducklink-bridge` | `timescaledb-composed` |
| **address-standardizer** ‡ | `address-standardizer-sqlink-bridge` | — | — |
| **tiger-geocoder** ‡ † | `tiger-geocoder-sqlink-bridge` | — | — |

‡ Still on the legacy wac-plug path; Phase 9.4 migration deferred.

† PostGIS-family scalar surface, US-locale only.

**Phase 9.4 wire-up.** The umbrella bridges retired the
build-time `wac plug` step. Each loadable wasm is ~0.2–1.4 MB
(bridge only); the ~120 MB shim ships once per family as a
vendored `<ext>-monolith-provider.wasm` under
`datafission/extensions/<ext>/deps/`. Compared to the retired
wac-plug loadables the bridge shrinks by ~100–300× — worked
example:

* `postgis-ducklink-loadable.wasm` (wac-plug): **128 MB**
* `postgis_duckdb_bridge_dynlink.wasm` (compose:dynlink): **816 KB**

**Known emit gap.** The dynlink emitter's scalar-first cut
means UDTFs / table-functions / spatial-index registration
paths stub out today (`postgis-sqlite-only/05-udtfs`,
`postgis-duckdb-only/01-table-functions,02-index-limitation,
03-spatial-index` fail against the umbrella bridges — same 4
cases failed before the migration). The umbrella smoke targets
(`make postgis-sqlite postgis-ducklink mobilitydb-sqlite
mobilitydb-ducklink timescaledb-sqlite timescaledb-ducklink`)
cover the scalar/load path.

### Per-sub-extension bridges (compose:dynlink runtime, Phase B)

Emitted by `sqlink-shim-codegen --dynlink --target <sub-ext>` off the
`sql-extension-catalog.toml` per-extension manifest. Each bridge imports
`compose:dynlink/linker@0.1.0` and resolves a resident composed provider
at LOAD time via `<sub-ext>-composed` id. Provider comes from the shim's
per-sub `wit-stub-gen`-completed composed wasm (all external component
imports satisfied by no-op stubs; instantiation is self-contained).

|  | sqlink (SQLite host) | ducklink (DuckDB host) | provider id |
| --- | --- | --- | --- |
| **postgis_core** | `postgis_core-sqlink-bridge` | `postgis_core-ducklink-bridge` | `postgis_core-composed` |
| **postgis_sfcgal** | `postgis_sfcgal-sqlink-bridge` | `postgis_sfcgal-ducklink-bridge` | `postgis_sfcgal-composed` |
| **postgis_raster** | `postgis_raster-sqlink-bridge` | `postgis_raster-ducklink-bridge` | `postgis_raster-composed` |
| **postgis_format_encoders** | `postgis_format_encoders-sqlink-bridge` | `postgis_format_encoders-ducklink-bridge` | `postgis_format_encoders-composed` |
| **postgis_metadata** † | `postgis_metadata-sqlink-bridge` | `postgis_metadata-ducklink-bridge` | `postgis_core-composed` |
| **postgis_3d** † | `postgis_3d-sqlink-bridge` | `postgis_3d-ducklink-bridge` | `postgis_core-composed` |
| **postgis_topology** † | `postgis_topology-sqlink-bridge` | `postgis_topology-ducklink-bridge` | `postgis_core-composed` |
| **postgis_clustering** † | `postgis_clustering-sqlink-bridge` | `postgis_clustering-ducklink-bridge` | `postgis_core-composed` |

† **Phase 9.1 shared-shim aliases.** These 4 sub-exts fold into
`postgis_core` at the shim level — one shared 30 MB
`postgis-core-composed.wasm` serves all 5 postgis_core-family bridges
via `compose:dynlink/linker.resolve-by-id("postgis_core-composed")`.
Codegen with `--provider-id postgis_core-composed`; host wiring adds
`{SQLINK,DUCKLINK}_SUB_EXT_ALIAS=postgis_metadata=postgis_core:postgis_3d=postgis_core:postgis_topology=postgis_core:postgis_clustering=postgis_core`
so the host's `SubExtLoader::materialize_sub_ext_provider` recurses
into `postgis_core`'s registration instead of registering 4 duplicate
30MB entries in the `ProviderRegistry`. Each bridge exposes only its
own scoped scalar set (~28 for metadata, ~15 for 3d, ~145 for
topology, ~13 for clustering) — the shared shim implements ALL of
them but the aliased bridge's `register_scalars()` only advertises
that sub-ext's slice.
| **timescale_meta** | `timescale_meta-sqlink-bridge` | `timescale_meta-ducklink-bridge` |
| **timescale_time_bucket** | `timescale_time_bucket-sqlink-bridge` | `timescale_time_bucket-ducklink-bridge` |
| **timescale_gapfill** | `timescale_gapfill-sqlink-bridge` | `timescale_gapfill-ducklink-bridge` |
| **timescale_hyperfunctions** | `timescale_hyperfunctions-sqlink-bridge` | `timescale_hyperfunctions-ducklink-bridge` |
| **timescale_toolkit_sketches** | `timescale_toolkit_sketches-sqlink-bridge` | `timescale_toolkit_sketches-ducklink-bridge` |
| **timescale_toolkit_stats** | `timescale_toolkit_stats-sqlink-bridge` | `timescale_toolkit_stats-ducklink-bridge` |
| **timescale_toolkit_categorical** | `timescale_toolkit_categorical-sqlink-bridge` | `timescale_toolkit_categorical-ducklink-bridge` |
| **timescale_toolkit_timevector** | `timescale_toolkit_timevector-sqlink-bridge` | `timescale_toolkit_timevector-ducklink-bridge` |
| **timescale_compression** | `timescale_compression-sqlink-bridge` | `timescale_compression-ducklink-bridge` |
| **timescale_hypertable** | `timescale_hypertable-sqlink-bridge` | `timescale_hypertable-ducklink-bridge` |
| **timescale_continuous_agg** | `timescale_continuous_agg-sqlink-bridge` | `timescale_continuous_agg-ducklink-bridge` |
| **timescale_policy** | `timescale_policy-sqlink-bridge` | `timescale_policy-ducklink-bridge` |
| **mobilitydb_core_types** | `mobilitydb_core_types-sqlink-bridge` | `mobilitydb_core_types-ducklink-bridge` |
| **mobilitydb_span_set** | `mobilitydb_span_set-sqlink-bridge` | `mobilitydb_span_set-ducklink-bridge` |
| **mobilitydb_stbox** | `mobilitydb_stbox-sqlink-bridge` | `mobilitydb_stbox-ducklink-bridge` |
| **mobilitydb_temporal_jsonb** | `mobilitydb_temporal_jsonb-sqlink-bridge` | `mobilitydb_temporal_jsonb-ducklink-bridge` |
| **mobilitydb_bitemporal** | `mobilitydb_bitemporal-sqlink-bridge` | `mobilitydb_bitemporal-ducklink-bridge` |
| **mobilitydb_temporal_generic** | `mobilitydb_temporal_generic-sqlink-bridge` | `mobilitydb_temporal_generic-ducklink-bridge` |
| **mobilitydb_temporal_scalar** | `mobilitydb_temporal_scalar-sqlink-bridge` | `mobilitydb_temporal_scalar-ducklink-bridge` |
| **mobilitydb_tcbuffer** | `mobilitydb_tcbuffer-sqlink-bridge` | `mobilitydb_tcbuffer-ducklink-bridge` |
| **mobilitydb_tpose** | `mobilitydb_tpose-sqlink-bridge` | `mobilitydb_tpose-ducklink-bridge` |
| **mobilitydb_network** | `mobilitydb_network-sqlink-bridge` | `mobilitydb_network-ducklink-bridge` |
| **mobilitydb_analytics** | `mobilitydb_analytics-sqlink-bridge` | `mobilitydb_analytics-ducklink-bridge` |
| **mobilitydb_pattern_detection** | `mobilitydb_pattern_detection-sqlink-bridge` | `mobilitydb_pattern_detection-ducklink-bridge` |
| **mobilitydb_clustering** | `mobilitydb_clustering-sqlink-bridge` | `mobilitydb_clustering-ducklink-bridge` |
| **mobilitydb_indexes** | `mobilitydb_indexes-sqlink-bridge` | `mobilitydb_indexes-ducklink-bridge` |
| **mobilitydb_io** | `mobilitydb_io-sqlink-bridge` | `mobilitydb_io-ducklink-bridge` |
| **mobilitydb_table_functions** | `mobilitydb_table_functions-sqlink-bridge` | `mobilitydb_table_functions-ducklink-bridge` |
| **mobilitydb_spatiotemporal** ‡ | `mobilitydb_spatiotemporal-sqlink-bridge` | `mobilitydb_spatiotemporal-ducklink-bridge` |

Host wiring — DuckDB / ducklink (mirror for sqlink):
```
DUCKLINK_SUB_EXT_BRIDGES=<sub_ext>=<bridge.wasm>
DUCKLINK_SUB_EXT_PREBUILT=<sub_ext>=<self-contained-provider.wasm>
ducklink -- duckdb-cli :memory: -c "LOAD <sub_ext>; SELECT ..."
```

† `tiger-geocoder-sqlink-bridge` builds + composes but does not
yet instantiate at load time — the upstream shim imports
`wasi:http/outgoing-handler@0.2.8` and `sqlink-host` does not
yet link `wasmtime-wasi-http`.

‡ `mobilitydb_spatiotemporal` declares a cross-extension dep on
`postgis:postgis_core_types` (tgeompoint/tgeogpoint/tgeography/
tgeometry sit on PostGIS's geometry/geography). The bridge crate
still emits + builds; the composed provider must link both the
MobilityDB spatiotemporal sub-shim and the PostGIS core provider
at LOAD time.

The ducklink bridges are currently scalar-first (per the
`--target duckdb` codegen status); aggregates/UDTFs/casts land
in follow-up datalink-shim-duckdb-emit work.

Every bridge crate expects sibling checkouts of the upstream
shim workspace (`../datafission/crates/`) for path deps.

## Regeneration flow (canonical)

Concrete example — postgis, both hosts.

```sh
# 1. Extract interface DB (per extension, once per shim rev)
#    NOTE: extract uses the DATAFISSION-composed shim (the one
#    exporting datafission:*), which drives surface extraction.
cd ~/git/postgis-shim-interface
cargo run --release -- \
  --wasm ~/git/datafission/extensions/postgis/target/wasm32-wasip2/release/postgis-shim-composed.wasm \
  --output ./postgis-interface.sqlite --summary

# 2. Regenerate bridge crate (per host)
CODEGEN=~/git/sqlink-shim-codegen/target/release/sqlink-shim-codegen
$CODEGEN --interface ~/git/postgis-shim-interface/postgis-interface.sqlite \
         --out       ~/git/postgis-sqlink-bridge   --target sqlite
$CODEGEN --interface ~/git/postgis-shim-interface/postgis-interface.sqlite \
         --out       ~/git/postgis-ducklink-bridge --target duckdb

# 3. Build each bridge to wasm
cd ~/git/postgis-sqlink-bridge   && cargo build --target wasm32-wasip2 --release
cd ~/git/postgis-ducklink-bridge && cargo build --target wasm32-wasip2 --release

# 4. Compose bridge + shim into one loadable
#    NOTE: use `datafission/extensions/postgis/deps/postgis-composed.wasm`,
#    NOT `~/git/postgis-wasm/postgis-composed.wasm`. The datafission
#    copy has kebab-fixed extern names (`st-is-threed` not `st-is-3d`,
#    etc.) applied by `datafission/scripts/fix-postgis-kebab.sh`,
#    matching the codegen's `kebab_fix_wit` transformation of the
#    bridge's WIT. The raw postgis-wasm shim will fail wac plug with
#    a resource-identity mismatch. Same rule for mobilitydb.
wac plug \
  --plug ~/git/datafission/extensions/postgis/deps/postgis-composed.wasm \
  --output ~/git/postgis-sqlink-bridge/postgis-sqlink-loadable.wasm \
  ~/git/postgis-sqlink-bridge/target/wasm32-wasip2/release/postgis_sqlink_bridge.wasm

wac plug \
  --plug ~/git/datafission/extensions/postgis/deps/postgis-composed.wasm \
  --output ~/git/postgis-ducklink-bridge/postgis-ducklink-loadable.wasm \
  ~/git/postgis-ducklink-bridge/target/wasm32-wasip2/release/postgis_ducklink_bridge.wasm

# 5. Load into the host
# sqlink:
/opt/homebrew/opt/sqlite/bin/sqlite3 <<'SQL'
.load ~/git/sqlink/target/release/libsqlink_loader.dylib
.load ~/git/postgis-sqlink-bridge/postgis-sqlink-loadable.wasm postgis_sqlink
SELECT st_astext(st_geomfromtext('POINT(1 2)'));
SQL

# ducklink: analogous via the ducklink loader (embeds wasmtime;
# exposes duckdb:extension host interfaces).

# 6. Smoke-test
cd ~/git/shim-bridge-smoke-tests && make smoke
```

### Regeneration flow — Phase B per-sub bridges (compose:dynlink)

For per-sub-extension bridges, `wac plug` composition is REPLACED by
runtime `compose:dynlink` resolution. Instead of one composed
loadable, each sub-ext ships two artifacts: a **bridge wasm** (small,
exports the host contract, imports `compose:dynlink/linker`) and a
**prebuilt composed provider** (self-contained, registers under
`<sub_ext>-composed` at LOAD time).

```sh
CODEGEN=~/git/sqlink-shim-codegen/target/release/sqlink-shim-codegen
CATALOG=~/git/postgis-shim-interface/postgis-catalog.toml
IFACE=~/git/postgis-shim-interface/postgis-interface.sqlite

# Generate + build all 8 postgis per-sub bridges (4 subs × 2 hosts).
for sub in postgis_core postgis_sfcgal postgis_raster postgis_format_encoders; do
  for tgt in sqlite duckdb; do
    label=${tgt/sqlite/sqlink}; label=${label/duckdb/ducklink}
    OUT=~/git/bridges/per-sub/${sub}-${label}-bridge
    $CODEGEN --dynlink --target-dialect $tgt --target $sub \
             --catalog $CATALOG --interface $IFACE --out $OUT
    (cd $OUT && cargo build --target wasm32-wasip2 --release)
  done
done

# Vendored per-sub composed provider wasms sit at
#   datafission/extensions/postgis/deps/postgis-<sub>-composed.wasm
# (already kebab-fixed; b3 pins in postgis-composed-pin.txt under
# `sub_<sub>_blake3=` / `sub_<sub>_blake3_pristine=` keys).
#
# Configure the host via env vars — sqlink and ducklink both accept
# the same `<name>=<path>:<name>=<path>` shape:
export SQLINK_SUB_EXT_PREBUILT="\
postgis_core=~/git/datafission/extensions/postgis/deps/postgis-core-composed.wasm:\
postgis_raster=~/git/datafission/extensions/postgis/deps/postgis-raster-composed.wasm:\
postgis_sfcgal=~/git/datafission/extensions/postgis/deps/postgis-sfcgal-composed.wasm:\
postgis_format_encoders=~/git/datafission/extensions/postgis/deps/postgis-format-encoders-composed.wasm"

export SQLINK_SUB_EXT_BRIDGES="\
postgis_core=~/git/bridges/per-sub/postgis_core-sqlink-bridge/target/wasm32-wasip2/release/postgis_core_sqlite_bridge_dynlink.wasm:\
postgis_raster=~/git/bridges/per-sub/postgis_raster-sqlink-bridge/target/wasm32-wasip2/release/postgis_raster_sqlite_bridge_dynlink.wasm:\
postgis_sfcgal=~/git/bridges/per-sub/postgis_sfcgal-sqlink-bridge/target/wasm32-wasip2/release/postgis_sfcgal_sqlite_bridge_dynlink.wasm:\
postgis_format_encoders=~/git/bridges/per-sub/postgis_format_encoders-sqlink-bridge/target/wasm32-wasip2/release/postgis_format_encoders_sqlite_bridge_dynlink.wasm"

# The DUCKLINK_* variants of these two vars use the same shape.
```

Regenerating kebab-fixed per-sub composed wasms after an upstream
postgis-wasm bump:

```sh
# Copy fresh compose output from postgis-wasm, kebab-fix in place.
for sub in core sfcgal raster format-encoders; do
  cp ~/git/postgis-wasm/build/plans/postgis-${sub}-composed.wasm \
     ~/git/datafission/extensions/postgis/deps/
  ~/git/datafission/scripts/fix-postgis-kebab.sh --sub-ext ${sub}
done
# Update sub_<sub>_blake3=... entries in postgis-composed-pin.txt with
# the new b3sums printed by the script.
```

## Why the datafission-vendored shim (not `postgis-wasm/postgis-composed.wasm`)

Both the bridge WIT and the shim's wasm-level extern names go
through a **kebab-fix pass** that rewrites `-3d`/`-2d`/`-4d`
(and bare trailing `-2`/`-3`/`-4`) into word forms
(`-threed`/`-twod`/`-fourd`/`-two`/`-three`/`-four`). Older
wit-bindgen accepts raw digit-suffixed idents; new wit-bindgen
(0.37+) rejects them, and the emitted bridge crates consume the
new wit-bindgen.

Two independent implementations of the same rewrite must stay in
lockstep:

- **WIT-text side** — `datalink-shim-codegen-core::kebab_fix::kebab_fix_wit`
  is called during `write_deps` when the codegen copies upstream
  WIT into the bridge's `wit/deps/`. So the bridge's WIT is the
  kebab-fixed form.
- **wasm-name side** — `datafission/scripts/fix-postgis-kebab.sh`
  patches the SAME identifiers at the wasm extern-name level and
  bakes the result into `datafission/extensions/postgis/deps/postgis-composed.wasm`
  (see `postgis-composed-pin.txt` for the pin workflow).

`~/git/postgis-wasm/postgis-composed.wasm` has RAW extern names
and does NOT match the codegen's bridge WIT. `wac plug` will
fail with a resource-identity mismatch on `postgis-processing/geometry`.

Same rule for mobilitydb — always compose against the
datafission-vendored shim (which chains postgis + mdb-temporal
through `datafission/extensions/mobilitydb/deps/*.wasm`).

## Where the codegen sources upstream WIT

`try_synthesize_upstream_deps` (in
`datalink-shim-codegen-core::wit_paths`) reads from
`~/git/postgis-wasm/wit/` (+ its 14 helper packages under
`wit/deps/`), applies `kebab_fix_wit`, and writes to
**`$TMPDIR/sqlink-codegen-upstream-<primary>/`**. On macOS
`$TMPDIR` is `/var/folders/…/T/` (NOT `/tmp`) — look there when
verifying the synth cache.

Every codegen run rewrites this cache from scratch, so upstream
WIT changes flow automatically. Override with
`SQLINK_SHIM_WIT_DEPS=/path/to/wit/deps` or
`SQLINK_{POSTGIS,MOBILITYDB}_BRIDGE_WIT_DEPS`; the last-resort
fallback is `~/git/sqlink/extensions/postgis-bridge/wit/deps`.

## Upstream / adjacent repos (not part of this fleet, but referenced)

- **Shims (wasm source):** `postgis-wasm`, `mobilitydb-wasm`,
  `address-standardizer-wasm`, `tiger-geocoder-wasm`
- **Runtime hosts:** `sqlink` (SQLite + wasm-component ecosystem),
  `ducklink` (DuckDB + wasm-component ecosystem)
- **Codegen workspace:** `datalink` (contains `datalink-shim-*`
  emitter crates that `sqlink-shim-codegen` dispatches to)
- **Contract crate:** `datafission` (df-plugin-api,
  df-plugin-loader, functions — path deps of every bridge)

## Archived legacy

Frozen; kept for history only. Do not regenerate; new work goes
into the wasm-component equivalents.

| Repo | State | Notes |
| --- | --- | --- |
| `zacharywhitley/postgis-sqlite-bridge` | archived | Legacy native-cdylib SQLite path via rusqlite. Superseded by `postgis-sqlink-bridge`. |
| `zacharywhitley/mobilitydb-sqlite-bridge` | archived | Legacy native-cdylib SQLite path via rusqlite. Superseded by `mobilitydb-sqlink-bridge`. |
| `zacharywhitley/shim-bridge-codegen-core` | archived | Prior mirror of `tegmentum/shim-bridge-codegen-core`. |
| `tegmentum/ducklink-shim-codegen` | archived | Emitted a native DuckDB cdylib with embedded wasmtime. Superseded by `sqlink-shim-codegen --target duckdb` → `datalink-shim-duckdb-emit`. |
| `tegmentum/mobilitydb-duckdb-bridge` | archived | Native DuckDB cdylib output of the archived `ducklink-shim-codegen`. Superseded by `mobilitydb-ducklink-bridge` (wasm-component). |
| `tegmentum/postgis-duckdb-bridge` | **renamed** → `tegmentum/postgis-ducklink-bridge` (retargeted from native to wasm-component in a breaking-change commit). |

## Not part of this fleet

- `pkcs11-bridge` — belongs to the separate PKCS11 / WebAuthn
  effort (`pkcs11-*` family, `pkcs11-wit`). Unrelated to
  DataFission shim codegen despite the "bridge" suffix.

## Ownership snapshot

All 13 active fleet repos live at **`github.com/tegmentum/`**:

- Layer 1 engine (4): `shim-interface-core`,
  `shim-bridge-codegen-core`, `shim-sql-preprocess`,
  `shim-bridge-smoke-tests`
- Layer 2 codegen CLI (1): `sqlink-shim-codegen`
  (dispatches to `datalink-shim-*-emit` in `tegmentum/datalink`)
- Layer 3 interfaces (2): `postgis-shim-interface`,
  `mobilitydb-shim-interface`
- Layer 4 bridges (6): `postgis-sqlink-bridge`,
  `postgis-ducklink-bridge`, `mobilitydb-sqlink-bridge`,
  `mobilitydb-ducklink-bridge`, `address-standardizer-sqlink-bridge`,
  `tiger-geocoder-sqlink-bridge`

Migration + retarget events (2026-07-07):
- 10 repos transferred/archived from `zacharywhitley/*` to `tegmentum/*`
- `ducklink-shim-codegen` archived (superseded by `datalink-shim-duckdb-emit`)
- `postgis-duckdb-bridge` renamed → `postgis-ducklink-bridge` + regen'd as wasm-component
- `mobilitydb-duckdb-bridge` archived; `mobilitydb-ducklink-bridge` regen'd

Old URLs auto-redirect.
