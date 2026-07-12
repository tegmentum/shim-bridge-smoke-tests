# mobilitydb-duckdb-only: known cross-repo failures

Baseline against the arm-wiring effort of 2026-07-11: `cases/mobilitydb-duckdb-only`
scores 1/13 pass. `cases/mobilitydb` (primary corpus) is at 4/4 PASS.

An investigator sub-classified the 12 failing cases into two buckets:

- **Bucket A / dispatch-arm work in `mdb_temporal` provider**: ~6-7 cases
  closed by a sibling agent adding arms in the upstream
  `~/git/mobilitydb-wasm` `mobilitydb_core.rs`.
- **Bucket B / cross-repo work spanning shim registry + bridge codegen +
  preprocess**: the 5 cases documented below.

This file scopes bucket B — all five require multi-repo work that cannot
be resolved by adding a dispatch arm alone.

## `11-udafs` — new UDAF surface required (F64-input aggregates)

**Failure**: case calls `tfloat_max_agg(v)`, `tfloat_min_agg(v)`,
`tfloat_count_agg(v)` where `v` is `DOUBLE`. Shim advertises no
`tfloat_*_agg` in either the scalar or aggregate registry — only the
windowed-scalar variants `tfloat_wmax/wmin/wcount` (which take
`(tfloat_sequence, i64) -> tfloat_sequence`, not a DOUBLE aggregate),
and the sequence-input aggregates `tfloat_temporal_max/min/count`
(which take `(BLOB tfloat_sequence)`, not a scalar DOUBLE).

**Not just an alias**: the aggregate framework in the shim
(`aggregate_function_registry`, `~/git/datafission/extensions/mobilitydb/src/lib.rs`
around L44603-L45581) streams accumulator values as
`ScalarValue::Binary` or `ScalarValue::Utf8` — see the
`accumulate()` guard at L45569 that errors on any non-Binary/Utf8 with
`"aggregate streaming arg must be BINARY"`. A DOUBLE-input aggregate
would require a new streaming-value path plus new WIT imports from the
provider — the analogous `tint_*_agg` entries at L44967+ also register
Binary param_types and dispatch through `arg_witvalue_tint_sequence`,
so the `_agg` name pattern is currently reserved for
sequence-blob-input aggregates.

**Work required** (cross-repo, non-local):

1. In `~/git/mobilitydb-wasm` (`mdb_temporal` provider), export new WIT
   functions in `temporal_aggregate_ops`:
   `tfloat_max_agg_f64(values: list<f64>) -> option<f64>` and siblings
   for min/count/stddev/sum/avg. (This is the actual functional gap —
   MobilityDB upstream ships these as F64 aggregators.)
2. In `~/git/datafission/extensions/mobilitydb/src/lib.rs`:
   - Register `tfloat_max_agg` / `tfloat_min_agg` / `tfloat_count_agg`
     / `tfloat_stddev_agg` etc. in `list_functions()` with
     `param_types: [[LogicalType::Float64]]`.
   - Add `return_type` arms returning `LogicalType::Binary` (8-byte LE
     f64 or i64 payload matching the case's `octet_length(...) = 8`
     assertion).
   - Extend the `accumulate()` streaming decoder to also accept
     `ScalarValue::Float64` — collect into an `f64` sidecar buffer in
     `AccState` (separate from the `blobs: Vec<Vec<u8>>` field, or
     encode f64 bytes into the existing blob stream).
   - Add `create_accumulator` + `finalize` arms that call the new WIT
     functions.
3. Re-run `extract-mobilitydb-interface` so the interface DB advertises
   the new aggregates with F64 param types.
4. Regenerate the `mobilitydb-ducklink-bridge` via
   `sqlink-shim-codegen --dynlink --target duckdb`. The codegen already
   dispatches aggregate accumulators by DataType (per the case-file
   comment: "bridge codegen now reads the per-aggregate input type from
   the interface DB and dispatches via DataType-based vector reads"),
   so it should route DOUBLE columns correctly once the interface DB
   surfaces the F64 shape.
5. Force-push the regenerated bridge to tegmentum.

**Blocker to fix locally**: the WIT surface change in step 1 is the
upstream MobilityDB coverage gap. Cannot land this in the shim
registry alone without a corresponding provider entry point.

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

## `02-spatial-index` — entire spatial-index surface stubbed out

**Failure**: case does
`SELECT ... FROM mobilitydb_spatial_index_query_envelope(getvariable('h'), 0.0, 0.0, 1.0, 1.0)`.
No `mobilitydb_spatial_index_*` UDTF is registered in the shim's
`table_function_registry` at L46991. Related sibling UDTFs
`spatial_index_find_in_envelope` / `_find_within_distance` /
`_nearest` are registered as ordinary TVFs but take a
`tgeompoint_sequence` binary arg, not a handle-from-build produced by
`mobilitydb_spatial_index_build`.

**More fundamental**: the shim's `spatial_index::Guest` impl at L49331
is a full stub — every method returns
`SpatialError::UnsupportedOperation("spatial-index not wired in
scalar-first cut")`. The whole spatial-index handle-lookup surface
(the "STRtree session handle" pattern the case exercises) is
deliberately deferred: `build`, `query_envelope`, `query_knn`,
`query_within_distance`, all stubbed. There is no shim registration
to alias against.

**Case also carries `02-spatial-index.no-preprocess`** — smoke runner
skips shim-sql-preprocess for this case, so the preprocess parser
bug (below) doesn't affect it. `SET VARIABLE h = ...` and
`getvariable('h')` are DuckDB-native and reach the ducklink CLI raw.

**Work required** (cross-repo, substantial — matches postgis's
spatial-index scope from earlier tranches):

1. In `~/git/mobilitydb-wasm` `mdb_temporal` provider, add a WIT UDTF
   `mobilitydb_spatial_index_query_envelope(handle: u64, xmin: f64,
   ymin: f64, xmax: f64, ymax: f64) -> list<u64>` (or list<record>
   matching the case's `q.item_id` column projection). Also lift the
   spatial-index build/destroy to a real STRtree-backed
   implementation.
2. In `~/git/datafission/extensions/mobilitydb/src/lib.rs`, replace
   the `spatial_index::Guest` stub impl at L49331 with a real handle
   registry backed by the provider WIT calls (the postgis extension
   has a working reference in
   `~/git/datafission/extensions/postgis/src/lib.rs`).
3. Add TableFunctionMeta entries for the handle-taking UDTFs
   (`mobilitydb_spatial_index_query_envelope` +
   `mobilitydb_spatial_index_query_knn` etc.) to
   `table_function_registry::list_functions()` at L46992, plus
   `return_type` schema arms and `execute` dispatch arms wrapping the
   spatial_index handle path.
4. Rebuild + re-vendor + re-extract + re-emit-bridge + force-push,
   same chain as #09.

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

## `01-table-functions` — shim-sql-preprocess parse error + missing UDTF wiring

**Failure**: bridge invokes `shim-sql-preprocess` at query time and it
returns: `preprocess: parse failed: sql parser error: Expected: equals
sign or TO, found: h at Line: 18, Column: 14`. The case's line 18 col
14 (the trailing `LIMIT 1;`) is not the problem — the error is on the
POST-textual-rewrite SQL that
`~/git/shim-sql-preprocess/src/lib.rs:process()` (L123) hands to
`Parser::parse_sql`. The pre-pass at L157 (`text_pre_pass`) rewrites
custom operator tokens; for MobilityDB the pass may mis-identify a
token inside the `[1.0@2000-01-01 00:00:00, ...]` EWKT literal payload
the case passes to `tfloat_from_ewkt`, producing SQL the sqlparser
dialect can't tokenise. `"found: h"` — the letter `h` at col 14 —
plausibly maps to an `hour` qualifier inside an `INTERVAL`-shaped
token synthesized by the preprocessor, or to `h` in a timestamp
fragment like `01:00:00` after some operator-token rewrite shifted
alignment.

**Verified: preprocess is not the only failure mode**. Adding a
sibling `01-table-functions.no-preprocess` marker to skip
preprocessing was tried locally; the case still fails with **empty
actual output** (no rows returned) rather than the preprocess-parse
error. So `as_of_join_float` UDTF, which IS registered in the shim
(L47000) and has an execute-side dispatch arm (L47941), is not
returning rows through the ducklink pipeline. Root cause of the empty
result set is separate from the preprocess parse error.

**Work required (two independent tracks)**:

1. Preprocess parse (`~/git/shim-sql-preprocess`):
   - Add a `--dump-preprocessed` mode to `src/main.rs` (or a test
     harness) that emits the string handed to `Parser::parse_sql` for
     `01-table-functions.sql`.
   - Bisect against the pre-pass operator patterns to find the
     offending rewrite. Likely candidates: any pattern whose LHS
     regex extends into an EWKT string literal, or a token rewrite
     triggered by a character inside `'...'` that the string-aware
     scanner missed.
   - Fix by tightening the string-aware skip in
     `text_rewrite_operators` (`src/lib.rs` around L157-L162).

2. `as_of_join_float` UDTF result-set delivery: with preprocessing
   skipped, ducklink still returns 0 rows for a case that expects 3.
   Verify by running the case through ducklink manually with the
   `.mode csv` header intact — check for a UDTF-registration error or
   an empty schema mismatch. Likely candidates:
   - Bridge codegen not registering `as_of_join_float` as a TVF in
     the ducklink extension (UDTF dispatch is a newer path than
     scalars; check
     `~/git/datalink/crates/datalink-shim-duckdb-dynlink-emit/src/emit_dynlink.rs`
     TVF emission).
   - Return-schema mismatch (`return_type` at L47327 returns a list
     of 4 typed columns; DuckDB binder expects matching column
     projection).

**Blocker note**: both tracks require inspection of runtime output
that this reviewer could not reproduce with confidence in a
non-interactive session. Preprocess dumper first — it's the simplest
lever.
