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
    #[modifier(non_reentrant)]
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
    use rustereum::testing::InMemoryDB;
    use rustereum::vm::{DEPLOYER, U256};

    #[test]
    fn reentrancy_guard_counter_end_to_end() {
        // `Counter::compile()` uses this crate's dir as the project root, where
        // `rustereum fetch` cloned the OZ sources into `lib/` + `remappings.txt`.
        let artifact = Counter::compile().expect("compile");

        let mut evm = InMemoryDB::default();
        let counter = Counter::deploy(&mut evm, &artifact);

        // A normal (non-reentrant) call passes straight through the guard.
        assert_eq!(counter.get(&mut evm), U256::from(0));
        counter.increment(&mut evm, DEPLOYER).unwrap();
        assert_eq!(counter.get(&mut evm), U256::from(1));
    }
}
