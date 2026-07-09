//! In-process EVM harness for end-to-end testing of compiled contracts.
//!
//! Compiled only when the `testing` feature is enabled, so it can be reused
//! from other crates' tests without pulling `revm` into normal builds.

use alloy_primitives::{Address, U256};
use revm::primitives::{AccountInfo, Bytes, ExecutionResult, Output, TxKind, U256 as RevmU256};
use revm::{Evm, InMemoryDB};
use tiny_keccak::{Hasher, Keccak};

/// A fixed, funded externally-owned account used as the sender for every tx.
const CALLER: Address = Address::new([0x11; 20]);

/// 4-byte function selector: the first 4 bytes of keccak256 over the canonical
/// signature (e.g. `"get()"`, which is already canonical for zero-arg fns).
fn selector(sig: &str) -> [u8; 4] {
    let mut hasher = Keccak::v256();
    hasher.update(sig.as_bytes());
    let mut out = [0u8; 32];
    hasher.finalize(&mut out);
    [out[0], out[1], out[2], out[3]]
}

/// An ABI argument value for constructor/call encoding. Only the two types the
/// tests need (YAGNI); each encodes to a single 32-byte head word.
pub enum Token {
    Address(Address),
    U256(U256),
}

/// ABI-encode a slice of tokens as concatenated 32-byte head words: an address
/// is left-padded to 32 bytes; a u256 is 32-byte big-endian.
fn encode_tokens(tokens: &[Token]) -> Vec<u8> {
    let mut out = Vec::with_capacity(tokens.len() * 32);
    for token in tokens {
        let mut word = [0u8; 32];
        match token {
            Token::Address(addr) => word[12..].copy_from_slice(addr.as_slice()),
            Token::U256(v) => word.copy_from_slice(&v.to_be_bytes::<32>()),
        }
        out.extend_from_slice(&word);
    }
    out
}

/// A tiny EVM sandbox backed by an in-memory database.
pub struct TestEvm {
    db: InMemoryDB,
}

impl Default for TestEvm {
    fn default() -> Self {
        Self::new()
    }
}

impl TestEvm {
    /// Build a fresh EVM with a single funded caller account.
    pub fn new() -> Self {
        let mut db = InMemoryDB::default();
        db.insert_account_info(
            CALLER,
            AccountInfo {
                balance: RevmU256::from(u128::MAX),
                ..Default::default()
            },
        );
        Self { db }
    }

    /// Insert a funded externally-owned account for `addr` (same mechanism
    /// `new()` uses for the default caller), so it can be used as a tx sender.
    pub fn fund(&mut self, addr: Address) {
        self.db.insert_account_info(
            addr,
            AccountInfo {
                balance: RevmU256::from(u128::MAX),
                ..Default::default()
            },
        );
    }

    /// Deploy `bytecode` as init/creation code and return the contract address.
    pub fn deploy(&mut self, bytecode: &[u8]) -> Address {
        self.deploy_with(bytecode, &[])
    }

    /// Deploy `bytecode` with ABI-encoded constructor `args` appended to the
    /// init code, and return the created contract address.
    pub fn deploy_with(&mut self, bytecode: &[u8], args: &[Token]) -> Address {
        let mut data = bytecode.to_vec();
        data.extend_from_slice(&encode_tokens(args));
        let result = self.run(CALLER, TxKind::Create, Bytes::from(data));
        match result {
            ExecutionResult::Success {
                output: Output::Create(_, Some(addr)),
                ..
            } => addr,
            other => panic!("deploy failed: {other:?}"),
        }
    }

    /// Call `sig` (e.g. `"increment()"`) on `to`, expecting success.
    pub fn call(&mut self, to: Address, sig: &str) {
        self.call_from(CALLER, to, sig)
            .unwrap_or_else(|()| panic!("call `{sig}` failed"));
    }

    /// Call `sig` on `to` from `caller`; return `Ok(())` on success and
    /// `Err(())` on revert/halt (does not panic, so callers can assert on a
    /// rejected access-controlled call).
    pub fn call_from(&mut self, caller: Address, to: Address, sig: &str) -> Result<(), ()> {
        let result = self.run(caller, TxKind::Call(to), Self::calldata(sig));
        match result {
            ExecutionResult::Success { .. } => Ok(()),
            _ => Err(()),
        }
    }

    /// Call `sig` on `to` and decode the 32-byte return value as a `U256`.
    pub fn call_u256(&mut self, to: Address, sig: &str) -> U256 {
        let result = self.run(CALLER, TxKind::Call(to), Self::calldata(sig));
        let output = match result {
            ExecutionResult::Success {
                output: Output::Call(bytes),
                ..
            } => bytes,
            other => panic!("call `{sig}` failed: {other:?}"),
        };
        assert_eq!(output.len(), 32, "expected 32-byte return, got {output:?}");
        U256::from_be_slice(&output)
    }

    /// Build calldata from a zero-arg signature: just the 4 selector bytes.
    fn calldata(sig: &str) -> Bytes {
        Bytes::copy_from_slice(&selector(sig))
    }

    /// Execute one transaction (from `caller`) against the persistent DB,
    /// committing state. Reverts/halts come back as normal `ExecutionResult`
    /// variants (only genuine EVM errors panic).
    fn run(&mut self, caller: Address, kind: TxKind, data: Bytes) -> ExecutionResult {
        Evm::builder()
            .with_db(&mut self.db)
            .modify_tx_env(|tx| {
                tx.caller = caller;
                tx.transact_to = kind;
                tx.data = data;
                tx.value = RevmU256::ZERO;
                tx.gas_limit = 30_000_000;
                tx.gas_price = RevmU256::ZERO;
            })
            .build()
            .transact_commit()
            .expect("evm execution error")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::compile_contract;
    use crate::ir::*;
    use alloy_primitives::U256;

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

    fn ownable_counter_ir() -> Contract {
        Contract {
            name: "Counter".into(),
            inherits: vec![Parent {
                name: "Ownable".into(),
                import_path: "@openzeppelin/contracts/access/Ownable.sol".into(),
                base_args: vec!["initial_owner".into()],
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
    fn ownable_counter_access_control() {
        use crate::driver::{compile_contract_with, CompileOptions};
        use alloy_primitives::{Address, U256};

        let c = ownable_counter_ir();
        let opts = CompileOptions {
            project_root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/project"),
        };
        let artifact = compile_contract_with(&c, &opts).unwrap();

        // Distinct from the fixed deployer/CALLER ([0x11; 20]), so the test
        // proves the constructor arg — not msg.sender — becomes the owner.
        let owner = Address::from([0x33u8; 20]);
        let stranger = Address::from([0x22u8; 20]);

        let mut evm = TestEvm::new();
        evm.fund(owner);
        evm.fund(stranger);
        let addr = evm.deploy_with(&artifact.bytecode, &[Token::Address(owner)]);

        // owner can increment
        evm.call_from(owner, addr, "increment()")
            .expect("owner should succeed");
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(1));

        // stranger is rejected by onlyOwner
        assert!(evm.call_from(stranger, addr, "increment()").is_err());
        // state unchanged
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(1));
    }

    #[test]
    fn counter_runs_in_evm() {
        let artifact = compile_contract(&counter()).unwrap();
        let mut evm = TestEvm::new();
        let addr = evm.deploy(&artifact.bytecode);
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(0));
        evm.call(addr, "increment()");
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(1));
        evm.call(addr, "increment()");
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(2));
    }
}
