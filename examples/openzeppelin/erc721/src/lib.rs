//! ERC721 NFT — inherits OpenZeppelin `ERC721`. The collection name/symbol are
//! baked into the base initializer as string literals via
//! `#[constructor(ERC721("MyNFT", "NFT"))]`. Compiling drops
//! target/rustereum/MyNft.sol.

pub mod bindings;

use crate::bindings::ERC721;
use rustereum::prelude::*;

#[contract]
pub struct MyNft {}

#[contract]
impl ERC721 for MyNft {}

#[contract]
impl MyNft {
    #[constructor(ERC721("MyNFT", "NFT"))]
    pub fn new() {}
}

#[cfg(test)]
mod tests {
    use super::MyNft;
    use rustereum::testing::InMemoryDB;

    #[test]
    fn erc721_end_to_end() {
        let artifact = MyNft::compile().expect("compile");

        // The core ERC721 surface is inherited into the ABI.
        let abi = artifact.abi.to_string();
        assert!(abi.contains("ownerOf"), "ownerOf should be inherited");
        assert!(abi.contains("balanceOf"), "balanceOf should be inherited");
        assert!(
            abi.contains("transferFrom"),
            "transferFrom should be inherited"
        );

        let mut evm = InMemoryDB::default();
        let _nft = MyNft::deploy(&mut evm, &artifact);
    }
}
