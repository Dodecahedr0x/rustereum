use rustereum::prelude::*;
use rustereum::testing::InMemoryDB;
use rustereum::vm::{DEPLOYER, U256};

#[contract]
struct Counter {
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

#[test]
fn typed_client_deploys_and_calls() {
    let artifact = Counter::compile().unwrap();
    let mut evm = InMemoryDB::default();
    let counter = Counter::deploy(&mut evm, &artifact);
    assert_eq!(counter.get(&mut evm), U256::from(0));
    counter.increment(&mut evm, DEPLOYER).unwrap();
    assert_eq!(counter.get(&mut evm), U256::from(1));
    counter.increment(&mut evm, DEPLOYER).unwrap();
    assert_eq!(counter.get(&mut evm), U256::from(2));
}
