#!/usr/bin/env bash
# Bridge smoke runner.
#
# For a given shim composed wasm + a per-target bridge dylib,
# loads the bridge into SQLite (or DuckDB) and runs a query
# suite asserting expected outputs. Used to catch regressions
# during bridge-codegen development.
#
# Usage:
#   scripts/run.sh sqlite  <bridge.dylib>           <shim.wasm>  <case-dir>
#   scripts/run.sh duckdb  <bridge.duckdb_extension> <shim.wasm>  <case-dir>
#
# Each <case-dir>/<name>.sql / <name>.expected file pair is one
# test case. The script runs each .sql via the target's CLI,
# strips trailing whitespace, and diffs against .expected.
# Exit non-zero on any mismatch.

set -euo pipefail

target="$1"
bridge_path="$2"
shim_path="$3"
case_dir="$4"

# Per-target CLI selection.
case "$target" in
    sqlite)
        # macOS system sqlite3 has -DSQLITE_OMIT_LOAD_EXTENSION;
        # use brew's. Override with $SQLITE3 if needed.
        cli="${SQLITE3:-/opt/homebrew/opt/sqlite/bin/sqlite3}"
        if [[ ! -x "$cli" ]]; then
            echo "ERROR: sqlite3 not found at $cli" >&2
            echo "       Install via 'brew install sqlite' or set SQLITE3=/path/to/sqlite3" >&2
            exit 2
        fi
        # SQLite needs the env var to find the shim; .load takes
        # the bare path (no quotes-in-path support, so resolve to
        # absolute first).
        bridge_abs="$(cd "$(dirname "$bridge_path")" && pwd)/$(basename "$bridge_path")"
        loader=".load $bridge_abs"
        ;;
    duckdb)
        cli="${DUCKDB:-duckdb}"
        if ! command -v "$cli" >/dev/null 2>&1; then
            echo "ERROR: duckdb CLI not found (set DUCKDB=/path/to/duckdb)" >&2
            exit 2
        fi
        bridge_abs="$(cd "$(dirname "$bridge_path")" && pwd)/$(basename "$bridge_path")"
        loader="LOAD '$bridge_abs';"
        cli_extra="-unsigned"  # we don't sign the extension
        ;;
    *)
        echo "ERROR: unknown target '$target' (want sqlite|duckdb)" >&2
        exit 2
        ;;
esac

# Shim path also resolved to absolute so it doesn't depend on
# the runner's cwd. The bridge reads it from the per-shim env
# var. The 5th argument names the var; if absent, infer from
# the case-dir basename (postgis* -> POSTGIS_SHIM_WASM,
# mobilitydb* -> MOBILITYDB_SHIM_WASM, etc.).
shim_abs="$(cd "$(dirname "$shim_path")" && pwd)/$(basename "$shim_path")"
if [[ -n "${5:-}" ]]; then
    shim_env="$5"
else
    base="$(basename "$case_dir")"
    shim_env="$(printf '%s' "${base%%-*}" | tr '[:lower:]' '[:upper:]')_SHIM_WASM"
fi
export "$shim_env=$shim_abs"

# Find all .sql / .expected pairs in case_dir.
pass=0
fail=0
declare -a failed_names=()
for sql in "$case_dir"/*.sql; do
    [[ -e "$sql" ]] || continue
    name="$(basename "$sql" .sql)"
    expected="$case_dir/$name.expected"
    if [[ ! -f "$expected" ]]; then
        echo "  SKIP $name  (no .expected file)"
        continue
    fi

    # Prepend the loader to the SQL so the user's case files
    # don't have to know the load incantation.
    sql_with_loader="$(mktemp -t bridge-smoke.XXXXXX.sql)"
    {
        printf '%s\n' "$loader"
        # SQLite needs `.mode list` + `.headers off` to produce
        # canonical pipe-delimited output; DuckDB needs explicit
        # ".mode csv".
        case "$target" in
            sqlite)
                printf '.mode list\n.headers off\n.separator |\n'
                ;;
            duckdb)
                printf '.mode csv\n.headers off\n'
                ;;
        esac
        cat "$sql"
    } > "$sql_with_loader"

    actual="$(mktemp -t bridge-smoke.XXXXXX.actual)"
    if [[ "$target" == "duckdb" ]]; then
        # shellcheck disable=SC2086
        "$cli" $cli_extra :memory: < "$sql_with_loader" \
            > "$actual" 2>&1 || true
    else
        "$cli" :memory: < "$sql_with_loader" \
            > "$actual" 2>&1 || true
    fi

    # Strip trailing whitespace per line + trailing empty lines
    # so .expected files can be hand-edited without breaking
    # the comparison. Also drop bridge-emitted load-time noise
    # (clash warnings, debug eprintln) that bleeds onto stdout
    # via the CLI's stderr merge — `[shim-*]` is the convention
    # the codegens use; `<crate>-duckdb-bridge:` is the legacy
    # prefix from aggregates_rs. Tests assert on actual query
    # output, not bridge chatter.
    norm_actual="$(mktemp -t bridge-smoke.XXXXXX.norm)"
    sed -e 's/[[:space:]]*$//' \
        -e '/^\[shim-/d' \
        -e '/-duckdb-bridge: /d' \
        -e '/-sqlite-bridge: /d' \
        "$actual" \
        | awk 'NR==1 || /./{print prev} {prev=$0} END{if(prev!="") print prev}' \
        > "$norm_actual"
    norm_expected="$(mktemp -t bridge-smoke.XXXXXX.norm)"
    sed -e 's/[[:space:]]*$//' "$expected" \
        | awk 'NR==1 || /./{print prev} {prev=$0} END{if(prev!="") print prev}' \
        > "$norm_expected"

    if diff -q "$norm_actual" "$norm_expected" >/dev/null 2>&1; then
        echo "  PASS $name"
        pass=$((pass+1))
    else
        echo "  FAIL $name"
        echo "    expected:"
        sed 's/^/      /' "$norm_expected"
        echo "    actual:"
        sed 's/^/      /' "$norm_actual"
        fail=$((fail+1))
        failed_names+=("$name")
    fi

    rm -f "$sql_with_loader" "$actual" "$norm_actual" "$norm_expected"
done

echo ""
echo "----"
echo "  pass=$pass fail=$fail"
if [[ $fail -gt 0 ]]; then
    echo "  failed: ${failed_names[*]}"
    exit 1
fi
