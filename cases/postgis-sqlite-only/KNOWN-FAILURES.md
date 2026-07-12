# postgis × sqlite-only — known failures

## 05-udtfs — sqlink dynlink-bridge vtab dispatch is not wired

**Symptom:** `Parse error near line N: dispatch_vtab: extension postgis not loaded (no provider backing)` for every `SELECT count(*) FROM st_dumppoints(...)` and sibling UDTF invocation.

**Root cause:** sqlink-host's `dispatch_vtab_connect` / `dispatch_vtab_create` / friends (`host/src/lib.rs:7525-7761`) route through `try_provider_invoke` → `resident_provider_handle`, which reads from `provider_backed`. The compose:dynlink bridge loader (`load_extension_as_dynlink_bridge` at `host/src/lib.rs:6834`) only populates `dynlink_bridges` + `provider_manifests`, NOT `provider_backed`. And even if it did, the compose:dynlink resident provider (postgis-monolith-provider.wasm) has no vtab arms — vtabs are the bridge's responsibility (the bridge exports `sqlite:extension/vtab@1.0.0`).

The scalar tier has a `try_bridge_scalar` fallback (`host/src/lib.rs:6968`) that dispatches per-call through the resident bridge instance. There's no analogous `try_bridge_vtab_*` for the vtab tier — the sqlink-host code comments this out as a follow-up:

> Scope 1: scalars are exhaustive; aggregates/vtabs get their specs mirrored so the outer registration machinery can enumerate them, but per-tier dispatch through the bridge is a follow-up.

## Fix recipe

Add `try_bridge_vtab_*` methods analogous to `try_bridge_scalar`:

1. **`sqlink/host/src/lib.rs`** — for each of the 13 `dispatch_vtab_*` functions (`create`, `connect`, `destroy`, `disconnect`, `best_index`, `open`, `close`, `filter`, `next`, `eof`, `column`, `rowid`, `update`), add a sibling `try_bridge_vtab_*` that:
   - Fetches the bridge from `self.dynlink_bridges.read().get(ext_name)`
   - Locks the bridge instance
   - Calls the corresponding `bridge.instance.sqlite_extension_vtab().call_xxx(&mut bridge.store, ...)`
   - Converts host-side arg types to `loaded::sqlite::extension::*` types and back

2. **`dispatch_vtab_*`** — try the bridge path BEFORE the provider path (mirroring `dispatch_scalar`).

3. **Type conversion** — the bridge world uses `loaded::sqlite::extension::vtab::*` types. Existing converters like `convert_sql_value_to_loaded` handle `SqlValue`; new converters needed for `Constraint`, `OrderBy`, `IndexInfo`, `Colvec`, etc.

## Estimate

- 13 vtab methods × ~30-50 LOC each = 400-650 LOC
- 5-8 new type converters
- Testing: this smoke case + probably a few unit tests
- Estimated 1-2 days of focused work

## Not blocking

The primary postgis-sqlite smoke corpus (5 cases) passes without vtab dispatch — this failure only affects the optional sqlite-only UDTF corpus (single case).
