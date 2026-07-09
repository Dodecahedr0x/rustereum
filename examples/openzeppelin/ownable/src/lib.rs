//! Ownable counter — inherits OpenZeppelin `Ownable`; `increment()` is gated by
//! the inherited `onlyOwner` modifier. Compiling drops target/rustereum/Counter.sol.

pub mod bindings;

use crate::bindings::Ownable;
use rustereum::prelude::*;

#[contract]
pub struct Counter {
    count: u256,
}

#[contract]
impl Ownable for Counter {}

#[contract]
impl Counter {
    // The body is empty: `#[constructor(Ownable(initial_owner))]` forwards the
    // argument straight to the inherited `Ownable` constructor.
    #[constructor(Ownable(initial_owner))]
    #[allow(unused_variables)]
    pub fn new(initial_owner: Address) {}

    #[modifier(only_owner)]
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
    use alloy_primitives::{Address, U256};
    use rustereum::assemble_inheriting;
    use rustereum::driver::{compile_contract_with, CompileOptions};
    use rustereum::testing::{TestEvm, Token};

    #[test]
    fn ownable_counter_end_to_end() {
        let c = assemble_inheriting::<Counter>();
        // The OZ sources aren't committed — `rustereum fetch` (run in CI, or
        // locally) clones them into this crate's own `lib/` and writes
        // `remappings.txt` beside its `rustereum.toml`, which is this project's
        // compilation root.
        let opts = CompileOptions {
            project_root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        };
        let artifact = compile_contract_with(&c, &opts).expect("compile");

        // Owner is DISTINCT from the deployer ([0x11..]) so the test proves the
        // constructor arg — not the deployer — becomes the owner.
        let owner = Address::from([0x33u8; 20]);
        let stranger = Address::from([0x22u8; 20]);

        let mut evm = TestEvm::new();
        evm.fund(owner);
        evm.fund(stranger);
        let addr = evm.deploy_with(&artifact.bytecode, &[Token::Address(owner)]);

        assert_eq!(evm.call_u256(addr, "get()"), U256::from(0));
        evm.call_from(owner, addr, "increment()")
            .expect("owner should succeed");
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(1));

        // A non-owner (including the deployer [0x11..]) is rejected.
        assert!(evm.call_from(stranger, addr, "increment()").is_err());
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(1));
    }
}
