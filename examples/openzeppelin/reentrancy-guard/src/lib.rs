//! Reentrancy-guarded counter — inherits OpenZeppelin `ReentrancyGuard`;
//! `increment()` is wrapped by the inherited `nonReentrant` modifier.
//! Compiling drops target/rustereum/Counter.sol.

pub mod bindings;

use crate::bindings::ReentrancyGuard;
use rustereum::prelude::*;

#[contract]
pub struct Counter {
    count: u256,
}

#[contract]
impl ReentrancyGuard for Counter {}

#[contract]
impl Counter {
    // `ReentrancyGuard` has no constructor, so this contract needs none either.
    #[modifier(nonReentrant)]
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
    fn reentrancy_guard_counter_end_to_end() {
        let c = assemble_inheriting::<Counter>();
        // The OZ sources aren't committed — `rustereum fetch` clones them into
        // `lib/` and writes `remappings.txt` in the SHARED `examples/openzeppelin`
        // folder (the parent of this crate), which is the compilation root.
        let opts = CompileOptions {
            project_root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .to_path_buf(),
        };
        let artifact = compile_contract_with(&c, &opts).expect("compile");

        let mut evm = TestEvm::new();
        let addr = evm.deploy(&artifact.bytecode);

        // A normal (non-reentrant) call passes straight through the guard.
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(0));
        evm.call(addr, "increment()");
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(1));
    }
}
