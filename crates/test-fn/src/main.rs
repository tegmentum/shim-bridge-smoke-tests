//! `test-fn` -- per-function shim-bridge verification harness (B1).
//!
//! MVP scope:
//! - `seed` loads TOML fixture files into the interface DB's
//!   `test_cases` table.
//! - `run` executes each non-skipped case for a function against a
//!   monolith postgis-wasm bridge via ducklink, INSERTs a
//!   `test_runs` row per case, and promotes the function's status
//!   in `scalars` to `implemented_verified` on pass-of-all.
//! - `coverage` prints a status roll-up.
//!
//! The per-function bridge cache (design B1 §4) is stubbed as a
//! single monolith cache entry keyed by (extension, bridge-path
//! sha256). Once `sqlink-shim-codegen --function` lands, the
//! `bridge_cache` module can grow into the full codegen-build-
//! compose flow without changing the CLI shape.

mod bridge_cache;
mod db;
mod hashing;
mod runner;
mod seed;
mod status;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "test-fn",
    about = "Per-function shim-bridge verification harness (B1)."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Seed `test_cases` from a TOML fixture file.
    Seed {
        /// Interface DB path. REQUIRED — there is no safe
        /// default because each shim owns its own canonical DB
        /// (postgis-shim-interface, mobilitydb-shim-interface,
        /// timescaledb-shim-interface). Pointing at a shared
        /// `/tmp` path silently drifted between callers, so the
        /// flag is now mandatory and validated below.
        #[arg(long)]
        interface: PathBuf,
        /// Extension name (e.g. `postgis`).
        #[arg(long)]
        extension: String,
        /// TOML fixture file.
        #[arg(long)]
        from: PathBuf,
        /// Overwrite existing rows with the same
        /// `(extension, function, case)` triple.
        #[arg(long)]
        replace: bool,
    },

    /// Run cases for a single function.
    Run {
        /// Interface DB path. REQUIRED — see `seed` for why.
        #[arg(long)]
        interface: PathBuf,
        /// Extension name (e.g. `postgis`).
        #[arg(long)]
        extension: String,
        /// Function name (canonical, lowercase).
        #[arg(long)]
        function: String,
        /// Optional single case name; without it every non-skipped
        /// case runs and the status-promotion rule applies.
        #[arg(long)]
        case: Option<String>,
        /// Bridge wasm (composed ducklink loadable).
        #[arg(long)]
        bridge: PathBuf,
        /// Provider wasm. Defaults to the bridge path itself for
        /// the MVP monolith mode.
        #[arg(long)]
        provider: Option<PathBuf>,
        /// Ducklink binary.
        #[arg(
            long,
            default_value = "/Users/zacharywhitley/git/ducklink/target/release/ducklink"
        )]
        ducklink: PathBuf,
        /// Force re-run even on cache-hit (`verified` + hashes match).
        #[arg(long)]
        force: bool,
        /// Emit JSON Lines instead of the human-readable block.
        #[arg(long)]
        json: bool,
    },

    /// Run cases in a batch across many functions. Selection
    /// modes:
    ///   `--all`                    every function with cases
    ///   `--leaf <leaf>`            functions tagged `leaf:<leaf>`
    ///   `--functions f1,f2,...`    explicit function list
    /// A total case cap can be applied with `--limit N` (across
    /// all selected functions, to bound run time).
    Batch {
        #[arg(long)]
        interface: PathBuf,
        #[arg(long)]
        extension: String,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        leaf: Option<String>,
        #[arg(long, value_delimiter = ',')]
        functions: Vec<String>,
        /// Cap the number of cases run across all selected
        /// functions. Bounds workflow duration.
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        bridge: PathBuf,
        #[arg(long)]
        provider: Option<PathBuf>,
        #[arg(
            long,
            default_value = "/Users/zacharywhitley/git/ducklink/target/release/ducklink"
        )]
        ducklink: PathBuf,
        /// Continue on individual failures (default). Off aborts
        /// the whole batch on the first failure.
        #[arg(long, default_value_t = true)]
        keep_going: bool,
        #[arg(long)]
        json: bool,
    },

    /// Coverage roll-up over the interface DB. No test execution.
    Coverage {
        /// Interface DB path. REQUIRED — see `seed` for why.
        #[arg(long)]
        interface: PathBuf,
        /// Extension name (e.g. `postgis`).
        #[arg(long)]
        extension: String,
        /// Roll-up axis. `leaf` groups by `tags_json` `leaf:*` tags;
        /// `function` groups by function name; `status` (default)
        /// gives the classic status histogram.
        #[arg(long, default_value = "status")]
        by: String,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
}

/// Guard against pointing at a not-yet-created interface DB. The
/// three canonical repos own their own copies; refuse to run
/// blind and drop a hint at the ones the operator likely meant.
fn require_interface(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        anyhow::bail!(
            "interface DB {} does not exist.\n\
             Point --interface at one of the canonical shim-interface \
             databases:\n  \
             ~/git/postgis-shim-interface/postgis-interface.sqlite\n  \
             ~/git/mobilitydb-shim-interface/mobilitydb-interface.sqlite\n  \
             ~/git/timescaledb-shim-interface/timescaledb-interface.sqlite",
            path.display()
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Seed {
            interface,
            extension,
            from,
            replace,
        } => {
            require_interface(&interface)?;
            seed::run(&interface, &extension, &from, replace).context("seed")
        }
        Cmd::Run {
            interface,
            extension,
            function,
            case,
            bridge,
            provider,
            ducklink,
            force,
            json,
        } => {
            require_interface(&interface)?;
            runner::run(runner::Args {
                interface,
                extension,
                function,
                case,
                bridge,
                provider,
                ducklink,
                force,
                json,
            })
            .context("run")
        }
        Cmd::Batch {
            interface,
            extension,
            all,
            leaf,
            functions,
            limit,
            bridge,
            provider,
            ducklink,
            keep_going,
            json,
        } => {
            require_interface(&interface)?;
            runner::run_batch(runner::BatchArgs {
                interface,
                extension,
                all,
                leaf,
                functions,
                limit,
                bridge,
                provider,
                ducklink,
                keep_going,
                json,
            })
            .context("batch")
        }
        Cmd::Coverage {
            interface,
            extension,
            by,
            json,
        } => {
            require_interface(&interface)?;
            match by.as_str() {
                "leaf" => db::coverage_by_leaf(&interface, &extension, json).context("coverage:leaf"),
                "function" => db::coverage_by_function(&interface, &extension, json).context("coverage:function"),
                _ => db::coverage(&interface, &extension, json).context("coverage"),
            }
        }
    }
}
