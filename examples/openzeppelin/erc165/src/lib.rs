//! ERC165 introspection — inherits OpenZeppelin `ERC165`. `ERC165` has no
//! constructor, so this contract needs none either. Compiling drops
//! target/rustereum/MyIntrospection.sol.

pub mod bindings;

use crate::bindings::ERC165;
use rustereum::prelude::*;

#[contract]
pub struct MyIntrospection {}

#[contract]
impl ERC165 for MyIntrospection {}

#[contract]
impl MyIntrospection {}

#[cfg(test)]
mod tests {
    use super::MyIntrospection;
    use rustereum::testing::InMemoryDB;

    #[test]
    fn erc165_end_to_end() {
        let artifact = MyIntrospection::compile().expect("compile");

        // ERC165's introspection entrypoint is inherited into the ABI.
        let abi = artifact.abi.to_string();
        assert!(
            abi.contains("supportsInterface"),
            "supportsInterface should be inherited"
        );

        let mut evm = InMemoryDB::default();
        let _c = MyIntrospection::deploy(&mut evm, &artifact);
    }
}
