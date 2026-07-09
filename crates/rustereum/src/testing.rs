//! In-process EVM harness for end-to-end testing of compiled contracts.
//!
//! Compiled only when the `testing` feature is enabled, so it can be reused
//! from other crates' tests without pulling `revm` into normal builds.

use alloy_primitives::{Address, U256};
use revm::primitives::{AccountInfo, Bytes, ExecutionResult, Output, TxKind, U256 as RevmU256};
use revm::{Evm, InMemoryDB};

/// A fixed, funded externally-owned account used as the sender for every tx.
const CALLER: Address = Address::new([0x11; 20]);

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

    /// Deploy `bytecode` as init/creation code and return the contract address.
    pub fn deploy(&mut self, bytecode: &[u8]) -> Address {
        let result = self.run(TxKind::Create, Bytes::copy_from_slice(bytecode));
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
        let result = self.run(TxKind::Call(to), Self::calldata(sig));
        if !result.is_success() {
            panic!("call `{sig}` failed: {result:?}");
        }
    }

    /// Call `sig` on `to` and decode the 32-byte return value as a `U256`.
    pub fn call_u256(&mut self, to: Address, sig: &str) -> U256 {
        let result = self.run(TxKind::Call(to), Self::calldata(sig));
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
        let name = sig.split('(').next().unwrap_or(sig);
        let selector = crate::lower::selector(name, &[]);
        Bytes::copy_from_slice(&selector.to_be_bytes())
    }

    /// Execute one transaction against the persistent DB, committing state.
    fn run(&mut self, kind: TxKind, data: Bytes) -> ExecutionResult {
        Evm::builder()
            .with_db(&mut self.db)
            .modify_tx_env(|tx| {
                tx.caller = CALLER;
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
            fields: vec![Field {
                name: "count".into(),
                ty: Type::U256,
            }],
            methods: vec![
                Method {
                    name: "increment".into(),
                    mutates: true,
                    params: vec![],
                    ret: None,
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
                    body: vec![Stmt::Return(Expr::StorageLoad("count".into()))],
                },
            ],
        }
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
