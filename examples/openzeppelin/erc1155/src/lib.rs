//! ERC1155 multi-token — inherits OpenZeppelin `ERC1155`. The metadata URI is
//! baked into the base initializer as a string literal via
//! `#[constructor(ERC1155("https://example.com/{id}.json"))]`. Compiling drops
//! target/rustereum/MyMultiToken.sol.

pub mod bindings;

use crate::bindings::ERC1155;
use rustereum::prelude::*;

#[contract]
pub struct MyMultiToken {}

#[contract]
impl ERC1155 for MyMultiToken {}

#[contract]
impl MyMultiToken {
    #[constructor(ERC1155("https://example.com/{id}.json"))]
    pub fn new() {}
}

#[cfg(test)]
mod tests {
    use super::MyMultiToken;
    use rustereum::testing::InMemoryDB;

    #[test]
    fn erc1155_end_to_end() {
        let artifact = MyMultiToken::compile().expect("compile");

        // The core ERC1155 surface is inherited into the ABI.
        let abi = artifact.abi.to_string();
        assert!(abi.contains("balanceOf"), "balanceOf should be inherited");
        assert!(
            abi.contains("safeTransferFrom"),
            "safeTransferFrom should be inherited"
        );

        let mut evm = InMemoryDB::default();
        let _token = MyMultiToken::deploy(&mut evm, &artifact);
    }
}
