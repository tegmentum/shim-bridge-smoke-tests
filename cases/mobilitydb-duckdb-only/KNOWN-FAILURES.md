# mobilitydb-duckdb-only: known cross-repo failures

Baseline against the arm-wiring effort of 2026-07-11: `cases/mobilitydb-duckdb-only`
scores 4/13 pass. `cases/mobilitydb` (primary corpus) is at 4/4 PASS.
Update 2026-07-11 (tranche after fleet arm-wiring closed 4/13): sharpened
recipes below reflect re-runs against the current bridges.

An investigator sub-classified the failing cases into two buckets:

- **Bucket A / dispatch-arm work in `mdb_temporal` provider**: closed by
  the fleet arm-wiring pass.
- **Bucket B / cross-repo work spanning shim registry + bridge codegen +
  preprocess**: the 5 cases documented below.

This file scopes bucket B — all five require multi-repo work that cannot
be resolved by adding a dispatch arm alone.

## `11-udafs` — new UDAF surface required (F64-input aggregates) — Path (B) only

**Failure** (confirmed 2026-07-11 re-run): case calls `tfloat_max_agg(v)`,
`tfloat_min_agg(v)`, `tfloat_count_agg(v)` where `v` is `DOUBLE`. Case
returns empty output (0 rows) — DuckDB's binder cannot resolve
`tfloat_max_agg(DOUBLE)` because no such aggregate is registered and the
suppressed error is filtered out by the smoke runner's numeric-line
whitelist.

**Path (A) alias route is dead**: aliasing `tfloat_temporal_max` →
`tfloat_max_agg` (mirroring the `tand`/`tor`/`tavg` pattern at
`~/git/datafission/extensions/mobilitydb/src/lib.rs:44738-44828`) fails
for THREE independent reasons:

1. `param_types` mismatch: `tfloat_temporal_max` registers
   `[LogicalType::Binary]` (see L44807-L44815) but the case passes a
   scalar `DOUBLE`. DuckDB's binder rejects with "no function matches",
   no implicit DOUBLE→BLOB cast exists.
2. Accumulator streaming guard at L45569 hard-errors on any
   `ScalarValue` other than `Binary`/`Utf8`:
   `"aggregate streaming arg must be BINARY"`. A DOUBLE stream can
   never reach the finalize arm.
3. Return-shape mismatch: the case asserts `octet_length(...) = 8` on
   the aggregate's result (an 8-byte LE f64 or i64). Existing
   `tfloat_temporal_max` returns a serialized `tfloat_sequence` blob
   (variable-length ciborium-encoded WTV frame, typically >100 bytes),
   never 8. Even if the input path worked, the assertion fails.

**Only viable route is Path (B) — new WIT surface**:

1. In `~/git/mobilitydb-wasm/crates/mdb-temporal-wasm/wit/temporal.wit`,
   add an interface (`temporal-scalar-aggregate-ops` or extend
   `temporal_aggregate_ops`) exporting:
   ```wit
   tfloat-max-agg-f64: func(values: list<f64>) -> option<f64>;
   tfloat-min-agg-f64: func(values: list<f64>) -> option<f64>;
   tfloat-count-agg-f64: func(values: list<f64>) -> u64;
   ```
   (For `stddev`/`sum`/`avg` follow the same shape.)
2. Implement the interface in the provider crates
   (`~/git/mobilitydb-wasm/crates/provider/src/dispatch/`), routing
   through `libmeos` or plain Rust f64 math.
3. In `~/git/datafission/extensions/mobilitydb/src/lib.rs`:
   - Register `tfloat_max_agg` / `_min_agg` / `_count_agg` in the
     aggregate registry at L44603 with
     `param_types: [[LogicalType::Float64]]` and
     `return_type: LogicalType::Binary`.
   - Extend the `AccState` (L44580 area, `alloc_accumulator`) with an
     `f64_sidecar: Vec<f64>` field.
   - Extend `accumulate()` at L45555 to accept `ScalarValue::Float64`
     and push into the sidecar buffer when the arm's declared
     `param_types` is `Float64`.
   - Add `finalize` arms that call the new WIT functions and encode the
     result as 8-byte LE bytes wrapped in `ScalarValue::Binary`.
4. Re-run `extract-mobilitydb-interface` so the interface DB advertises
   the new aggregates with F64 param types.
5. Regenerate `mobilitydb-ducklink-bridge` via
   `sqlink-shim-codegen --dynlink --target duckdb`. Codegen must
   dispatch aggregate accumulators by DataType (already does per case
   comment) — verify the DuckDB DataType::DOUBLE arm reads via
   `FlatVector::GetData<double>` and forwards as `ScalarValue::Float64`.
6. Rebuild + re-vendor the monolith provider, re-emit the bridge,
   commit locally (do NOT force-push tegmentum in this tranche).

**Estimated scope**: 4 repos, 2-3 days. Not tractable in the current
tranche. Design decision to defer to caller: pick Path (B) or drop the
case from the smoke corpus until a broader F64-aggregate surface lands
(the `tint_*_agg` line already runs into the same gap — the 12-entry
`tint_count_agg` / `tint_max_agg` etc. block at L44967+ also registers
Binary param_types).

## `09-bitemporal` — `_to_text` / `_from_text` alias UDFs missing

**Failure**: case calls `bitemporal_{bool,int,float,text}_{to,from}_text`.
Shim only registers `_to_ewkt` / `_from_ewkt` (see L8854-L9754 in
`src/lib.rs`). For MobilityDB bitemporal types over
{bool,int,float,text} — no geometry — text and EWKT are the same wire
format (`[value@[vs, ve)@[ts, te)]`), so aliasing is semantically safe.

**Work required** (single-repo, straightforward):

1. In `~/git/datafission/extensions/mobilitydb/src/lib.rs`, edit the 8
   `ScalarFunctionMeta` entries at L8854, L8958, L9096, L9228, L9387,
   L9505, L9650, L9754 to change
   `aliases: Vec::new()` to
   `aliases: alloc::vec!["bitemporal_<type>_to_text".to_string()]`
   (mirroring the aggregate-alias pattern at L44738 `tand`).
2. Rebuild the datafission-bridge wasm
   (`cargo build --release --target wasm32-wasip2`), re-compose the
   monolith via `compose.wac`, re-vendor into `deps/`.
3. Re-extract `mobilitydb-interface.sqlite`. Aliases populate
   `scalar_aliases` (currently empty — see
   `~/git/shim-interface-core/src/lib.rs:881`). The dynlink codegen
   already consumes `catalog.aliases` filtered by
   `kind == "scalar"` and threads them through `canonical_for` so both
   spellings reach the same shim dispatch arm
   (`~/git/datalink/crates/datalink-shim-duckdb-dynlink-emit/src/emit_dynlink.rs:337`).
4. Regenerate the ducklink bridge and force-push to tegmentum.

**No dispatch-arm change needed**: the bridge's `canonical_for`
translation rewrites the alias name to the canonical WIT method before
crossing the component boundary. Sibling aggregate aliases (`tand`,
`tor`, `tavg`, ...) are the working reference.

Follow-up cases to consider once the pattern is in place:
`bitemporal_bool_sequence_new` etc. already accept text form via
their own `_from_text`-style constructor, so audit for name overlap
before landing.

## `02-spatial-index` — SQL-callable spatial-index UDTF surface missing across BOTH extensions

**Failure** (re-confirmed 2026-07-11 investigator sweep): case does
`SET VARIABLE h = (SELECT mobilitydb_spatial_index_build(id, wkb) FROM pts);`
followed by
`SELECT q.item_id FROM mobilitydb_spatial_index_query_envelope(getvariable('h'), 0.0, 0.0, 1.0, 1.0) q;`.
No `mobilitydb_spatial_index_*` function (aggregate OR UDTF) is
registered anywhere. Empty output, no rows.

**Confirmed the sibling `postgis-duckdb-only/03-spatial-index.sql` also
fails** — same SQL shape with `postgis_spatial_index_*` names — so the
gap spans BOTH extensions.

### 2026-07-11 sharpening: the postgis Guest impl is NOT a portable reference

The postgis `spatial_index::Guest` impl at
`~/git/datafission/extensions/postgis/src/lib.rs:32476-32568` implements
only the PLANNER-visible datafission
`spatial-index-plugin/spatial-index@1.0.0` interface, which the host
invokes when it processes `CREATE INDEX … USING spatial`. It is NOT
SQL-callable. The auto-wire at
`~/git/datalink/crates/datalink-shim-datafission-emit/src/emit_lib.rs:709-723,
1235-1335` emits that Guest impl when the shim imports
`postgis:wasm/postgis-spatial-index`, and does NOT emit any aggregate or
UDTF surface. Porting the postgis Guest impl to mobilitydb (add
`import postgis:wasm/postgis-spatial-index` to mobilitydb's `world.wit`
and let the auto-wire fire) closes ONE of the three missing pieces but
does not make the smoke case pass — the SQL-callable aggregate + UDTF
are still absent from BOTH sides.

The three-piece work sketched below therefore still applies, and the
"fix postgis first" plan is unchanged in shape but blocked on the two
architectural gaps documented under **NOVEL WORK** below.

**Current state — three distinct pieces are missing**:

1. **The `spatial_index::Guest` datafission-plugin impl** — this is
   the PLANNER-visible interface, not user-callable SQL. mobilitydb
   stubs it at
   `~/git/datafission/extensions/mobilitydb/src/lib.rs:49331` with
   `SpatialError::UnsupportedOperation("spatial-index not wired in
   scalar-first cut")`. **The codegen at
   `~/git/datalink/crates/datalink-shim-datafission-emit/src/emit_lib.rs:709-723,1235-1335`
   auto-wires this impl to `pg_strtree::*` when the shim imports
   `postgis:wasm/postgis-spatial-index`** — the postgis shim (which
   does import it) gets a working impl (`~/git/datafission/extensions/postgis/src/lib.rs:32476-32568`).
   The mobilitydb shim does NOT import postgis-spatial-index, so it
   keeps the stub. Fix: add
   `use bindings::postgis::wasm::postgis_spatial_index` to
   mobilitydb's world.wit dependency graph (postgis-wasm is already
   composed alongside via the postgis+mobilitydb dynlink chain).
2. **SQL-callable `postgis_spatial_index_build(id, wkb) -> u64`
   AGGREGATE** — NOT registered anywhere. Neither the postgis shim's
   `aggregate_function_registry` (`~/git/datafission/extensions/postgis/src/lib.rs`)
   nor the codegen fleet emits it. Needs a new entry that internally
   calls `pg_strtree::create_index` on init, `insert_wkb` per row,
   `build` on finalize, and returns the handle.
3. **SQL-callable `postgis_spatial_index_query_envelope(handle: u64,
   xmin, ymin, xmax, ymax) -> table<item_id: u64>` UDTF** — NOT
   registered. Needs an entry in `table_function_registry` with a
   `return_type` schema arm projecting `{item_id: Int64}` and an
   execute arm invoking `pg_strtree::query_envelope`. The scalar
   `query_envelope` at `postgis/src/lib.rs:14831-14844` returns only
   the FIRST hit — not a full result set — so the UDTF path needs its
   own arm.

**Reference implementation to grep for**: postgis's
`spatial_index::Guest` impl at
`~/git/datafission/extensions/postgis/src/lib.rs:32476-32568` is the
working template for pieces #1 and (partially) for the pg_strtree
wiring in #2/#3. `pg_strtree::create_index` / `insert_wkb` / `build` /
`query_envelope` / `destroy_index` are the WIT primitives to route
through.

**Work required — postgis-side first** (which mobilitydb inherits via
the composed monolith):

1. In `~/git/datafission/extensions/postgis/src/lib.rs`:
   - Add `postgis_spatial_index_build` as a
     `AggregateFunctionMeta` at the `aggregate_function_registry`
     block. `param_types: [[Int64, Binary]]` (item_id + WKB blob).
     `return_type: LogicalType::Int64` (the u64 handle, cast to i64).
     Accumulator arm: on first accumulate, call
     `pg_strtree::create_index(10)`; per accumulate,
     `pg_strtree::insert_wkb(handle, wkb, item_id)`; finalize calls
     `pg_strtree::build(handle)` and returns the handle.
   - Add `postgis_spatial_index_query_envelope` as a
     `TableFunctionMeta` at `table_function_registry` (near L46991
     in the mobilitydb file, or the equivalent postgis block).
     `param_types: [[Int64, Float64, Float64, Float64, Float64]]`.
     `return_type` arm returns `[ColumnInfo { name: "item_id",
     ty: LogicalType::Int64 }]`. Execute arm calls
     `pg_strtree::query_envelope(handle, xmin, ymin, xmax, ymax)`
     and materializes each u64 as a row.
2. Repeat verbatim in
   `~/git/datafission/extensions/mobilitydb/src/lib.rs` with
   `mobilitydb_` prefix, either by:
   (a) adding `postgis_spatial_index` to mobilitydb's WIT deps and
       calling the same primitives, OR
   (b) forwarding the handle through the composed shim's
       `spatial_index::Guest` interface once #1 is un-stubbed.
   Approach (a) is straightforward once postgis is done; approach (b)
   requires wiring the planner-visible interface into a UDTF surface,
   which needs a new codegen path.
3. Re-run `extract-{postgis,mobilitydb}-interface` to surface the new
   aggregate + UDTF metadata.
4. Regenerate both `-ducklink-bridge` dynlinks and re-vendor.
5. Rebuild the composed monolith providers.

**Estimated scope**: 4 repos, ~1 day for postgis side alone, another
day for mobilitydb. Not tractable this tranche. Postgis case
`03-spatial-index` is the natural first target — same shape and
better-documented WIT surface.

### NOVEL WORK required before either side can land

Two architectural gaps make this NOT a "port the pattern" job:

**Gap 1 — the datafission aggregate accumulator is single-column.** The
current codegen at
`~/git/datalink/crates/datalink-shim-datafission-emit/src/emit_lib.rs:1764-1790`
emits `accumulate(handle, value: ScalarValue)` with a hard-coded
`ScalarValue::Binary` / `Utf8` guard — one BINARY per row. But
`postgis_spatial_index_build(id, wkb)` is a 2-column streaming
aggregate (`id: Int64`, `wkb: Binary`), not 1 streaming + N config
extras. Neither the datafission `aggregate-function-registry@1.0.0`
contract nor its bridge emit path supports multi-column streaming.
Options:
  (a) Add a `accumulate_row(handle, values: list<ScalarValue>)` method
      to `datafission:function-plugin/aggregate-function-registry` (WIT
      surface change, ripple through every emit + every host).
  (b) Encode `(id, wkb)` into a single BINARY value host-side (e.g.
      i64-length-prefix framing) and unpack inside the shim's finalize
      arm. Doesn't require a WIT change but adds a per-case wire.
      contract; the codegen would need a new shape flag like
      `accumulator_kind = "pair_id_wkb"` to route these correctly.

**Gap 2 — no session-handle registry exists on the provider side.** The
ducklink CBOR dispatch layer (`datalink-shim-duckdb-dynlink-emit`)
runs everything through per-call CBOR round-trips to the monolith
provider. To turn `spatial_index_build` into a SQL aggregate that
returns a `u64` handle usable later, the provider (either
`postgis-monolith-provider.wasm` or `mobilitydb-monolith-provider.wasm`)
would need:
  - A `HandleRegistry<u64, Arc<dyn SpatialIndex>>` living inside the
    provider component (thread-local since wasm is single-threaded).
  - A `spatial-index-build(item_ids: list<u64>, wkbs: list<list<u8>>) ->
    u64` CBOR method that builds via `pg_strtree::create_index +
    insert_wkb + build` and parks the handle.
  - A `spatial-index-query-envelope(handle: u64, minx, miny, maxx, maxy)
    -> list<u64>` CBOR method that fetches from the registry and calls
    `pg_strtree::query_envelope`.
None of this exists today: `~/git/postgis-wasm/src/spatial_index.rs`
only implements the Guest impl (`impl spatial_index::Guest`), not a
CBOR-callable session-handle build path. Grep confirms zero hits for
`store_index` / `NEXT_HANDLE` / `HandleRegistry` in either provider
tree.

There IS a working reference implementation in the ARCHIVED
`~/git/ducklink-shim-codegen/src/emit/mod.rs:2455-2650`, but that
emits a NATIVE DuckDB extension (writes directly to `libduckdb_sys`)
that consumes `datafission_index::spatial::build_spatial_index` from a
same-process crate. It doesn't work in the current wasm-composed
ducklink chain — the wac-plug monolith runs entirely in wasm with no
shared in-process registry.

**Recommended sequence** (2-3 days end-to-end, spans 5 repos):

1. `~/git/postgis-wasm`: add a session-handle registry + two CBOR
   methods (`spatial-index-build`, `spatial-index-query-envelope`) to
   the postgis provider crate. New WIT stanzas need to be added to a
   new interface `postgis:wasm/postgis-spatial-index-session` (or
   extend the existing `postgis-spatial-index` interface with the
   session ops). Rebuild + recompose the monolith provider.
2. `~/git/mobilitydb-wasm`: composed monolith already includes
   postgis-wasm — the same CBOR methods surface through the composed
   provider without a mobilitydb-side change.
3. `~/git/shim-interface-core` + `extract-*-interface`: extend the
   interface DB shape with an `aggregate_kind` column that carries
   `spatial_index_build` as a discriminator, and equivalent for UDTFs.
   Re-extract `postgis-interface.sqlite` and
   `mobilitydb-interface.sqlite`.
4. `~/git/datalink`: extend
   `datalink-shim-duckdb-dynlink-emit::emit_dynlink` to detect the new
   `aggregate_kind = "spatial_index_build"` (and matching UDTF kind)
   and emit a specialized dispatch arm that CBOR-encodes the (id, wkb)
   pair-list into the provider's `spatial-index-build` method.
5. Regenerate `postgis-ducklink-bridge` + `mobilitydb-ducklink-bridge`,
   force-push tegmentum, re-vendor into datafission.
6. Verify with the two smoke cases.

Datafission `extensions/mobilitydb/src/lib.rs:49451-49510` (Guest impl
stub) stays as-is until step 3 also wires the planner-side Guest impl
via the same import trick (add
`import postgis:wasm/postgis-spatial-index` to mobilitydb's
`world.wit`). That's a nice-to-have for a future `CREATE INDEX …
USING spatial` on `mobilitydb` catalog tables and does NOT gate the
smoke case, which is aggregate + UDTF only.

**STOPPED here** (2026-07-11 sub-agent). No commits landed. The
`spatial_index::Guest` stub at `mobilitydb/src/lib.rs:49451-49510`
remains — porting the postgis Guest impl into mobilitydb is one
commit's worth of work but doesn't move the smoke case, so it was
left untouched to avoid noise in a partial landing.

## `10-stindex` — `stindex_count_in_stbox` signature mismatch (JSON vs u32-prefixed BLOB)

**Failure**: case calls
`stindex_count_in_stbox(unhex('00000000'), stbox_make(...))` — expects
`(BLOB, STBOX) -> BIGINT`. Shim registers
`(Utf8, Binary) -> Int64` at L13674 with dispatch arm at L32629 that
parses arg0 via `parse_json_list_record_stindex_entry` — a JSON-array
decoder expecting a text of the form
`'[{"item_id":..., "envelope":...}, ...]'`, not a raw u32-prefixed
blob (4 bytes count + 65×N bytes entries).

**Root cause**: shim's WIT type for arg0 is `list<stindex-entry>`, and
the codegen surfaces every `list<record>` as JSON-text-in-Utf8 (see
`ParamShape::ListRecord` at
`~/git/datalink/crates/datalink-shim-codegen-core/src/interface_db.rs:198-223`).
Case author expected the BLOB u32-prefixed representation used
internally by the mobilitydb-strtree upstream.

**Not fixable by a codegen input-type-table tweak alone**: the entire
JSON-vs-BLOB decoding path is baked into the ListRecord shape. Either:

- **Path A (shim-side)**: add a new WIT function
  `stindex_count_in_stbox_bytes(bytes: list<u8>, bbox: stbox) -> u32`
  that parses the u32-prefixed blob format directly, then register it
  in the shim and have the case call the new name. Straightforward
  but adds a parallel surface for every stindex UDF the case-suite
  wants to exercise via BLOB.
- **Path B (codegen-side)**: extend `ParamShape` with a
  `ListRecordBinary` variant that decodes a u32-prefixed blob when the
  shim WIT annotates the list arg with a `@representation("bytes")`
  attribute or similar. Requires WIT-parser + emit change in
  `datalink-shim-codegen-core` and both `-emit` crates.
- **Path C (case-side)**: rewrite the case to pass a JSON literal
  matching the shim's current surface (e.g. `'[]'` for empty). Least
  work but bypasses the "sentinel-correctness end-to-end" intent —
  the case is deliberately exercising the u32-prefixed empty-stindex
  wire format.

Recommend Path A (add `_bytes` sibling) for smoke completeness; Path
B is the right long-term design decision but crosses more repos.

## `01-table-functions` — WIT-marshaling shape mismatch + UDTF column-naming disagreement

**Preprocess NOT the failure** (re-verified 2026-07-11): running
`shim-sql-preprocess --dialect postgres` against
`01-table-functions.sql` succeeds and emits a clean three-statement
SQL — no parse error. The KNOWN-FAILURES.md entry was stale; earlier
preprocess bugs have been closed by upstream fixes. This case does NOT
need a `.no-preprocess` marker.

**Actual failure — two distinct bugs surface on the same case**:

### Bug (a): CBOR shape mismatch on `as_of_join_float` args

Direct trace (query 1 of the case, run raw through ducklink):
```
internal error: Invalid Input Error: internal error:
as-of-join-float: as-of-join-float: arg0: expected 4-element list
for tfloat-sequence
```

Origin:
`~/git/mobilitydb-wasm/crates/provider/src/dispatch/mobilitydb_analytics.rs:2668-2711`
(`cbor_to_tfloat_sequence`). The provider's dispatch layer decodes
Requests as `CborValue` and expects the tfloat-sequence to arrive as
`CborValue::List([instants, interp, lower, upper])` — a 4-element
positional list.

But the shim at `~/git/datafission/extensions/mobilitydb/src/lib.rs:47942`
routes through `bindings::mobilitydb::temporal::join_ops::as_of_join_float(&arg0, &arg1)`
where `arg0`/`arg1` are `TfloatSequence` structs decoded via
`arg_witvalue_tfloat_sequence` (L1039). wit-bindgen marshals structs
by-field over the WIT ABI, but the analytics provider is a CBOR-based
Request/Response layer (see `dispatch(method: &str, req: &Request)` at
L193). Somewhere in the shim→provider path a struct-to-CBOR encoding
is producing a Map-shaped value (default serde derive on
`#[derive(Serialize)]`), and the provider decoder rejects it because
it expects positional List form.

**Fix candidates**:

- **Provider-side (preferred)**: teach
  `cbor_to_tfloat_sequence` to accept either shape — check for
  `CborValue::List(4)` OR `CborValue::Map` with the named fields, and
  extract accordingly. Same for `cbor_to_tint_sequence` at L2750,
  `cbor_to_ttext_sequence`, `cbor_to_tbool_sequence`, and the
  analogous instant decoders (each `expected 4-element list` /
  `expected 2-element list` site). About 8 sites in
  `provider/src/dispatch/mobilitydb_analytics.rs` to widen.
- **Shim-side**: force positional ciborium serialization via
  `#[serde(with = "serde_bytes")]` or a hand-written serialize path.
  Requires touching the WIT-generated bindings — invasive.

### Bug (b): UDTF return-schema column names

Direct trace (query 2 of the case, run raw through ducklink):
```
internal error: Binder Error: Referenced column "left_value" not
found in FROM clause!
Candidate bindings: "result"
```

Case SQL uses column names `left_value`, `right_value`. Shim
declares 3-column schema `{timestamp, left, right}` at
`~/git/datafission/extensions/mobilitydb/src/lib.rs:47327-47340`:
```rust
"as_of_join_float" => Ok(alloc::vec![
    ColumnInfo { name: "timestamp".into(), ty: LogicalType::Int64 },
    ColumnInfo { name: "left".into(),      ty: LogicalType::Float64 },
    ColumnInfo { name: "right".into(),     ty: LogicalType::Float64 },
]),
```
Additionally, the case's header comment says the schema should be
4-column `{left_timestamp, right_timestamp, left_value, right_value}`
— but MobilityDB's upstream `as_of_join_float` only carries ONE
timestamp per joined pair (the left instant), so the 3-column shape
IS correct. The mismatch is purely naming.

**Note**: The binder error says "Candidate bindings: result" — this
is a plural-name catch-all that DuckDB surfaces when the UDTF returns
a struct-typed single column rather than a wide row. Suggests the
dynlink codegen may be projecting the row as a single struct column
named `result` rather than as separate typed columns matching
`ColumnInfo`. Grep target:
`~/git/datalink/crates/datalink-shim-duckdb-dynlink-emit/src/emit_dynlink.rs`
UDTF-return-schema materialization.

**Fix candidates**:

- **Case-side** (least invasive): rename SQL references to `left`
  and `right` — matches current shim schema. But the "left_value"
  intent is clearer; consider renaming the shim.
- **Shim-side**: rename ColumnInfo entries to `left_timestamp`,
  `left_value`, `right_value` — need to keep in step with the
  `as_of_join_int` / `as_of_join_text` siblings at L47341/L47355.
- **Codegen-side**: verify the dynlink emit projects each
  `ColumnInfo` as a distinct DuckDB column rather than wrapping in a
  single struct row.

**Work sequence**:

1. Fix Bug (a) provider-side first — widen the 8 CBOR decoders in
   `provider/src/dispatch/mobilitydb_analytics.rs`. Rebuild the
   composed monolith provider and re-vendor
   `mobilitydb-monolith-provider.wasm`.
2. Rerun the case. If Query 1 (`SELECT COUNT(*)`) now returns 2, Bug
   (a) is closed and Bug (b) becomes visible.
3. Fix Bug (b): decide direction (case rename vs shim rename vs
   codegen fix) then apply. This is a DESIGN DECISION — defer to
   caller.

**Estimated scope**: 2 repos (`mobilitydb-wasm`, one of
`datafission`/`datalink`/case-suite), 4-6 hours end-to-end once the
direction is picked. Both bugs are diagnosable but not both fixable
in a bounded tranche without picking the direction for Bug (b).
