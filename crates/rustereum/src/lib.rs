pub mod driver; // implemented in a later task
pub mod ir; // implemented in a later task
pub mod lower; // implemented in a later task
#[cfg(feature = "testing")]
pub mod testing;

pub mod prelude {
    pub use crate::u256;
    pub use rustereum_macros::contract;
}

/// EVM 256-bit unsigned integer. In a `#[contract]`, contract bodies are a
/// DSL: this type exists so they type-check as native Rust; the real
/// semantics come from the generated Yul, not these operator impls.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct u256(pub alloy_primitives::U256);

impl From<u64> for u256 {
    fn from(v: u64) -> Self {
        u256(alloy_primitives::U256::from(v))
    }
}

// Enables `self.count += 1` (the integer literal infers to u64).
impl core::ops::AddAssign<u64> for u256 {
    fn add_assign(&mut self, rhs: u64) {
        self.0 += alloy_primitives::U256::from(rhs);
    }
}
