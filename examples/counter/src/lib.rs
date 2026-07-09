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
    use alloy_primitives::U256;
    use rustereum::assemble;
    use rustereum::driver::compile_contract;
    use rustereum::testing::TestEvm;

    #[test]
    fn counter_end_to_end() {
        let contract = assemble::<Counter>();
        let artifact = compile_contract(&contract).expect("compile");

        // Inspectable Yul artifact is written to target/rustereum/.
        assert!(artifact.yul_path.exists());

        let mut evm = TestEvm::new();
        let addr = evm.deploy(&artifact.bytecode);
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(0));
        evm.call(addr, "increment()");
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(1));
        evm.call(addr, "increment()");
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(2));
    }
}
