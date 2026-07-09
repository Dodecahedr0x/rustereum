//! The `rustereum` CLI: scaffold projects and import Solidity dependencies.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use rustereum_cli::{add_dependency, find_project_root, scaffold_new};

#[derive(Parser)]
#[command(
    name = "rustereum",
    about = "Scaffold rustereum projects and import Solidity deps"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new rustereum project in ./<name>.
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
            let root = find_project_root(&cwd);
            add_dependency(&root, &github_spec, git_ref.as_deref()).map(|_| {
                println!("Added {github_spec} to {}", root.display());
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
