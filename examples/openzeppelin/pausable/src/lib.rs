//! Pausable counter — inherits OpenZeppelin `Pausable`; `increment()` is gated by
//! the inherited `whenNotPaused` modifier. Compiling drops target/rustereum/Counter.sol.

pub mod bindings;

use crate::bindings::Pausable;
use rustereum::prelude::*;

#[contract]
pub struct Counter {
    count: u256,
}

#[contract]
impl Pausable for Counter {}

#[contract]
impl Counter {
    // `Pausable` has no constructor, so this contract needs none either.
    #[modifier(when_not_paused)]
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
    use rustereum::assemble_inheriting;
    use rustereum::driver::{compile_contract_with, CompileOptions};
    use rustereum::testing::InMemoryDB;
    use rustereum::vm::{DEPLOYER, U256};

    #[test]
    fn pausable_counter_end_to_end() {
        // The OZ sources aren't committed — `rustereum fetch` clones them into
        // this crate's own `lib/` and writes `remappings.txt` beside its
        // `rustereum.toml`, which is this project's compilation root.
        let opts = CompileOptions {
            project_root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        };
        let artifact =
            compile_contract_with(&assemble_inheriting::<Counter>(), &opts).expect("compile");

        let mut evm = InMemoryDB::default();
        let counter = Counter::deploy(&mut evm, &artifact);

        // Only the HAPPY path is exercised: the contract starts unpaused, so
        // `whenNotPaused` lets `increment()` through. We can't pause it — that
        // needs an internal `_pause()` call the DSL doesn't currently express.
        assert_eq!(counter.get(&mut evm), U256::from(0));
        counter.increment(&mut evm, DEPLOYER).unwrap();
        assert_eq!(counter.get(&mut evm), U256::from(1));
    }
}
