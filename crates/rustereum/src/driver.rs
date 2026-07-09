use crate::ir::Contract;
use foundry_compilers::solc::Solc;
use semver::Version;
use std::path::PathBuf;

/// solc version fetched/pinned by foundry-compilers (via svm) for compilation.
const SOLC_VERSION: &str = "0.8.28";

/// A compiled contract: the Solidity source and solc-generated Yul (IR) that
/// were written to disk, plus the resulting creation (deployment) bytecode and
/// ABI.
pub struct Artifact {
    pub name: String,
    pub sol_path: PathBuf,
    pub yul_path: PathBuf,
    pub bytecode: Vec<u8>,
    pub abi: serde_json::Value,
}

#[derive(Debug)]
pub enum CompileError {
    Io(std::io::Error),
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

/// Lower `c` to Solidity, write it to disk, then compile it to EVM bytecode via
/// foundry-compilers (solc, standard-JSON `language: "Solidity"`). The
/// solc-generated Yul (IR) is dumped alongside for inspection.
pub fn compile_contract(c: &Contract) -> Result<Artifact, CompileError> {
    let sol = crate::solidity::lower_solidity(c);

    // Write the Solidity FIRST so a later solc failure still leaves it for
    // inspection.
    let dir = target_dir();
    let sol_path = dir.join(format!("{}.sol", c.name));
    std::fs::write(&sol_path, &sol)?;

    let source_name = format!("{}.sol", c.name);
    let input = serde_json::json!({
        "language": "Solidity",
        "sources": { source_name.clone(): { "content": sol } },
        "settings": {
            "outputSelection": { "*": { "*": ["evm.bytecode.object", "abi", "ir"] } },
            "optimizer": { "enabled": true }
        }
    });

    let version = Version::parse(SOLC_VERSION).expect("SOLC_VERSION is valid semver");
    let solc = Solc::find_or_install(&version)
        .map_err(|e| CompileError::Solc(format!("could not obtain solc: {e}")))?;

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
            return Err(CompileError::Solc(format!(
                "{}; inspect target/rustereum/{}.sol",
                fatal.join("\n"),
                c.name
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
