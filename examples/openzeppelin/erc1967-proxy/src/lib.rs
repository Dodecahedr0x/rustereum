//! ERC1967 proxy — inherits OpenZeppelin `ERC1967Proxy`. The constructor
//! forwards a runtime `implementation` address (identifier) and empty init data
//! (string literal `""`) to the base initializer via
//! `#[constructor(ERC1967Proxy(implementation, ""))]`.
//!
//! `ERC1967Proxy` requires the implementation to have code, so this crate also
//! defines a trivial standalone `Logic` contract to deploy and point at.

pub mod bindings;

use crate::bindings::ERC1967Proxy;
use rustereum::prelude::*;

/// Trivial standalone logic contract the proxy points at (needs real code).
#[contract]
pub struct Logic {
    value: u256,
}

#[contract]
impl Logic {
    pub fn get(&self) -> u256 {
        self.value
    }
}

#[contract]
pub struct MyProxy {}

#[contract]
impl ERC1967Proxy for MyProxy {}

#[contract]
impl MyProxy {
    // `implementation` is an identifier → references the constructor param
    // (camelCased). `""` is a string literal → empty init calldata, so the base
    // just writes the implementation slot without delegatecalling an initializer.
    #[constructor(ERC1967Proxy(implementation, ""))]
    pub fn new(implementation: Address) {}
}

#[cfg(test)]
mod tests {
    use super::{Logic, MyProxy};
    use rustereum::testing::InMemoryDB;
    use rustereum::vm::Address;

    #[test]
    fn erc1967_proxy_end_to_end() {
        let mut evm = InMemoryDB::default();

        // Deploy the real implementation so it has code on-chain.
        let logic_art = Logic::compile().expect("compile logic");
        let logic = Logic::deploy(&mut evm, &logic_art);

        // Deploy the proxy pointing at the implementation's address.
        let proxy_art = MyProxy::compile().expect("compile proxy");
        let proxy = MyProxy::deploy(&mut evm, &proxy_art, logic.address);

        // A non-zero address proves the ERC1967Proxy constructor accepted the
        // implementation (which has code) and deployment succeeded.
        assert_ne!(proxy.address, Address::ZERO);
    }
}
