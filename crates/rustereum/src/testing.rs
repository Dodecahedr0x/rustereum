//! In-process EVM harness for end-to-end testing of compiled contracts.
//!
//! This module provides the `revm`-backed implementation of the
//! [`Vm`] trait that the generated test clients drive. It is
//! compiled only when the `testing` feature is enabled, which keeps `revm`
//! optional — the abstraction in [`vm`](crate::vm) and the generated client
//! itself build without it — while still letting other crates' tests reuse the
//! harness by turning the feature on.
//!
//! The whole surface is: revm's [`InMemoryDB`] (re-exported so callers don't
//! depend on `revm` directly) and the [`Vm`] impl on it below.
//! The impl auto-funds any caller/deployer that isn't already present, so tests
//! can pick arbitrary addresses without setting up balances.

use alloy_primitives::Address;
use revm::primitives::{AccountInfo, Bytes, ExecutionResult, Output, TxKind, U256 as RevmU256};
use revm::Evm;

/// revm's in-memory state database, re-exported as the concrete
/// [`Vm`] used in tests. Construct one with
/// `InMemoryDB::default()`.
pub use revm::InMemoryDB;

use crate::vm::{Revert, Vm};

/// Fund `addr` with a large balance only if it is not already present, so
/// existing accounts (and their nonces) are preserved across calls — nonces
/// must persist for correct CREATE address derivation.
fn fund_if_absent(db: &mut InMemoryDB, addr: Address) {
    if !db.accounts.contains_key(&addr) {
        db.insert_account_info(
            addr,
            AccountInfo {
                balance: RevmU256::from(u128::MAX),
                ..Default::default()
            },
        );
    }
}

/// Build and commit one transaction against the persistent DB (gas limit 30M,
/// gas price 0, value 0).
fn run_tx(db: &mut InMemoryDB, caller: Address, kind: TxKind, data: Bytes) -> ExecutionResult {
    Evm::builder()
        .with_db(db)
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

/// Drives compiled contracts against a persistent [`InMemoryDB`]: deploys via a
/// `CREATE` transaction and calls via a `CALL` transaction, each caller/deployer
/// auto-funded on first use. State is committed to the DB, so successive calls
/// see prior storage and nonce changes.
impl Vm for InMemoryDB {
    fn deploy_code(&mut self, deployer: Address, init_code: &[u8]) -> Address {
        fund_if_absent(self, deployer);
        let result = run_tx(
            self,
            deployer,
            TxKind::Create,
            Bytes::copy_from_slice(init_code),
        );
        match result {
            ExecutionResult::Success {
                output: Output::Create(_, Some(addr)),
                ..
            } => addr,
            other => panic!("deploy failed: {other:?}"),
        }
    }

    fn call_raw(&mut self, caller: Address, to: Address, data: &[u8]) -> Result<Vec<u8>, Revert> {
        fund_if_absent(self, caller);
        let result = run_tx(self, caller, TxKind::Call(to), Bytes::copy_from_slice(data));
        match result {
            ExecutionResult::Success {
                output: Output::Call(bytes),
                ..
            } => Ok(bytes.to_vec()),
            ExecutionResult::Success { .. } => Ok(Vec::new()),
            ExecutionResult::Revert { output, .. } => Err(Revert {
                data: output.to_vec(),
            }),
            ExecutionResult::Halt { .. } => Err(Revert::default()),
        }
    }
}
