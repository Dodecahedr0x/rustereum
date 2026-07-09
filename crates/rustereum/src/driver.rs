//! The compile driver: [`ir`](crate::ir) → Solidity → `solc` → bytecode + ABI.
//!
//! This is the last stage of the [pipeline](crate#the-compilation-pipeline).
//! [`compile_contract`] (and [`compile_contract_with`]) lower a
//! [`Contract`] to Solidity via
//! [`lower_solidity`](crate::solidity::lower_solidity), invoke `solc` (obtained
//! and pinned by [`foundry-compilers`](https://crates.io/crates/foundry-compilers)
//! via svm), and return an [`Artifact`]. Along the way three files are written
//! to `target/rustereum/`:
//!
//! - `<Name>.sol` — the generated Solidity (written *before* solc runs, so a
//!   compile failure still leaves it for inspection),
//! - `<Name>.yul` — solc's Yul IR, dumped for inspection,
//! - `<Name>.json` — the deployment bytecode + ABI.
//!
//! Solidity imports (for inherited OpenZeppelin sources) are resolved through a
//! project's `remappings.txt`; [`compile_contract`] discovers the project root
//! by searching upward for that file, while [`compile_contract_with`] takes it
//! explicitly via [`CompileOptions`].
//!
//! In practice a `#[contract]` type is compiled through the macro-generated
//! `Contract::compile()` / `Contract::compile_with(&opts)`, which call these
//! functions for you.

use crate::ir::Contract;
use foundry_compilers::solc::Solc;
use semver::Version;
use std::path::PathBuf;

/// solc version fetched/pinned by foundry-compilers (via svm) for compilation.
const SOLC_VERSION: &str = "0.8.28";

/// A compiled contract: the paths written to disk plus the resulting bytecode
/// and ABI. Returned by [`compile_contract`] / [`compile_contract_with`].
pub struct Artifact {
    /// The contract name (matches [`Contract::name`]).
    pub name: String,
    /// Path to the generated Solidity source (`target/rustereum/<Name>.sol`).
    pub sol_path: PathBuf,
    /// Path to solc's dumped Yul IR (`target/rustereum/<Name>.yul`).
    pub yul_path: PathBuf,
    /// The creation (deployment) bytecode.
    pub bytecode: Vec<u8>,
    /// The contract ABI as JSON (a JSON array of ABI entries).
    pub abi: serde_json::Value,
}

/// An error from [`compile_contract`] / [`compile_contract_with`].
#[derive(Debug)]
pub enum CompileError {
    /// A filesystem error writing the `.sol`/`.yul`/`.json` artifacts.
    Io(std::io::Error),
    /// A solc-side failure: solc could not be obtained, compilation reported
    /// fatal errors, or the expected contract/bytecode was missing from the
    /// output. The string includes a hint pointing at `target/rustereum/<Name>.sol`.
    Solc(String),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::Io(e) => write!(f, "io error: {e}"),
            CompileError::Solc(s) => write!(f, "solc error: {s}"),
        }
    }
}

impl std::error::Error for CompileError {}

impl From<std::io::Error> for CompileError {
    fn from(e: std::io::Error) -> Self {
        CompileError::Io(e)
    }
}

/// Base output directory: `<OUT_DIR|CARGO_TARGET_DIR|target>/rustereum`, created if missing.
fn target_dir() -> PathBuf {
    let base = std::env::var_os("OUT_DIR")
        .or_else(|| std::env::var_os("CARGO_TARGET_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    let dir = base.join("rustereum");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Options controlling how a contract is compiled: primarily the project root
/// used to resolve Solidity imports (via `remappings.txt`) against vendored
/// dependency sources (e.g. OpenZeppelin). Passed to [`compile_contract_with`].
pub struct CompileOptions {
    /// The project root under which `remappings.txt` is read. Remapping targets
    /// are resolved relative to this directory and made absolute so solc finds
    /// vendored dependency sources regardless of the current directory.
    pub project_root: PathBuf,
}

/// Lower `c` to Solidity, write it to disk, then compile it to EVM bytecode via
/// foundry-compilers (solc, standard-JSON `language: "Solidity"`). The
/// solc-generated Yul (IR) is dumped alongside for inspection. Returns an
/// [`Artifact`].
///
/// The project root is discovered by searching upward from the current
/// directory for a `remappings.txt`; if none is found the current directory is
/// used (standalone contracts need no remappings). Use
/// [`compile_contract_with`] to set the project root explicitly.
///
/// Contracts usually call the macro-generated `Contract::compile()` instead of
/// this directly.
pub fn compile_contract(c: &Contract) -> Result<Artifact, CompileError> {
    let project_root = find_project_root();
    compile_contract_with(c, &CompileOptions { project_root })
}

/// Read `remappings.txt` under `root`, returning `prefix=<absolute target>`
/// strings suitable for solc's `settings.remappings`. Missing or empty files
/// yield an empty list. Targets are made absolute (joined onto `root` and
/// canonicalized when possible) so solc resolves imports regardless of cwd.
fn read_remappings(root: &std::path::Path) -> Vec<String> {
    let path = root.join("remappings.txt");
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    contents
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            let (prefix, target) = line.split_once('=')?;
            let target = target.trim();
            let joined = root.join(target);
            let mut abs = std::fs::canonicalize(&joined)
                .unwrap_or(joined)
                .display()
                .to_string();
            // solc does a textual prefix replacement, so a remapping whose
            // prefix ends in `/` needs a target that also ends in `/` (else
            // `@oz/contracts/` + `access/..` -> `contractsaccess/..`).
            // canonicalize drops trailing separators; restore one to match.
            if target.ends_with('/') && !abs.ends_with('/') {
                abs.push('/');
            }
            Some(format!("{}={}", prefix.trim(), abs))
        })
        .collect()
}

/// Search upward from the current directory for a directory containing
/// `remappings.txt`, returning it as the project root. Falls back to the
/// current directory (or `.`) when none is found.
fn find_project_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut dir = cwd.as_path();
    loop {
        if dir.join("remappings.txt").is_file() {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }
    cwd
}

/// Like [`compile_contract`], but resolves Solidity imports using the project's
/// `remappings.txt` (under [`opts.project_root`](CompileOptions::project_root)),
/// with remapping targets made absolute so vendored dependency sources (e.g.
/// OpenZeppelin) are found.
///
/// This is the workhorse both [`compile_contract`] and the macro-generated
/// `Contract::compile_with(&opts)` delegate to. On failure it returns a
/// [`CompileError`]; the generated `.sol` is written before solc runs, so it
/// remains at `target/rustereum/<Name>.sol` for inspection even on error.
pub fn compile_contract_with(
    c: &Contract,
    opts: &CompileOptions,
) -> Result<Artifact, CompileError> {
    let sol = crate::solidity::lower_solidity(c);

    // Write the Solidity FIRST so a later solc failure still leaves it for
    // inspection.
    let dir = target_dir();
    let sol_path = dir.join(format!("{}.sol", c.name));
    std::fs::write(&sol_path, &sol)?;

    let remappings = read_remappings(&opts.project_root);

    let source_name = format!("{}.sol", c.name);
    let input = serde_json::json!({
        "language": "Solidity",
        "sources": { source_name.clone(): { "content": sol } },
        "settings": {
            "outputSelection": { "*": { "*": ["evm.bytecode.object", "abi", "ir"] } },
            "optimizer": { "enabled": true },
            "remappings": remappings,
        }
    });

    let version = Version::parse(SOLC_VERSION).expect("SOLC_VERSION is valid semver");
    let mut solc = Solc::find_or_install(&version)
        .map_err(|e| CompileError::Solc(format!("could not obtain solc: {e}")))?;

    // Let solc read the vendored dependency sources that the (absolute)
    // remapping targets point at. Without this, solc reports "File not found"
    // for imports that resolve outside its default sandbox.
    if !remappings.is_empty() {
        let root =
            std::fs::canonicalize(&opts.project_root).unwrap_or_else(|_| opts.project_root.clone());
        solc.allow_paths.insert(root);
    }

    let output: serde_json::Value = solc.compile_as(&input).map_err(|e| {
        CompileError::Solc(format!(
            "solc compilation failed: {e}; inspect target/rustereum/{}.sol",
            c.name
        ))
    })?;

    // Surface solc errors (as opposed to warnings).
    if let Some(errors) = output.get("errors").and_then(|e| e.as_array()) {
        let fatal: Vec<String> = errors
            .iter()
            .filter(|e| e.get("severity").and_then(|s| s.as_str()) == Some("error"))
            .map(|e| {
                e.get("formattedMessage")
                    .and_then(|m| m.as_str())
                    .unwrap_or("<no message>")
                    .to_string()
            })
            .collect();
        if !fatal.is_empty() {
            let joined = fatal.join("\n");
            let hint = if joined.contains("File not found")
                || joined.contains("not found: File")
                || joined.contains("Source \"")
            {
                " ; did you run 'rustereum add'? (missing remapping or dependency)"
            } else {
                ""
            };
            return Err(CompileError::Solc(format!(
                "{}; inspect target/rustereum/{}.sol{}",
                joined, c.name, hint
            )));
        }
    }

    // Navigate: contracts -> <file> -> <contract>. Index explicitly by the
    // source file name and object name rather than grabbing the first entry,
    // so this is robust regardless of ordering.
    let contract = output
        .get("contracts")
        .and_then(|files| files.get(&source_name))
        .and_then(|file| file.get(&c.name))
        .ok_or_else(|| {
            CompileError::Solc(format!(
                "solc output missing contract {} in {}; inspect target/rustereum/{}.sol",
                c.name, source_name, c.name
            ))
        })?;

    let object = contract
        .get("evm")
        .and_then(|e| e.get("bytecode"))
        .and_then(|b| b.get("object"))
        .and_then(|o| o.as_str())
        .ok_or_else(|| {
            CompileError::Solc(format!(
                "no bytecode object in solc output; inspect target/rustereum/{}.sol",
                c.name
            ))
        })?;

    let hex_str = object.strip_prefix("0x").unwrap_or(object);
    let bytecode = decode_hex(hex_str).ok_or_else(|| {
        CompileError::Solc(format!(
            "invalid hex bytecode from solc; inspect target/rustereum/{}.sol",
            c.name
        ))
    })?;

    let abi = contract
        .get("abi")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));

    // Dump the solc-generated Yul (IR) alongside the Solidity source.
    let ir = contract.get("ir").and_then(|s| s.as_str()).unwrap_or("");
    let yul_path = dir.join(format!("{}.yul", c.name));
    std::fs::write(&yul_path, ir)?;

    let json_path = dir.join(format!("{}.json", c.name));
    let artifact_json = serde_json::json!({
        "bytecode": format!("0x{hex_str}"),
        "abi": abi,
    });
    std::fs::write(
        &json_path,
        serde_json::to_string_pretty(&artifact_json).unwrap(),
    )?;

    Ok(Artifact {
        name: c.name.clone(),
        sol_path,
        yul_path,
        bytecode,
        abi,
    })
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    fn counter() -> Contract {
        Contract {
            name: "Counter".into(),
            inherits: vec![],
            fields: vec![Field {
                name: "count".into(),
                ty: Type::U256,
            }],
            constructor: None,
            methods: vec![
                Method {
                    name: "increment".into(),
                    mutates: true,
                    params: vec![],
                    ret: None,
                    modifiers: vec![],
                    body: vec![Stmt::Assign {
                        target: Place::Storage("count".into()),
                        op: AssignOp::Add,
                        value: Expr::Literal(1),
                    }],
                },
                Method {
                    name: "get".into(),
                    mutates: false,
                    params: vec![],
                    ret: Some(Type::U256),
                    modifiers: vec![],
                    body: vec![Stmt::Return(Expr::StorageLoad("count".into()))],
                },
            ],
        }
    }

    fn ownable_counter() -> Contract {
        Contract {
            name: "Counter".into(),
            inherits: vec![Parent {
                name: "Ownable".into(),
                import_path: "@openzeppelin/contracts/access/Ownable.sol".into(),
                base_args: vec!["initialOwner".into()],
            }],
            fields: vec![Field {
                name: "count".into(),
                ty: Type::U256,
            }],
            constructor: Some(Constructor {
                params: vec![Param {
                    name: "initial_owner".into(),
                    ty: Type::Address,
                }],
                body: vec![],
            }),
            methods: vec![
                Method {
                    name: "increment".into(),
                    mutates: true,
                    params: vec![],
                    ret: None,
                    modifiers: vec!["onlyOwner".into()],
                    body: vec![Stmt::Assign {
                        target: Place::Storage("count".into()),
                        op: AssignOp::Add,
                        value: Expr::Literal(1),
                    }],
                },
                Method {
                    name: "get".into(),
                    mutates: false,
                    params: vec![],
                    ret: Some(Type::U256),
                    modifiers: vec![],
                    body: vec![Stmt::Return(Expr::StorageLoad("count".into()))],
                },
            ],
        }
    }

    #[test]
    fn compiles_inheriting_contract_with_remappings() {
        let opts = CompileOptions {
            project_root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/project"),
        };
        let artifact = compile_contract_with(&ownable_counter(), &opts).expect("compile");
        assert!(!artifact.bytecode.is_empty());
        // ABI includes the inherited owner() function.
        assert!(artifact.abi.to_string().contains("owner"));
    }

    #[test]
    fn compile_writes_solidity_yul_and_returns_bytecode() {
        let artifact = compile_contract(&counter()).expect("compile");
        assert!(
            artifact.sol_path.exists(),
            "solidity artifact must be written"
        );
        assert!(std::fs::read_to_string(&artifact.sol_path)
            .unwrap()
            .contains("contract Counter"));
        assert!(
            artifact.yul_path.exists(),
            "solc-generated Yul must be dumped"
        );
        assert!(!artifact.bytecode.is_empty());
        // Real ABI now (increment/get present), not the empty [] from the Yul backend.
        let abi = artifact.abi.to_string();
        assert!(abi.contains("increment") && abi.contains("get"));
    }
}
