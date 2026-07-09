#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    U256,
    Address,
    Bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: Type,
}

/// A named, typed parameter for a method or constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
}

/// A base contract this contract inherits from, with its import path and the
/// argument expressions passed to its constructor (`Base(args...)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parent {
    pub name: String,
    pub import_path: String,
    pub base_args: Vec<String>,
}

/// A contract constructor with its parameters and body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constructor {
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    pub name: String,
    pub inherits: Vec<Parent>,
    pub fields: Vec<Field>,
    pub constructor: Option<Constructor>,
    pub methods: Vec<Method>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    pub name: String,
    pub mutates: bool,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    pub modifiers: Vec<String>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Assign {
        target: Place,
        op: AssignOp,
        value: Expr,
    },
    Return(Expr),
    ExprStmt(Expr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    Storage(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    StorageLoad(String),
    Literal(u64),
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinOp {
    Add,
}

pub trait ContractStorage {
    fn fields() -> Vec<Field>;
    fn name() -> String;
}

pub trait ContractMethods {
    fn methods() -> Vec<Method>;
    fn constructor() -> Option<Constructor> {
        None
    }
}

pub trait ContractInherits {
    fn parents() -> Vec<Parent>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_ir_describes_two_methods() {
        let c = counter_contract();
        assert_eq!(c.name, "Counter");
        assert_eq!(c.fields.len(), 1);
        assert_eq!(c.fields[0].name, "count");
        assert_eq!(c.methods.len(), 2);
        let inc = &c.methods[0];
        assert_eq!(inc.name, "increment");
        assert!(inc.mutates);
        let get = &c.methods[1];
        assert_eq!(get.name, "get");
        assert!(!get.mutates);
        assert!(matches!(get.ret, Some(Type::U256)));
    }

    // Hand-written IR for the counter — the reference value the whole
    // pipeline is built against before the macro exists.
    fn counter_contract() -> Contract {
        Contract {
            name: "Counter".into(),
            inherits: vec![],
            fields: vec![Field {
                name: "count".into(),
                ty: Type::U256,
            }],
            constructor: None,
            methods: vec![
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
            ],
        }
    }

    #[test]
    fn ir_supports_inheritance_and_constructor() {
        let c = Contract {
            name: "Counter".into(),
            inherits: vec![Parent {
                name: "Ownable".into(),
                import_path: "@openzeppelin/contracts/access/Ownable.sol".into(),
                base_args: vec!["initial_owner".into()],
            }],
            fields: vec![Field {
                name: "count".into(),
                ty: Type::U256,
            }],
            constructor: Some(Constructor {
                params: vec![Param {
                    name: "initial_owner".into(),
                    ty: Type::Address,
                }],
                body: vec![],
            }),
            methods: vec![Method {
                name: "increment".into(),
                params: vec![],
                mutates: true,
                ret: None,
                modifiers: vec!["onlyOwner".into()],
                body: vec![Stmt::Assign {
                    target: Place::Storage("count".into()),
                    op: AssignOp::Add,
                    value: Expr::Literal(1),
                }],
            }],
        };
        assert_eq!(c.inherits[0].name, "Ownable");
        assert_eq!(c.inherits[0].base_args, vec!["initial_owner".to_string()]);
        assert_eq!(c.constructor.as_ref().unwrap().params[0].ty, Type::Address);
        assert_eq!(c.methods[0].modifiers, vec!["onlyOwner".to_string()]);
    }
}
