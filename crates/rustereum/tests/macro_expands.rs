use rustereum::ir::{AssignOp, ContractMethods, ContractStorage, Expr, Method, Place, Stmt, Type};
use rustereum::prelude::*;

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
fn macro_produces_expected_ir() {
    assert_eq!(<Counter as ContractStorage>::name(), "Counter");
    let fields = <Counter as ContractStorage>::fields();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "count");
    assert_eq!(fields[0].ty, Type::U256);

    let methods = <Counter as ContractMethods>::methods();
    assert_eq!(methods.len(), 2);
    assert_eq!(methods[0].name, "increment");
    assert!(methods[0].mutates);
    assert_eq!(methods[1].name, "get");
    assert!(!methods[1].mutates);
    assert_eq!(methods[1].ret, Some(Type::U256));
}

#[test]
fn macro_ir_matches_handwritten() {
    // The macro-generated methods must deep-equal the hand-written reference IR.
    let expected = vec![
        Method {
            name: "increment".into(),
            mutates: true,
            params: vec![],
            ret: None,
            modifiers: vec![],
            body: vec![Stmt::Assign {
                target: Place::Storage("count".into()),
                op: AssignOp::Add,
                value: Expr::Literal(1),
            }],
        },
        Method {
            name: "get".into(),
            mutates: false,
            params: vec![],
            ret: Some(Type::U256),
            modifiers: vec![],
            body: vec![Stmt::Return(Expr::StorageLoad("count".into()))],
        },
    ];
    assert_eq!(<Counter as ContractMethods>::methods(), expected);
}

#[test]
fn contract_impl_is_natively_callable() {
    // The re-emitted impl means the DSL bodies (`self.count += 1`,
    // `self.count`) also compile and run as native Rust.
    let mut c = Counter {
        count: u256::from(0u64),
    };
    c.increment();
    assert_eq!(c.get(), u256::from(1u64));
}
