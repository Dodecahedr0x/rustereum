//! ERC20 token — inherits OpenZeppelin `ERC20`. The token name/symbol are baked
//! into the base initializer as string literals via
//! `#[constructor(ERC20("MyToken", "MTK"))]`. Compiling drops
//! target/rustereum/MyToken.sol.

pub mod bindings;

use crate::bindings::ERC20;
use rustereum::prelude::*;

#[contract]
pub struct MyToken {}

#[contract]
impl ERC20 for MyToken {}

#[contract]
impl MyToken {
    // Name and symbol are string LITERALS forwarded verbatim to the inherited
    // ERC20 constructor. An empty contract with only a constructor is fine.
    #[constructor(ERC20("MyToken", "MTK"))]
    pub fn new() {}
}

#[cfg(test)]
mod tests {
    use super::MyToken;
    use rustereum::testing::InMemoryDB;

    #[test]
    fn erc20_end_to_end() {
        let artifact = MyToken::compile().expect("compile");

        // The full ERC20 surface is inherited into the ABI.
        let abi = artifact.abi.to_string();
        assert!(abi.contains("transfer"), "transfer should be inherited");
        assert!(abi.contains("balanceOf"), "balanceOf should be inherited");
        assert!(
            abi.contains("totalSupply"),
            "totalSupply should be inherited"
        );
        assert!(abi.contains("approve"), "approve should be inherited");

        let mut evm = InMemoryDB::default();
        let _token = MyToken::deploy(&mut evm, &artifact);
    }
}
