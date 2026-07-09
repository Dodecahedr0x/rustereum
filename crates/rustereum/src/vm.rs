//! A minimal EVM abstraction the generated contract clients call into, plus ABI
//! selector/encode/decode helpers. Kept free of revm so it compiles in every
//! build; the revm-backed `Vm` impl lives in the feature-gated `testing` module.

pub use alloy_primitives::{Address, U256};
use tiny_keccak::{Hasher, Keccak};

/// The fixed account that deploys contracts in tests (auto-funded by the Vm impl).
pub const DEPLOYER: Address = Address::new([0xde; 20]);

/// A reverted call: the returned revert data (may be empty).
#[derive(Debug, Clone, Default)]
pub struct Revert {
    pub data: Vec<u8>,
}

impl core::fmt::Display for Revert {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "call reverted ({} bytes)", self.data.len())
    }
}
impl std::error::Error for Revert {}

/// An in-process EVM the generated clients drive. Implemented for revm's
/// `InMemoryDB` in the `testing` module.
pub trait Vm {
    /// Deploy `init_code` from `deployer`; return the created contract address.
    /// Panics if creation fails (test harness).
    fn deploy_code(&mut self, deployer: Address, init_code: &[u8]) -> Address;
    /// Call `to` from `caller` with `data`; return output bytes, or `Revert` on
    /// revert/halt.
    fn call_raw(&mut self, caller: Address, to: Address, data: &[u8]) -> Result<Vec<u8>, Revert>;
}

/// 4-byte function selector: first 4 bytes of keccak256 over the canonical
/// signature (e.g. `"increment()"`, `"transfer(address,uint256)"`).
pub fn selector(sig: &str) -> [u8; 4] {
    let mut h = Keccak::v256();
    h.update(sig.as_bytes());
    let mut out = [0u8; 32];
    h.finalize(&mut out);
    [out[0], out[1], out[2], out[3]]
}

/// ABI head-word encoders (each value → one 32-byte word).
pub fn encode_address(a: Address) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(a.as_slice());
    w
}
pub fn encode_u256(v: U256) -> [u8; 32] {
    v.to_be_bytes::<32>()
}
pub fn encode_bool(b: bool) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[31] = b as u8;
    w
}

/// ABI decoders reading the first 32-byte word of `out`.
pub fn decode_u256(out: &[u8]) -> U256 {
    U256::from_be_slice(&out[..32])
}
pub fn decode_address(out: &[u8]) -> Address {
    Address::from_slice(&out[12..32])
}
pub fn decode_bool(out: &[u8]) -> bool {
    out[..32].iter().any(|&b| b != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selectors_match_known_values() {
        assert_eq!(u32::from_be_bytes(selector("increment()")), 0xd09de08a);
        assert_eq!(u32::from_be_bytes(selector("get()")), 0x6d4ce63c);
    }
    #[test]
    fn address_round_trips() {
        let a = Address::from([0x33u8; 20]);
        assert_eq!(decode_address(&encode_address(a)), a);
    }
    #[test]
    fn u256_round_trips() {
        let v = U256::from(12345u64);
        assert_eq!(decode_u256(&encode_u256(v)), v);
    }
    #[test]
    fn bool_round_trips() {
        assert!(decode_bool(&encode_bool(true)));
        assert!(!decode_bool(&encode_bool(false)));
    }
}
