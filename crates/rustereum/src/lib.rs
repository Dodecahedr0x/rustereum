pub mod driver;
pub mod ir;
pub mod solidity;
#[cfg(feature = "testing")]
pub mod testing;
pub mod vm;

pub mod prelude {
    pub use crate::u256;
    pub use crate::Address;
    pub use rustereum_macros::contract;
}

use crate::ir::{Contract, ContractInherits, ContractMethods, ContractStorage};

/// Assemble the full `Contract` IR for a `#[contract]` type from its
/// generated storage + methods traits.
pub fn assemble<T: ContractStorage + ContractMethods>() -> Contract {
    Contract {
        name: <T as ContractStorage>::name(),
        inherits: vec![],
        fields: <T as ContractStorage>::fields(),
        constructor: <T as ContractMethods>::constructor(),
        methods: <T as ContractMethods>::methods(),
    }
}

/// Assemble the full `Contract` IR for an inheriting `#[contract]` type,
/// merging the base-constructor argument bindings (`base_inits`) into the
/// parents declared via the trait impl.
pub fn assemble_inheriting<T: ContractStorage + ContractMethods + ContractInherits>() -> Contract {
    let mut inherits = <T as ContractInherits>::parents();
    let base = <T as ContractMethods>::base_inits();
    for p in inherits.iter_mut() {
        if let Some((_, args)) = base.iter().find(|(n, _)| n == &p.name) {
            p.base_args = args.clone();
        }
    }
    for (name, _) in &base {
        assert!(
            inherits.iter().any(|p| &p.name == name),
            "rustereum: #[constructor({name}(..))] names `{name}`, which is not inherited via `impl {name} for ...`"
        );
    }
    Contract {
        name: <T as ContractStorage>::name(),
        inherits,
        fields: <T as ContractStorage>::fields(),
        constructor: <T as ContractMethods>::constructor(),
        methods: <T as ContractMethods>::methods(),
    }
}

/// EVM address type. In a `#[contract]`, contract bodies are a DSL: this type
/// exists so constructor/method params typed `Address` type-check as native
/// Rust; the real on-chain semantics come from the generated Yul.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Address(pub alloy_primitives::Address);

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

// The `+` operator impls below exist so DSL bodies that use binary `+`
// (e.g. `a + b`, `self.total + 10`) type-check as native Rust. The real
// on-chain semantics come from the generated Yul (`add(...)`), not these.

// u256 + u256
impl core::ops::Add for u256 {
    type Output = u256;
    fn add(self, rhs: u256) -> u256 {
        u256(self.0 + rhs.0)
    }
}

// u256 + <integer literal> (literal infers to u64)
impl core::ops::Add<u64> for u256 {
    type Output = u256;
    fn add(self, rhs: u64) -> u256 {
        u256(self.0 + alloy_primitives::U256::from(rhs))
    }
}
