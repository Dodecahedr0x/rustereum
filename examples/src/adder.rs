//! Adder — showcases binary `+` in a contract body.
//!
//! Stores a running total; `add_ten()` adds 10 via `self.total = self.total + 10`.

use rustereum::prelude::*;

#[contract]
pub struct Adder {
    total: u256,
}

#[contract]
impl Adder {
    // Written as explicit `+` (not `+=`) on purpose: this example exists to
    // exercise the binary `+` grammar end-to-end, so keep the addition visible.
    #[allow(clippy::assign_op_pattern)]
    pub fn add_ten(&mut self) {
        self.total = self.total + 10;
    }

    pub fn get(&self) -> u256 {
        self.total
    }
}

#[cfg(test)]
mod tests {
    use super::Adder;
    use alloy_primitives::U256;
    use rustereum::assemble;
    use rustereum::driver::compile_contract;
    use rustereum::testing::TestEvm;

    #[test]
    fn adder_end_to_end() {
        let contract = assemble::<Adder>();
        let artifact = compile_contract(&contract).expect("compile");
        assert!(artifact.yul_path.exists());

        let mut evm = TestEvm::new();
        let addr = evm.deploy(&artifact.bytecode);
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(0));
        evm.call(addr, "add_ten()");
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(10));
        evm.call(addr, "add_ten()");
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(20));
    }
}
