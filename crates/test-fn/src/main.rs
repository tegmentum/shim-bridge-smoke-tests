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
        /// Interface DB path.
        #[arg(long, default_value = "/tmp/postgis-interface.sqlite")]
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
        /// Interface DB path.
        #[arg(long, default_value = "/tmp/postgis-interface.sqlite")]
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

    /// Coverage roll-up over the interface DB. No test execution.
    Coverage {
        /// Interface DB path.
        #[arg(long, default_value = "/tmp/postgis-interface.sqlite")]
        interface: PathBuf,
        /// Extension name (e.g. `postgis`).
        #[arg(long)]
        extension: String,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Seed {
            interface,
            extension,
            from,
            replace,
        } => seed::run(&interface, &extension, &from, replace)
            .context("seed"),
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
        } => runner::run(runner::Args {
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
        .context("run"),
        Cmd::Coverage {
            interface,
            extension,
            json,
        } => db::coverage(&interface, &extension, json).context("coverage"),
    }
}
