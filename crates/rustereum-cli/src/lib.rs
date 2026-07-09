//! Library logic for the `rustereum` CLI: project scaffolding, git dependency
//! import, and Solidity → Rust trait binding generation.
//!
//! Everything here is plain functions so tests can call them directly; the
//! `main.rs` binary is a thin clap wrapper around these. The three subcommands
//! map to public entry points:
//!
//! - `rustereum new`   → [`scaffold_new`]
//! - `rustereum add`   → [`add_dependency`]
//! - `rustereum fetch` → [`fetch`]
//!
//! # Dependency model
//!
//! Imported Solidity sources are **not committed**. They are declared in a
//! [`Manifest`] (`rustereum.toml`, beside `Cargo.toml`) and cloned into a
//! git-ignored `lib/` directory. `add` clones immediately and records the
//! dependency; `fetch` reproduces every declared dependency from the manifest
//! (this is what CI runs). Both regenerate `remappings.txt`, the file that tells
//! `solc` how to resolve `import "@openzeppelin/..."` style paths.
//!
//! ## `rustereum.toml` schema
//!
//! ```toml
//! [dependencies.openzeppelin-contracts]
//! git = "https://github.com/OpenZeppelin/openzeppelin-contracts"
//! rev = "v5.1.0"                                # optional tag/branch
//! remap = "@openzeppelin/contracts/=contracts/"
//! ```
//!
//! The table key (`openzeppelin-contracts`) is the `lib/` clone directory name.
//! `remap` is `"<prefix>=<subpath>"`, where `<subpath>` is relative to the clone
//! root; [`fetch`] expands it into the `remappings.txt` line
//! `<prefix>=lib/<key>/<subpath>`. See [`Manifest`] and [`Dependency`].
//!
//! # Bindings and the `#[contract]` macro
//!
//! [`add_dependency`] also scans the cloned `.sol` files and, via
//! [`generate_bindings`], writes one `pub trait` per Solidity
//! contract/interface into `src/bindings.rs`. Each trait carries two consts:
//!
//! ```ignore
//! pub trait Ownable {
//!     const SOL_NAME: &'static str = "Ownable";
//!     const SOL_IMPORT: &'static str = "@openzeppelin/contracts/access/Ownable.sol";
//! }
//! ```
//!
//! Implementing such a trait (`impl Ownable for MyContract {}`) is how a
//! contract declares inheritance. The `#[contract]` macro reads `SOL_NAME` and
//! `SOL_IMPORT` off the trait to emit the correct Solidity `import` and base
//! name — so the macro never touches the filesystem, and the binding traits are
//! the committed handoff between the CLI and the codegen.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The `rustereum.toml` manifest: the committed, reproducible record of a
/// project's Solidity git dependencies. `rustereum fetch` reads this to clone
/// declared deps and regenerate `remappings.txt`.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Manifest {
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
}

/// A single Solidity git dependency entry in `rustereum.toml`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Dependency {
    /// Repository URL to clone.
    pub git: String,
    /// Optional tag/branch (used as `git clone --branch <rev>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// Remapping spec `"<prefix>=<subpath>"` where `<subpath>` is relative to
    /// the clone root.
    pub remap: String,
}

/// Read `<project_root>/rustereum.toml` into a [`Manifest`].
///
/// Returns [`Manifest::default`] (an empty dependency set) if the file is
/// absent, so callers need not special-case a fresh project. TOML parse errors
/// are mapped to [`io::Error::other`]. The inverse is [`write_manifest`].
pub fn read_manifest(project_root: &Path) -> io::Result<Manifest> {
    let path = project_root.join("rustereum.toml");
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Manifest::default()),
        Err(e) => return Err(e),
    };
    toml::from_str(&text).map_err(|e| io::Error::other(format!("failed to parse {path:?}: {e}")))
}

/// Serialize `m` and write it to `<project_root>/rustereum.toml`.
///
/// The inverse of [`read_manifest`]. Serialization failures are mapped to
/// [`io::Error::other`].
pub fn write_manifest(project_root: &Path, m: &Manifest) -> io::Result<()> {
    let text = toml::to_string_pretty(m)
        .map_err(|e| io::Error::other(format!("failed to serialize manifest: {e}")))?;
    fs::write(project_root.join("rustereum.toml"), text)
}

/// Run `git clone --depth 1 [--branch <rev>] <url> <target>`.
fn git_clone(url: &str, rev: Option<&str>, target: &Path) -> io::Result<()> {
    let mut cmd = Command::new("git");
    cmd.arg("clone").arg("--depth").arg("1");
    if let Some(r) = rev {
        cmd.arg("--branch").arg(r);
    }
    cmd.arg(url).arg(target);

    let status = cmd
        .status()
        .map_err(|e| io::Error::other(format!("failed to run git: {e}")))?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "git clone of {url} failed (status {status})"
        )));
    }
    Ok(())
}

/// Reproduce a project's Solidity dependencies from its [`Manifest`].
///
/// Reads `<project_root>/rustereum.toml`, clones every declared [`Dependency`]
/// into `lib/<name>`, and (re)generates `remappings.txt`. This is the command
/// CI runs before tests, and what you run after cloning a project. Each
/// manifest `remap` of the form `"<prefix>=<subpath>"` becomes the
/// `remappings.txt` line `<prefix>=lib/<name>/<subpath>`; lines are emitted in
/// sorted key order (the [`BTreeMap`] iteration order) so output is
/// deterministic.
///
/// Idempotent: existing clones under `lib/<name>` are skipped, so `fetch` is
/// safely re-runnable offline. Unlike [`add_dependency`], `fetch` does **not**
/// regenerate bindings — `src/bindings.rs` is committed source and is left
/// untouched.
///
/// If the manifest declares no dependencies, this prints a note and returns
/// `Ok(())` without writing `remappings.txt`.
pub fn fetch(project_root: &Path) -> io::Result<()> {
    let manifest = read_manifest(project_root)?;
    if manifest.dependencies.is_empty() {
        eprintln!("no dependencies declared in rustereum.toml — nothing to fetch");
        return Ok(());
    }

    fs::create_dir_all(project_root.join("lib"))?;

    // BTreeMap iterates sorted by key, so remapping lines are deterministic.
    let mut remap_lines = Vec::new();
    for (name, dep) in &manifest.dependencies {
        let clone_target = project_root.join("lib").join(name);
        if clone_target.exists() {
            eprintln!(
                "skipping {name}: already present at {}",
                clone_target.display()
            );
        } else {
            git_clone(&dep.git, dep.rev.as_deref(), &clone_target)?;
        }

        let (prefix, subpath) = dep.remap.split_once('=').ok_or_else(|| {
            io::Error::other(format!(
                "invalid remap for {name}: {:?} (expected \"prefix=subpath\")",
                dep.remap
            ))
        })?;
        remap_lines.push(format!("{prefix}=lib/{name}/{subpath}"));
    }

    let mut contents = remap_lines.join("\n");
    contents.push('\n');
    fs::write(project_root.join("remappings.txt"), contents)?;
    Ok(())
}

/// The template contract dropped into a fresh project's `src/lib.rs`.
const TEMPLATE_LIB: &str = r#"//! A rustereum project. Edit this contract or add your own modules.

use rustereum::prelude::*;

#[contract]
pub struct Counter {
    count: u256,
}

#[contract]
impl Counter {
    pub fn increment(&mut self) {
        self.count += 1;
    }

    pub fn get(&self) -> u256 {
        self.count
    }
}

mod bindings;
"#;

/// The template `rustereum.toml` dropped into a fresh project's root.
const TEMPLATE_MANIFEST: &str = "\
# Solidity dependencies. Add with `rustereum add <owner/repo> --ref <tag>`,
# then run `rustereum fetch` to clone them into `lib/` (git-ignored).

[dependencies]
";

/// The template `.gitignore` dropped into a fresh project's root, so scaffolded
/// projects don't commit build output or fetched deps.
const TEMPLATE_GITIGNORE: &str = "\
/target
lib/
remappings.txt
";

/// Scaffold a fresh rustereum project skeleton at `target_dir`.
///
/// Backs `rustereum new`. Creates:
///
/// ```text
/// <target_dir>/
/// ├── Cargo.toml         # `name`, with a placeholder rustereum dependency
/// ├── rustereum.toml     # empty [dependencies] manifest (beside Cargo.toml)
/// ├── .gitignore         # ignores /target, lib/, remappings.txt
/// └── src/
///     ├── lib.rs         # a working template Counter contract
///     └── bindings.rs    # empty binding module (populated by `add`)
/// ```
///
/// `lib/` and `remappings.txt` are generated by [`fetch`] (and git-ignored), so
/// they are not scaffolded here. The generated `Cargo.toml` points `rustereum`
/// at a placeholder version, since the crate is not yet published — consumers
/// repoint it at a path/git dependency.
///
/// If `target_dir` already exists and is non-empty, this returns
/// [`io::ErrorKind::AlreadyExists`] unless `force` is set.
pub fn scaffold_new(target_dir: &Path, name: &str, force: bool) -> io::Result<()> {
    if target_dir.exists() {
        let non_empty = fs::read_dir(target_dir)?.next().is_some();
        if non_empty && !force {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "target directory {} is not empty (use --force to overwrite)",
                    target_dir.display()
                ),
            ));
        }
    }

    let src = target_dir.join("src");
    fs::create_dir_all(&src)?;

    // `rustereum = "0.1"` is a placeholder — rustereum is not published yet, so
    // consumers must repoint this at a path/git dependency for now.
    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
# Placeholder: rustereum is not published yet. Point this at a path or git
# dependency (e.g. rustereum = {{ path = "../rustereum" }}) until it is.
rustereum = "0.1"
alloy-primitives = "0.8"

[dev-dependencies]
"#
    );
    fs::write(target_dir.join("Cargo.toml"), cargo_toml)?;
    fs::write(target_dir.join("rustereum.toml"), TEMPLATE_MANIFEST)?;
    fs::write(target_dir.join(".gitignore"), TEMPLATE_GITIGNORE)?;
    fs::write(src.join("lib.rs"), TEMPLATE_LIB)?;
    fs::write(src.join("bindings.rs"), bindings_header())?;

    Ok(())
}

fn bindings_header() -> String {
    "//! Generated by `rustereum add`.\n".to_string()
}

/// Recursively collect all `.sol` file paths under `root`, sorted for
/// deterministic output.
fn collect_sol_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("sol") {
                out.push(path);
            }
        }
    }
    walk(root, &mut out);
    out.sort();
    out
}

/// Extract contract/interface names declared in a Solidity source string.
///
/// Matches `contract Name`, `abstract contract Name`, and `interface Name`.
/// Skips `library`. This is a deliberately simple line scan — not a full
/// Solidity parser (YAGNI).
fn extract_declarations(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    for raw in source.lines() {
        let line = raw.trim();
        let rest = if let Some(r) = line.strip_prefix("abstract contract ") {
            Some(r)
        } else if let Some(r) = line.strip_prefix("contract ") {
            Some(r)
        } else {
            line.strip_prefix("interface ")
        };
        if let Some(rest) = rest {
            if let Some(name) = parse_identifier(rest) {
                names.push(name);
            }
        }
    }
    names
}

/// Read a leading Solidity identifier from `s` (stops at the first char that
/// can't be part of an identifier, e.g. space, `{`, or `is`).
fn parse_identifier(s: &str) -> Option<String> {
    let ident: String = s
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    if ident.is_empty() || ident.chars().next().unwrap().is_numeric() {
        None
    } else {
        Some(ident)
    }
}

/// Generate Rust trait bindings for every contract/interface found under
/// `sol_root`, returned as the text of a bindings module.
///
/// Every `contract`, `abstract contract`, and `interface` declared in a `.sol`
/// file under `sol_root` gets one `pub trait` with two associated consts:
///
/// - `SOL_NAME` — the Solidity contract/interface name, and
/// - `SOL_IMPORT` — the import path `<remap_prefix><rel>`, where `<rel>` is the
///   file's path relative to `sol_root`.
///
/// These are exactly the consts the `#[contract]` macro reads when it lowers an
/// `impl Trait for Contract {}` inheritance declaration into a Solidity
/// `import` and base name. `remap_prefix` must therefore be the same import
/// prefix registered in `remappings.txt` (e.g. `@openzeppelin/contracts/`), so
/// the emitted `SOL_IMPORT` resolves through `solc`. See [`add_dependency`] for
/// how the prefix and scan root are chosen.
///
/// Duplicate trait names (the same contract name declared in multiple files)
/// are emitted once — the first occurrence wins. Library declarations
/// (`library X`) are skipped.
pub fn generate_bindings(sol_root: &Path, remap_prefix: &str) -> String {
    let mut out = bindings_header();
    let mut seen: Vec<String> = Vec::new();

    for file in collect_sol_files(sol_root) {
        let rel = file
            .strip_prefix(sol_root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        let import = format!("{remap_prefix}{rel}");
        let Ok(source) = fs::read_to_string(&file) else {
            continue;
        };
        for name in extract_declarations(&source) {
            if seen.contains(&name) {
                continue;
            }
            seen.push(name.clone());
            out.push_str(&format!(
                "pub trait {name} {{\n    const SOL_NAME: &'static str = \"{name}\";\n    const SOL_IMPORT: &'static str = \"{import}\";\n}}\n",
            ));
        }
    }

    out
}

/// Result of resolving how a cloned repo maps into Solidity import space.
struct Remap {
    /// Import prefix, e.g. `@openzeppelin/contracts/`.
    prefix: String,
    /// The remapping target relative to the project root, e.g.
    /// `lib/openzeppelin-contracts/contracts/`.
    target: String,
    /// Directory to scan for `.sol` files (absolute).
    sol_root: PathBuf,
}

fn resolve_remap(project_root: &Path, repo: &str) -> Remap {
    let clone_dir = format!("lib/{repo}");
    let has_contracts = project_root.join(&clone_dir).join("contracts").is_dir();

    // Special-case OpenZeppelin so canonical `@openzeppelin/contracts/...`
    // imports resolve. This is the one remapping that must work verbatim.
    if repo == "openzeppelin-contracts" && has_contracts {
        return Remap {
            prefix: "@openzeppelin/contracts/".to_string(),
            target: format!("{clone_dir}/contracts/"),
            sol_root: project_root.join(&clone_dir).join("contracts"),
        };
    }

    if has_contracts {
        Remap {
            prefix: format!("@{repo}/"),
            target: format!("{clone_dir}/contracts/"),
            sol_root: project_root.join(&clone_dir).join("contracts"),
        }
    } else {
        Remap {
            prefix: format!("@{repo}/"),
            target: format!("{clone_dir}/"),
            sol_root: project_root.join(&clone_dir),
        }
    }
}

/// Derive the repository name from a `owner/repo` GitHub spec.
fn repo_name(github_spec: &str) -> &str {
    github_spec
        .trim_end_matches(".git")
        .rsplit('/')
        .next()
        .unwrap_or(github_spec)
}

/// Import a GitHub Solidity dependency: clone it, record it, remap it, and
/// generate bindings.
///
/// Backs `rustereum add`. Given a `github_spec` of the form `owner/repo` and an
/// optional git `git_ref` (tag or branch), this:
///
/// 1. `git clone`s `https://github.com/<spec>.git` into `lib/<repo>` (failing
///    with [`io::ErrorKind::AlreadyExists`] if it is already present),
/// 2. resolves the import remapping (OpenZeppelin is special-cased to the
///    canonical `@openzeppelin/contracts/` prefix; a repo with a top-level
///    `contracts/` directory maps `@<repo>/` to `lib/<repo>/contracts/`,
///    otherwise to `lib/<repo>/`) and appends it to `remappings.txt` if absent,
/// 3. records the dependency in `rustereum.toml` (via [`write_manifest`]) so
///    [`fetch`] can reproduce it,
/// 4. generates trait bindings for the newly cloned sources (via
///    [`generate_bindings`]) and merges the new traits into `src/bindings.rs`,
///    leaving any already-defined traits untouched.
///
/// If the clone contains no contracts/interfaces, it prints a warning and
/// leaves the bindings file unchanged.
pub fn add_dependency(
    project_root: &Path,
    github_spec: &str,
    git_ref: Option<&str>,
) -> io::Result<()> {
    let repo = repo_name(github_spec);
    let clone_target = project_root.join("lib").join(repo);

    fs::create_dir_all(project_root.join("lib"))?;

    if clone_target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("dependency already present at {}", clone_target.display()),
        ));
    }

    let url = format!(
        "https://github.com/{}.git",
        github_spec.trim_end_matches(".git")
    );
    git_clone(&url, git_ref, &clone_target)?;

    let remap = resolve_remap(project_root, repo);

    // Append the remapping line if not already present (idempotent).
    let remap_line = format!("{}={}", remap.prefix, remap.target);
    append_line_if_absent(&project_root.join("remappings.txt"), &remap_line)?;

    // Record the dependency into rustereum.toml so `fetch` can reproduce it.
    // The manifest is the committed, reproducible record even though `add`
    // clones immediately. `remap.target` is `lib/<repo>/<subpath>`; strip the
    // `lib/<repo>/` prefix to store the clone-relative subpath.
    let subpath = remap
        .target
        .strip_prefix(&format!("lib/{repo}/"))
        .unwrap_or(&remap.target)
        .to_string();
    let mut manifest = read_manifest(project_root)?;
    manifest.dependencies.insert(
        repo.to_string(),
        Dependency {
            git: url.clone(),
            rev: git_ref.map(str::to_string),
            remap: format!("{}={}", remap.prefix, subpath),
        },
    );
    write_manifest(project_root, &manifest)?;

    // Generate bindings for the newly added dependency.
    let generated = generate_bindings(&remap.sol_root, &remap.prefix);
    let trait_count = generated.matches("pub trait ").count();
    if trait_count == 0 {
        eprintln!(
            "warning: no contracts/interfaces found under {} — bindings unchanged",
            remap.sol_root.display()
        );
        return Ok(());
    }

    merge_bindings(&project_root.join("src").join("bindings.rs"), &generated)?;
    Ok(())
}

/// Append `line` to the file at `path` if it is not already present. Creates
/// the file if missing.
fn append_line_if_absent(path: &Path, line: &str) -> io::Result<()> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        writeln!(file)?;
    }
    writeln!(file, "{line}")?;
    Ok(())
}

/// Merge freshly generated trait definitions into an existing bindings file,
/// skipping any trait name already defined there.
fn merge_bindings(path: &Path, generated: &str) -> io::Result<()> {
    let existing = fs::read_to_string(path).unwrap_or_else(|_| bindings_header());

    // Collect trait names already present.
    let existing_traits: Vec<String> = existing
        .lines()
        .filter_map(|l| l.trim().strip_prefix("pub trait "))
        .filter_map(parse_identifier)
        .collect();

    // Split the generated output into per-trait blocks and keep only new ones.
    let mut additions = String::new();
    let mut current: Option<(String, String)> = None; // (name, block)
    for line in generated.lines() {
        if let Some(rest) = line.trim().strip_prefix("pub trait ") {
            // Flush previous block.
            if let Some((name, block)) = current.take() {
                if !existing_traits.contains(&name) {
                    additions.push_str(&block);
                }
            }
            let name = parse_identifier(rest).unwrap_or_default();
            current = Some((name, format!("{line}\n")));
        } else if let Some((_, block)) = current.as_mut() {
            block.push_str(line);
            block.push('\n');
        }
        // Header / blank lines before the first trait are ignored.
    }
    if let Some((name, block)) = current.take() {
        if !existing_traits.contains(&name) {
            additions.push_str(&block);
        }
    }

    if additions.is_empty() {
        return Ok(());
    }

    let mut merged = existing;
    if !merged.ends_with('\n') {
        merged.push('\n');
    }
    merged.push_str(&additions);
    fs::write(path, merged)
}

/// Locate a project root by searching upward for `remappings.txt`.
///
/// Returns `start` if it contains `remappings.txt`, otherwise walks up the
/// parent chain until one is found. Falls back to `start` if no ancestor has
/// one. Compare [`find_manifest_root`], which keys off `rustereum.toml`
/// instead — the CLI subcommands use the manifest-based root, since
/// `remappings.txt` is git-ignored and may not exist before a `fetch`.
pub fn find_project_root(start: &Path) -> PathBuf {
    let mut dir = start;
    loop {
        if dir.join("remappings.txt").exists() {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return start.to_path_buf(),
        }
    }
}

/// Locate a project root by searching upward for `rustereum.toml`.
///
/// Returns `start` if it contains `rustereum.toml`, otherwise walks up the
/// parent chain until one is found; falls back to `start` if none exists. This
/// is the root the `add` and `fetch` subcommands resolve from the current
/// directory, so they work when invoked from a project subdirectory. Compare
/// [`find_project_root`], which keys off `remappings.txt`.
pub fn find_manifest_root(start: &Path) -> PathBuf {
    let mut dir = start;
    loop {
        if dir.join("rustereum.toml").exists() {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return start.to_path_buf(),
        }
    }
}
