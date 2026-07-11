#!/usr/bin/env bash
# Bridge smoke runner.
#
# For a given shim composed wasm + a per-target bridge dylib,
# loads the bridge into SQLite (or DuckDB) and runs a query
# suite asserting expected outputs. Used to catch regressions
# during bridge-codegen development.
#
# Usage:
#   scripts/run.sh sqlite    <bridge.dylib|.wasm>       <shim.wasm>  <case-dir>
#   scripts/run.sh duckdb    <bridge.duckdb_extension>  <shim.wasm>  <case-dir>
#   scripts/run.sh ducklink  <composed-loadable.wasm>   <shim.wasm>  <case-dir>
#
# `ducklink` target: bridge_path is a WAC-plug'd composed
# loadable (bridge + shim inlined). Shim path is ignored (the
# shim is already inside the loadable) but kept in the argv
# shape for parity with sqlite/duckdb.
#
# A colon-separated bridge_path loads multiple extensions in
# order — required for mobilitydb (postgis's GEOMETRY type must
# register first, D5 load-order convention).
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
        # absolute first). A colon-separated bridge_path loads
        # multiple bridges in order — required for mobilitydb,
        # which depends on postgis's GEOMETRY type having loaded
        # first (D5 load-order convention).
        if [[ "$bridge_path" == *.wasm* ]]; then
            if [[ -z "${SQLINK_LOADER:-}" ]]; then
                # The bespoke loader crate (`sqlink-loader`) was retired
                # by sqlink #220. Its replacement is `sqlink-extension`
                # (libsqlink_extension.dylib) which loads via
                # `sqlite3_sqlinkloader_init` and exposes the
                # `sqlink_load_ext()` SQL function. Fall back to it if
                # the legacy path doesn't exist.
                if [[ -f "$HOME/git/sqlink/target/release/libsqlink_loader.dylib" ]]; then
                    SQLINK_LOADER="$HOME/git/sqlink/target/release/libsqlink_loader.dylib"
                elif [[ -f "$HOME/git/sqlink/target/release/libsqlink_extension.dylib" ]]; then
                    SQLINK_LOADER="$HOME/git/sqlink/target/release/libsqlink_extension.dylib"
                fi
            fi
            if [[ ! -f "$SQLINK_LOADER" ]]; then
                echo "ERROR: SQLINK_LOADER=$SQLINK_LOADER not found" >&2
                echo "       Build with: cd ~/git/sqlink && cargo build --release -p sqlink-loader" >&2
                exit 2
            fi
            # sqlite3 auto-derives the init entry from the filename
            # (`libsqlink_extension.dylib` → `sqlite3_sqlinkextension_init`),
            # but sqlink-extension actually exports the entry as
            # `sqlite3_sqlinkloader_init` — kept for backward-compat with
            # the retired sqlink-loader crate. Pass the explicit entry so
            # sqlite3 doesn't dlsym the wrong name.
            case "$(basename "$SQLINK_LOADER")" in
                libsqlink_extension.*)
                    loader=".load $SQLINK_LOADER sqlite3_sqlinkloader_init"
                    ;;
                *)
                    loader=".load $SQLINK_LOADER"
                    ;;
            esac
            IFS=':' read -r -a bridge_paths <<< "$bridge_path"
            for bp in "${bridge_paths[@]}"; do
                bp_abs="$(cd "$(dirname "$bp")" && pwd)/$(basename "$bp")"
                # Derive the extension name from the wasm filename,
                # stripping the canonical `-sqlink-loadable.wasm`
                # suffix. Falls back to the basename minus the
                # `.wasm` extension. For chained loads (postgis +
                # mobilitydb) each load uses its own name so the
                # bridge registers under the right identity.
                bp_base="$(basename "$bp")"
                case "$bp_base" in
                    *-sqlink-loadable.wasm)
                        ext_name="${bp_base%-sqlink-loadable.wasm}"
                        ;;
                    *.wasm)
                        ext_name="${bp_base%.wasm}"
                        ;;
                    *)
                        # Last-resort: case-dir basename, first `-`-segment.
                        base="$(basename "$case_dir")"
                        ext_name="${base%%-*}"
                        ;;
                esac
                loader+=$'\n'"SELECT sqlink_load_ext('$ext_name', '$bp_abs');"
            done
            # Last bridge is the "primary" for diagnostic context.
            bridge_abs="$(cd "$(dirname "${bridge_paths[-1]}")" && pwd)/$(basename "${bridge_paths[-1]}")"
        else
            bridge_abs="$(cd "$(dirname "$bridge_path")" && pwd)/$(basename "$bridge_path")"
            loader=".load $bridge_abs"
        fi
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
    ducklink)
        # ducklink is a wasm-component DuckDB host that embeds
        # wasmtime. Extensions are wasm components resolved by
        # NAME from --extensions-dir/<name>.wasm (see
        # ducklink-host `resolve_provider_artifact` /
        # `extension_artifact_path`). The bridge_path arg here
        # is a WAC-plug'd composed loadable (bridge + shim
        # inlined) — copy it into a scratch dir with the
        # canonical extension name and let ducklink resolve.
        cli="${DUCKLINK:-$HOME/git/ducklink/target/release/ducklink}"
        if [[ ! -x "$cli" ]]; then
            echo "ERROR: ducklink binary not found at $cli" >&2
            echo "       Build with: cd ~/git/ducklink && cargo build --release -p ducklink-host --bin ducklink" >&2
            exit 2
        fi
        # Scratch extensions-dir populated from the colon-separated
        # bridge_path. Each entry's basename is stripped of the
        # canonical `-ducklink-loadable.wasm` suffix to derive the
        # extension NAME the guest LOAD statement uses. Chained
        # loads (postgis + mobilitydb) each land under their own
        # identity in this dir.
        scratch_ext_dir="$(mktemp -d -t ducklink-ext.XXXXXX)"
        # Ensure cleanup even if the script exits mid-loop.
        trap 'rm -rf "$scratch_ext_dir"' EXIT
        loader=""
        # Phase 9.3 dynlink env-var wiring: bridges with `_bridge_dynlink.wasm`
        # in the filename are compose:dynlink guests that resolve their
        # provider through the host's ProviderRegistry via the
        # DUCKLINK_SUB_EXT_* env vars, not by being present at the scratch
        # `extensions/$ext_name.wasm` path. Collect them here and export
        # once at the end so subsequent bridges in the colon chain layer
        # cleanly. Legacy `*-ducklink-loadable.wasm` bridges keep the
        # copy-into-scratch behavior.
        dynlink_bridges=""
        dynlink_prebuilts=""
        IFS=':' read -r -a bridge_paths <<< "$bridge_path"
        for bp in "${bridge_paths[@]}"; do
            bp_abs="$(cd "$(dirname "$bp")" && pwd)/$(basename "$bp")"
            bp_base="$(basename "$bp")"
            case "$bp_base" in
                *_bridge_dynlink.wasm)
                    # Strip `_{sqlite,duckdb}_bridge_dynlink.wasm` — both variants
                    # collapse onto the same ext_name (`postgis`, `postgis_core`).
                    ext_name="${bp_base%_sqlite_bridge_dynlink.wasm}"
                    ext_name="${ext_name%_duckdb_bridge_dynlink.wasm}"
                    ext_name="${ext_name%_bridge_dynlink.wasm}"
                    if [[ -n "$dynlink_bridges" ]]; then
                        dynlink_bridges+=":"
                        dynlink_prebuilts+=":"
                    fi
                    # Resolve shim to absolute path for the env var
                    # (script resolves shim_abs later; do the same
                    # transformation here inline for the dynlink map).
                    _shim_abs="$(cd "$(dirname "$shim_path")" && pwd)/$(basename "$shim_path")"
                    dynlink_bridges+="${ext_name}=${bp_abs}"
                    dynlink_prebuilts+="${ext_name}=${_shim_abs}"
                    ;;
                *-ducklink-loadable.wasm)
                    ext_name="${bp_base%-ducklink-loadable.wasm}"
                    cp "$bp_abs" "$scratch_ext_dir/$ext_name.wasm"
                    ;;
                *.wasm)
                    ext_name="${bp_base%.wasm}"
                    cp "$bp_abs" "$scratch_ext_dir/$ext_name.wasm"
                    ;;
                *)
                    base="$(basename "$case_dir")"
                    ext_name="${base%%-*}"
                    cp "$bp_abs" "$scratch_ext_dir/$ext_name.wasm"
                    ;;
            esac
            loader+="LOAD $ext_name;"$'\n'
        done
        # `LOAD $ext_name` (bare identifier) triggers
        # extension_artifact_path("$ext_name") -> "$dir/$ext_name.wasm" for
        # legacy loadables, OR the sub-ext env-var branch for dynlink bridges.
        # Silence the default `jsonfns` autoload so it doesn't
        # bleed into the query output.
        export DUCKLINK_AUTOLOAD=""
        if [[ -n "$dynlink_bridges" ]]; then
            export DUCKLINK_SUB_EXT_BRIDGES="$dynlink_bridges"
            export DUCKLINK_SUB_EXT_PREBUILT="$dynlink_prebuilts"
        fi
        # ducklink invokes an internal duckdb-cli via wasm; the
        # `--` separates host args from guest-CLI args. `:memory:`
        # + stdin-piped SQL mirrors how sqlite/duckdb targets run.
        bridge_abs="$(cd "$(dirname "${bridge_paths[-1]}")" && pwd)/$(basename "${bridge_paths[-1]}")"
        ;;
    *)
        echo "ERROR: unknown target '$target' (want sqlite|duckdb|ducklink)" >&2
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
    # Preprocess SQL through shim-sql-preprocess when:
    #   - SHIM_SQL_PREPROCESS env var points at the binary, AND
    #   - SHIM_INTERFACE_DB env var points at the matching
    #     interface DB, AND
    #   - the case file doesn't have a sibling `<case>.no-preprocess`
    #     marker. Cases that don't need rewriting (i.e. the
    #     existing 01..04 postgis cases) opt out via the marker.
    if [[ -n "${SHIM_SQL_PREPROCESS:-}" \
        && -n "${SHIM_INTERFACE_DB:-}" \
        && ! -f "$case_dir/$name.no-preprocess" ]]
    then
        # Per-target dialect. sqlparser-rs's DuckDb dialect
        # rejects some PG-style infix operators (`&&` etc.);
        # postgres dialect handles them and the rewrite is
        # dialect-neutral on the output.
        case "$target" in
            duckdb)  pp_dialect="postgres" ;;
            sqlite)  pp_dialect="sqlite" ;;
            *)       pp_dialect="generic" ;;
        esac
        rewritten="$(mktemp -t bridge-smoke.XXXXXX.rewritten.sql)"
        if ! "$SHIM_SQL_PREPROCESS" \
                --interface "$SHIM_INTERFACE_DB" \
                --dialect "$pp_dialect" \
                < "$sql" > "$rewritten" 2> "$rewritten.err"
        then
            echo "  FAIL $name  (preprocess error)"
            sed 's/^/    /' "$rewritten.err"
            rm -f "$rewritten" "$rewritten.err"
            fail=$((fail+1))
            failed_names+=("$name")
            continue
        fi
        # Normalize preprocessor output: sqlparser reunparses the
        # AST as a single line of `stmt; stmt; stmt` with no
        # trailing `;` and no newlines. The ducklink guest CLI
        # dispatches one line at a time (a line with embedded `;`
        # becomes ONE statement handed to duckdb::execute, and a
        # line without a trailing `;` never dispatches at all).
        # Split on `;` boundaries onto separate lines, drop empty
        # lines, and ensure a trailing `;` so the final statement
        # always fires. sqlite/duckdb CLIs tolerate this format.
        rewritten_norm="$(mktemp -t bridge-smoke.XXXXXX.norm.sql)"
        sed -e 's/;[[:space:]]*/;\n/g' "$rewritten" \
            | awk 'NF{ if (!/;[[:space:]]*$/) $0=$0";"; print }' \
            > "$rewritten_norm"
        mv "$rewritten_norm" "$rewritten"
        sql_input="$rewritten"
        rm -f "$rewritten.err"
    else
        sql_input="$sql"
    fi

    {
        # SQLite needs `.mode list` + `.headers off` to produce
        # canonical pipe-delimited output; DuckDB (native) needs
        # ".mode csv" + ".headers off". The ducklink guest CLI
        # (WASI duckdb-cli) supports ".mode {table|csv|json}"
        # only — no ".headers", so we emit ".mode csv" alone and
        # strip the header/prompt noise from the output below.
        # `.mode` is emitted BEFORE the LOAD statements so the
        # LOAD's "Success" reply is captured in the same output
        # format, keeping the per-target filter set uniform.
        case "$target" in
            sqlite)
                printf '.mode list\n.headers off\n.separator |\n'
                ;;
            duckdb)
                printf '.mode csv\n.headers off\n'
                ;;
            ducklink)
                printf '.mode csv\n'
                ;;
        esac
        printf '%s\n' "$loader"
        cat "$sql_input"
    } > "$sql_with_loader"

    actual="$(mktemp -t bridge-smoke.XXXXXX.actual)"
    case "$target" in
        duckdb)
            # shellcheck disable=SC2086
            "$cli" $cli_extra :memory: < "$sql_with_loader" \
                > "$actual" 2>&1 || true
            ;;
        ducklink)
            # ducklink guest-CLI reads stdin the same as native
            # duckdb-cli; `--` splits host flags from guest args.
            "$cli" --extensions-dir "$scratch_ext_dir" -- \
                duckdb-cli :memory: < "$sql_with_loader" \
                > "$actual" 2>&1 || true
            ;;
        *)
            "$cli" :memory: < "$sql_with_loader" \
                > "$actual" 2>&1 || true
            ;;
    esac

    # Strip trailing whitespace per line + trailing empty lines
    # so .expected files can be hand-edited without breaking
    # the comparison. Also drop bridge-emitted load-time noise
    # (clash warnings, debug eprintln) that bleeds onto stdout
    # via the CLI's stderr merge — `[shim-*]` is the convention
    # the codegens use; `<crate>-duckdb-bridge:` is the legacy
    # prefix from aggregates_rs. For ducklink we additionally
    # strip:
    #   - `D> ` / `...> ` REPL prompts (written to stdout even
    #     when stdin is piped; can appear inline mid-line)
    #   - `[extension-manager]` / `[extension-runtime:*]` /
    #     `[sub-ext]` / `[duckdb-core]` host-side registration
    #     chatter (postgis monolith registers ~1100 functions
    #     with per-line trace)
    #   - LOAD's table-mode reply (`+---+`, `| Success |`) when
    #     it slips through before `.mode csv` takes effect
    #   - CSV column-header lines emitted before each result
    #     (the ducklink CLI has no `.headers off`; the postgis
    #     smoke cases return single-column numeric results, so
    #     keeping only lines that match a pure numeric / signed
    #     numeric pattern reliably drops the headers).
    # Tests assert on actual query output, not bridge chatter.
    norm_actual="$(mktemp -t bridge-smoke.XXXXXX.norm)"
    sed -e 's/[[:space:]]*$//' \
        -e 's/D> //g' \
        -e 's/\.\.\.> //g' \
        -e '/^\[shim-/d' \
        -e '/^\[extension-/d' \
        -e '/^\[sub-ext/d' \
        -e '/^\[duckdb-core\]/d' \
        -e '/-duckdb-bridge: /d' \
        -e '/-sqlite-bridge: /d' \
        -e '/^loaded /d' \
        -e '/^Error: dlsym/d' \
        "$actual" \
        | awk '{ lines[NR]=$0; if (NF) last=NR } END { for (i=1; i<=last; i++) print lines[i] }' \
        > "$norm_actual"
    if [[ "$target" == "ducklink" ]] \
        && awk 'NF && !/^-?[0-9]+(\.[0-9]+)?$/ { bad=1; exit } END { exit bad+0 }' "$expected"
    then
        # ducklink CSV mode emits a header line per query and there
        # is no ".headers off" on the WASI guest CLI. When the
        # expected file is entirely numeric, we can whitelist data
        # lines and strip everything else — CSV column headers,
        # `+---+` / `| Success |` LOAD borders, and any leftover
        # diagnostic chatter that slipped past the prefix filters.
        # Cases with non-numeric expected output (e.g., text or
        # error messages in postgis-duckdb-only) fall through and
        # rely on the noise-prefix filter alone.
        filtered="$(mktemp -t bridge-smoke.XXXXXX.filt)"
        grep -E '^-?[0-9]+(\.[0-9]+)?$' "$norm_actual" > "$filtered" || true
        mv "$filtered" "$norm_actual"
    fi
    norm_expected="$(mktemp -t bridge-smoke.XXXXXX.norm)"
    sed -e 's/[[:space:]]*$//' "$expected" \
        | awk '{ lines[NR]=$0; if (NF) last=NR } END { for (i=1; i<=last; i++) print lines[i] }' \
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
