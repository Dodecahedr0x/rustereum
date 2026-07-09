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
    use rustereum::testing::InMemoryDB;
    use rustereum::vm::{Address, U256};

    #[test]
    fn ownable_counter_end_to_end() {
        // `Counter::compile()` uses this crate's dir as the project root, where
        // `rustereum fetch` cloned the OZ sources into `lib/` + `remappings.txt`.
        let artifact = Counter::compile().expect("compile");

        // Owner is DISTINCT from the deployer so the test proves the
        // constructor arg — not the deployer — becomes the owner.
        let owner = Address::from([0x33u8; 20]);
        let stranger = Address::from([0x22u8; 20]);

        let mut evm = InMemoryDB::default();
        let counter = Counter::deploy(&mut evm, &artifact, owner);

        assert_eq!(counter.get(&mut evm), U256::from(0));
        counter
            .increment(&mut evm, owner)
            .expect("owner should succeed");
        assert_eq!(counter.get(&mut evm), U256::from(1));

        // A non-owner is rejected by onlyOwner.
        assert!(counter.increment(&mut evm, stranger).is_err());
        assert_eq!(counter.get(&mut evm), U256::from(1));
    }
}
