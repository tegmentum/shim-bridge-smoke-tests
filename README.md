<p align="center">
  <a href="https://github.com/tegmentum/sqlink"><img src="https://raw.githubusercontent.com/tegmentum/sqlink/main/sqlink_logo.png" alt="SQLink" width="320"></a>
</p>

# shim-bridge-smoke-tests

End-to-end smoke test suite for DataFission shim bridges.
Catches regressions during bridge-codegen development by
running query suites through `sqlite3`, `duckdb`, and
`ducklink` (wasm-component DuckDB host) against generated
bridges.

## What it tests

For each (target, bridge artifact, composed shim wasm) tuple,
the runner:

1. Loads the bridge into the target's CLI.
2. Sets the `<EXT>_SHIM_WASM` env var so the bridge can find
   its composed wasm.
3. Runs each `<case>.sql` file in the case directory.
4. Diffs the actual output against `<case>.expected`.
5. Reports per-case PASS/FAIL.

## Usage

```sh
# SQLite via sqlink (wasm-component host; the current default path)
scripts/run.sh sqlite \
    /path/to/postgis-sqlink-loadable.wasm \
    /path/to/postgis-shim-composed.wasm \
    cases/postgis

# DuckDB via ducklink (wasm-component host)
DUCKLINK=~/git/ducklink/target/release/ducklink scripts/run.sh ducklink \
    /path/to/postgis-ducklink-loadable.wasm \
    /path/to/postgis-shim-composed.wasm \
    cases/postgis

# DuckDB via native cdylib (legacy path; the -duckdb-bridge repos
# are archived — kept runnable for regression comparison only)
DUCKDB=/opt/homebrew/bin/duckdb scripts/run.sh duckdb \
    /path/to/postgis_duckdb_bridge.duckdb_extension \
    /path/to/postgis-shim-composed.wasm \
    cases/postgis
```

### Ducklink runtime prerequisites

The `ducklink` target requires the ducklink host binary AND
its two wasm components:

```sh
cd ~/git/ducklink
cargo build --release -p ducklink-host --bin ducklink
# Also builds ~/git/ducklink/target/wasm32-wasip2/release/{ducklink_core,ducklink_cli}.wasm
# via the same crate's wasm workspace members.
```

The runner copies the composed loadable to a scratch
`--extensions-dir` under its canonical extension name
(`<ext>.wasm` — derived from the `-ducklink-loadable.wasm`
suffix) and issues `LOAD <ext>;` inside the guest DuckDB CLI.
No `<EXT>_SHIM_WASM` env var is needed — the shim wasm is
already inlined into the composed loadable via `wac plug`.

## Case design

`cases/postgis/` holds **portable cases** that run on both
SQLite and DuckDB. Every query returns an integer (typically
0 or 1 from a `CASE WHEN <predicate> THEN 1 ELSE 0 END`)
because SQL formatting differs across targets:

| | SQLite `.mode list` | DuckDB `.mode csv` |
|---|---|---|
| Boolean | `1` / `0` | `true` / `false` |
| String with commas | `LINESTRING(0 0,1 1)` | `"LINESTRING(0 0,1 1)"` |
| NULL | empty | `NULL` |
| Integer | `1` | `1` |

Integers are the lowest-common-denominator output that's
identical everywhere. Wrapping boolean and string predicates
in `CASE WHEN ... THEN 1 ELSE 0 END` keeps the cases portable.

For features that only one target supports (e.g. UDTFs which
sqlink ships but ducklink scaffolds), cases live under
`cases/<shim>-<target>-only/`. Run with the matching target.

## Adding a new case

```sh
# cases/postgis/06-clusters.sql
SELECT CASE WHEN ST_NumGeometries(
    ST_ClusterIntersecting(<set of geoms>)
) = 2 THEN 1 ELSE 0 END;

# cases/postgis/06-clusters.expected
1
```

The runner normalises trailing whitespace on each line and
trailing empty lines, so `.expected` files are forgiving to
hand-edit.

## Status

Verified 2026-06-24 on:

- SQLite v3.53.1 (brew) — 5/5 cases pass
- DuckDB v1.5.2 — 4/4 portable cases pass (UDTF cases are
  sqlink-only)

Adding new shims is a matter of writing one more case
directory. The runner is shim-agnostic; it just needs to know
the env var name for the shim wasm path (currently hardcoded
to `POSTGIS_SHIM_WASM` — see `scripts/run.sh` for the
extension point).
