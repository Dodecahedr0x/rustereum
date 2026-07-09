//! AccessControl counter — inherits OpenZeppelin `AccessControl`. This proves
//! `AccessControl` inherits, compiles and deploys (its `hasRole`/`grantRole`
//! surface in the ABI). Compiling drops target/rustereum/Counter.sol.

pub mod bindings;

use crate::bindings::AccessControl;
use rustereum::prelude::*;

#[contract]
pub struct Counter {
    count: u256,
}

#[contract]
impl AccessControl for Counter {}

#[contract]
impl Counter {
    // `AccessControl` has no constructor, so this contract needs none either.
    //
    // `increment()` carries NO modifier: the natural guard, `onlyRole(role)`,
    // takes a `bytes32` role argument the DSL can't express yet (modifiers are
    // referenced by name only, without arguments). So the counter simply
    // demonstrates that AccessControl is inherited and its role machinery is
    // present in the ABI.
    pub fn increment(&mut self) {
        self.count += 1;
    }

    pub fn get(&self) -> u256 {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::Counter;
    use rustereum::driver::CompileOptions;
    use rustereum::testing::InMemoryDB;
    use rustereum::vm::{DEPLOYER, U256};

    #[test]
    fn access_control_counter_end_to_end() {
        // The OZ sources aren't committed — `rustereum fetch` clones them into
        // this crate's own `lib/` and writes `remappings.txt` beside its
        // `rustereum.toml`, which is this project's compilation root.
        let opts = CompileOptions {
            project_root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        };
        let artifact = Counter::compile_with(&opts).expect("compile");

        // AccessControl's role machinery is inherited into the ABI.
        let abi = artifact.abi.to_string();
        assert!(abi.contains("hasRole"), "hasRole should be inherited");
        assert!(abi.contains("grantRole"), "grantRole should be inherited");

        let mut evm = InMemoryDB::default();
        let counter = Counter::deploy(&mut evm, &artifact);

        assert_eq!(counter.get(&mut evm), U256::from(0));
        counter.increment(&mut evm, DEPLOYER).unwrap();
        assert_eq!(counter.get(&mut evm), U256::from(1));
    }
}
