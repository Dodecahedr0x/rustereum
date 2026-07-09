//! Counter — the canonical rustereum example.
//!
//! A single `u256` storage variable with an `increment()` mutator and a
//! `get()` view. Compiling this drops an inspectable `target/rustereum/Counter.yul`.

use rustereum::prelude::*;

#[contract]
pub struct Counter {
    count: u256,
}

#[contract]
impl Counter {
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
    fn counter_end_to_end() {
        let art = Counter::compile().expect("compile");
        let mut evm = InMemoryDB::default();
        let counter = Counter::deploy(&mut evm, &art);
        assert_eq!(counter.get(&mut evm), U256::from(0));
        counter.increment(&mut evm, DEPLOYER).unwrap();
        assert_eq!(counter.get(&mut evm), U256::from(1));
        counter.increment(&mut evm, DEPLOYER).unwrap();
        assert_eq!(counter.get(&mut evm), U256::from(2));
    }
}
