use rustereum::assemble_inheriting;
use rustereum::ir::{ContractInherits, ContractMethods, Type};
use rustereum::prelude::*;

// Stand-in for a generated binding (Task 9/10 generates these for real).
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
fn parents_and_import_path() {
    let parents = <Counter as ContractInherits>::parents();
    assert_eq!(parents.len(), 1);
    assert_eq!(parents[0].name, "Ownable");
    assert_eq!(
        parents[0].import_path,
        "@openzeppelin/contracts/access/Ownable.sol"
    );
}

#[test]
fn constructor_and_modifier_captured() {
    let ctor = <Counter as ContractMethods>::constructor().expect("ctor");
    assert_eq!(ctor.params.len(), 1);
    assert_eq!(ctor.params[0].name, "initial_owner");
    assert_eq!(ctor.params[0].ty, Type::Address);

    let methods = <Counter as ContractMethods>::methods();
    // constructor excluded from methods
    assert!(methods.iter().all(|m| m.name != "new"));
    let inc = methods.iter().find(|m| m.name == "increment").unwrap();
    assert_eq!(inc.modifiers, vec!["only_owner".to_string()]);
}

#[test]
fn assemble_merges_base_args() {
    let c = assemble_inheriting::<Counter>();
    assert_eq!(c.inherits.len(), 1);
    assert_eq!(c.inherits[0].base_args, vec!["initial_owner".to_string()]);
    assert_eq!(c.constructor.unwrap().params[0].ty, Type::Address);
}

// A contract that inherits `Ownable` but whose `#[constructor(Wrong(..))]`
// base-initializes a parent it does NOT inherit. `assemble_inheriting` must
// reject this rather than silently drop the base-init.
#[contract]
struct Mismatch {
    count: u256,
}

#[contract]
impl Ownable for Mismatch {}

#[contract]
impl Mismatch {
    #[constructor(Wrong(initial_owner))]
    pub fn new(initial_owner: Address) {}

    pub fn get(&self) -> u256 {
        self.count
    }
}

#[test]
#[should_panic(expected = "not inherited")]
fn base_init_naming_uninherited_parent_panics() {
    let _ = assemble_inheriting::<Mismatch>();
}
