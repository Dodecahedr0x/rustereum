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
    use alloy_primitives::U256;
    use rustereum::assemble_inheriting;
    use rustereum::driver::{compile_contract_with, CompileOptions};
    use rustereum::testing::TestEvm;

    #[test]
    fn access_control_counter_end_to_end() {
        let c = assemble_inheriting::<Counter>();
        // The OZ sources aren't committed — `rustereum fetch` clones them into
        // this crate's own `lib/` and writes `remappings.txt` beside its
        // `rustereum.toml`, which is this project's compilation root.
        let opts = CompileOptions {
            project_root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        };
        let artifact = compile_contract_with(&c, &opts).expect("compile");

        // AccessControl's role machinery is inherited into the ABI.
        let abi = artifact.abi.to_string();
        assert!(abi.contains("hasRole"), "hasRole should be inherited");
        assert!(abi.contains("grantRole"), "grantRole should be inherited");

        let mut evm = TestEvm::new();
        let addr = evm.deploy(&artifact.bytecode);

        assert_eq!(evm.call_u256(addr, "get()"), U256::from(0));
        evm.call(addr, "increment()");
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(1));
    }
}
