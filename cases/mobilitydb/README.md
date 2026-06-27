# cases/mobilitydb

End-to-end smoke cases for the **mobilitydb wasm bridge** loaded
through `sqlink-loader.dylib`. Each `<name>.sql` runs against the
brew sqlite3 CLI with two extensions chained (postgis FIRST for the
GEOMETRY type, then mobilitydb), and the output diffs against
`<name>.expected`.

Verified 2026-06-27 on:

- SQLite v3.53.2 (brew)
- sqlink-loader @ sqlink main + feat/mobilitydb-cases
- postgis-sqlink-loadable.wasm @ postgis-sqlink-bridge main
- mobilitydb-sqlink-loadable.wasm @ mobilitydb-sqlink-bridge main (post-W4b)

`make mobilitydb-sqlite` → **4/4 pass**.

## Cases

| File                        | Surface exercised                          |
|-----------------------------|--------------------------------------------|
| `01-spatial-join.sql`       | Primitive-in/out scalars: `distance`, `bearing`, `angular_diff` |
| `02-time-split.sql`         | W2 Phase 1 list-of-primitive marshaling via JSON-as-TEXT (`dateset_*`, `intset_*`) |
| `03-type-roundtrip.sql`     | Cross-type primitive marshaling (f64, s32, text) via JSON-list helpers |
| `04-wit-value-roundtrip.sql`| W2 Phase 2 list-of-RECORD marshaling via JSON-as-TEXT (`date_spanset_contains`, `float_spanset_contains`, `intspanset_contains`) |

## Substrate notes (why the W5 spec'd cases were narrowed)

PLAN-shim-tooling-residue.md W5 asked for these four cases:

1. `temporal_join_float` UDTF spatial-join smoke
2. `tfloat_time_split` UDTF time-split smoke
3. `tfloat_min_value` wit-value-shaped scalar round-trip
4. `kdtree_xy_within` UDTF wit-value roundtrip via JSON-as-TEXT

The first three hit substrate gaps the W5 task is not chartered to
fix; the cases are **narrowed** to substrate-equivalent surface that
exercises the same codegen/dispatch substrate without depending on
work blocked elsewhere:

### Gap A — SQLite Blob → WitValue lift missing

The wit-value SQL surface (`arg_witvalue_<record>` helpers) requires
`SqlValue::WitValue` on the dispatch boundary, but the sqlink-loader's
`read_value` (sqlink-loader/src/value.rs) maps a sqlite3_value of
SQLITE_BLOB to `SqlValue::Blob` — there is no Blob-to-WitValue
recovery against the per-extension TypedValueRegistry. So a wit-value
returned by one scalar (e.g. `intspan_from_text(...)` → `WitValue` →
BLOB in the column) cannot be passed as the input to the NEXT scalar
that requires `arg_witvalue_int_span` — the recipient sees a `Blob`
and rejects it with "must be WIT-VALUE".

This blocks every chained wit-value SQL pattern of the form
`<wit-value-out>(<wit-value-out>(...))` — which is the entire
`intspan_lower(intspan_from_text(...))` family. The 11 cases in
`cases/mobilitydb-duckdb-only/` (relocated from this directory) use
this pattern and all fail today on the sqlite target. They are kept
for the duckdb target where the dispatch substrate predates the
wit-value system.

The spec'd case 3 (`tfloat_min_value`) is a single wit-value scalar
— there is no SQL-callable constructor for `tfloat-sequence` that
returns a wit-value (every constructor is itself stubbed at the
dispatch level: `tfloat_from_csv` / `tfloat_from_ewkt` /
`tfloat_from_mfjson` are registered in the manifest but have no
dispatch arms). The verify subcrate exercises this scalar end-to-end
by constructing the wit-value host-side; smoke cases can't.

### Gap B — sqlink-loader vtab wiring deferred

`sqlink-loader/src/load.rs:218` notes: "Collations / vtabs / hooks:
not in this iteration. Surface the count so the env-var dispatcher
can log a hint." So even though the bridges register vtabs in their
manifest (postgis: 12, mobilitydb: 27+), the sqlink-loader doesn't
call `sqlite3_create_module_v2` on them. The
`cases/postgis-sqlite-only/05-udtfs` case fails with `no such table:
st_dumppoints` for the same reason.

This blocks the spec'd UDTF cases (1, 2, 4): `temporal_join_float`,
`tfloat_time_split`, and `kdtree_xy_within`. The kdtree end-to-end
substrate IS proven in the mobilitydb-sqlink-bridge verify subcrate,
which drives the vtab dispatch directly through the sqlink-host API.

### What case 4 DOES exercise

Case 4 is honest to the SPIRIT of the spec'd "wit-value roundtrip via
JSON-as-TEXT" — it uses the W2 Phase 2 (#553)
`parse_json_list_record_<X>` codec on SCALARS that take a
`list<record>` parameter:

- `date_spanset_contains(json: list<date-span>, value: s32) -> bool`
- `float_spanset_contains(json: list<float-span>, value: f64) -> bool`
- `intspanset_contains(json: list<int-span>, value: s32) -> bool`
- `date_spanset_num_spans(json: list<date-span>) -> i64`

The codepath is: SQL TEXT → serde_json → UPSTREAM record vec → call
into mobilitydb upstream spans-ops → primitive return. This is the
SAME serde-derived UPSTREAM-record decoding that the kdtree UDTF
uses; the only difference is the dispatch shape (scalar vs vtab).
JSON keys MUST be snake-cased (`lower_inc`, `upper_inc`) because
wit-bindgen kebab→snake-cases WIT record fields and serde_json
matches against the generated Rust struct names.

## Adding a case

```
cases/mobilitydb/<NN>-<topic>.sql      -- one or more SELECTs
cases/mobilitydb/<NN>-<topic>.expected -- one line of integer output
                                          per SELECT
```

Each query should return an integer (`CASE WHEN <predicate> THEN 1
ELSE 0 END`) so the output is portable across SQLite `.mode list`
and DuckDB `.mode csv`. The runner normalises trailing whitespace +
trailing empty lines.

Stay on the surfaces that DON'T hit Gap A / Gap B above:

- **Primitive scalars** (`arg_f64` / `arg_i64` / `arg_text` →
  `Ok(SqlValue::Real/Integer/Text)`). 68 such arms are wired in
  `mobilitydb-sqlink-bridge/src/lib.rs`.
- **JSON-list-of-primitive scalars**
  (`parse_json_list_<T>(args, idx, name)` — W2 Phase 1, #542). Any
  scalar taking a `list<f64>` / `list<s32>` / `list<bool>` etc.
  reaches the dispatch via TEXT.
- **JSON-list-of-record scalars** (`parse_json_list_record_<X>` —
  W2 Phase 2, #553). Span-set / time-set scalars with `list<X>`
  record params take JSON arrays of records with snake_case fields.

Anything that returns `ret_to_witvalue_<X>` and feeds INTO another
`arg_witvalue_<X>` will trip Gap A — keep it to one wit-value-shaped
call per query.

Anything routed through a vtab (FROM `<udtf>(...)`) will trip Gap B
— defer until sqlink-loader's vtab path lands.
