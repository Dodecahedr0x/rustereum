use rustereum::assemble_inheriting;
use rustereum::driver::{compile_contract_with, CompileOptions};
use rustereum::prelude::*;
use rustereum::testing::InMemoryDB;
use rustereum::vm::{Address, U256};

// Stand-in binding for OZ Ownable (as `rustereum add` would generate).
pub trait Ownable {
    const SOL_NAME: &'static str = "Ownable";
    const SOL_IMPORT: &'static str = "@openzeppelin/contracts/access/Ownable.sol";
}

#[contract]
struct Counter {
    count: u256,
}

#[contract]
impl Ownable for Counter {}

#[contract]
impl Counter {
    #[constructor(Ownable(initial_owner))]
    #[allow(unused_variables)]
    pub fn new(initial_owner: Address) {}

    #[modifier(only_owner)]
    pub fn increment(&mut self) {
        self.count += 1;
    }

    pub fn get(&self) -> u256 {
        self.count
    }
}

#[test]
fn ownable_access_control() {
    let opts = CompileOptions {
        project_root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/project"),
    };
    let artifact = compile_contract_with(&assemble_inheriting::<Counter>(), &opts).unwrap();

    let owner = Address::from([0x33u8; 20]); // distinct from DEPLOYER
    let stranger = Address::from([0x22u8; 20]);

    let mut evm = InMemoryDB::default();
    let counter = Counter::deploy(&mut evm, &artifact, owner);

    counter
        .increment(&mut evm, owner)
        .expect("owner should succeed");
    assert_eq!(counter.get(&mut evm), U256::from(1));

    assert!(counter.increment(&mut evm, stranger).is_err()); // only_owner rejects
    assert_eq!(counter.get(&mut evm), U256::from(1));
}
