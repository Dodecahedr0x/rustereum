pub mod driver; // implemented in a later task
pub mod ir; // implemented in a later task
pub mod lower; // implemented in a later task
pub mod testing; // implemented in a later task

pub mod prelude {
    pub use crate::u256;
    pub use rustereum_macros::contract;
}

#[allow(non_camel_case_types)]
pub type u256 = alloy_primitives::U256;
