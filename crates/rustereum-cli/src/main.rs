//! The `rustereum` command-line binary: scaffold projects and manage Solidity
//! dependencies.
//!
//! This is a thin `clap` wrapper; all real work lives in the `rustereum_cli`
//! library so it can be unit-tested directly. Three subcommands are exposed:
//!
//! ```console
//! $ rustereum new my-project                                        # scaffold a project
//! $ cd my-project
//! $ rustereum add OpenZeppelin/openzeppelin-contracts --ref v5.1.0  # import a dependency
//! $ rustereum fetch                                                 # reproduce deps from rustereum.toml
//! ```
//!
//! - `new <name> [--force]` — scaffold a fresh project in `./<name>`
//!   (`rustereum_cli::scaffold_new`). Refuses a non-empty directory unless
//!   `--force`.
//! - `add <owner/repo> [--ref <tag>]` — clone a GitHub Solidity dependency,
//!   record it in `rustereum.toml`, write its remapping, and generate Rust trait
//!   bindings (`rustereum_cli::add_dependency`).
//! - `fetch` — clone every dependency declared in `rustereum.toml` and
//!   regenerate `remappings.txt` (`rustereum_cli::fetch`); this is what CI runs.
//!
//! `add` and `fetch` resolve the project root by searching upward for
//! `rustereum.toml` (`rustereum_cli::find_manifest_root`), so they work from any
//! subdirectory. On success the process exits with [`ExitCode::SUCCESS`]; any
//! error is printed to stderr and yields [`ExitCode::FAILURE`].

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use rustereum_cli::{add_dependency, fetch, find_manifest_root, scaffold_new};

/// Top-level command-line interface: a single required subcommand.
#[derive(Parser)]
#[command(
    name = "rustereum",
    about = "Scaffold rustereum projects and import Solidity deps"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// The `rustereum` subcommands. Each variant's doc comment is surfaced by clap
/// as the command's `--help` text.
#[derive(Subcommand)]
enum Commands {
    /// Create a new rustereum project in `./<name>`.
    New {
        /// Project name (also the directory created under the cwd).
        name: String,
        /// Overwrite a non-empty target directory.
        #[arg(long)]
        force: bool,
    },
    /// Clone a GitHub Solidity dependency and generate Rust bindings.
    Add {
        /// GitHub spec, e.g. OpenZeppelin/openzeppelin-contracts.
        github_spec: String,
        /// Git ref (tag/branch) to clone, e.g. v5.1.0.
        #[arg(long = "ref")]
        git_ref: Option<String>,
    },
    /// Clone deps declared in rustereum.toml and regenerate remappings.txt.
    Fetch,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::New { name, force } => {
            let target = PathBuf::from(&name);
            scaffold_new(&target, &name, force).map(|_| {
                println!("Created rustereum project at {}", target.display());
            })
        }
        Commands::Add {
            github_spec,
            git_ref,
        } => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let root = find_manifest_root(&cwd);
            add_dependency(&root, &github_spec, git_ref.as_deref()).map(|_| {
                println!("Added {github_spec} to {}", root.display());
            })
        }
        Commands::Fetch => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let root = find_manifest_root(&cwd);
            fetch(&root).map(|_| {
                println!("Fetched dependencies for {}", root.display());
            })
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
