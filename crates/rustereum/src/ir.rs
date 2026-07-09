#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    U256,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Contract {
    pub name: String,
    pub fields: Vec<Field>,
    pub methods: Vec<Method>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Method {
    pub name: String,
    pub mutates: bool,
    pub params: Vec<(String, Type)>,
    pub ret: Option<Type>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Assign {
        target: Place,
        op: AssignOp,
        value: Expr,
    },
    Return(Expr),
    ExprStmt(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    Storage(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    StorageLoad(String),
    Literal(u64),
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignOp {
    Set,
    Add,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
    Add,
}

pub trait ContractStorage {
    fn fields() -> Vec<Field>;
    fn name() -> String;
}

pub trait ContractMethods {
    fn methods() -> Vec<Method>;
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
            fields: vec![Field {
                name: "count".into(),
                ty: Type::U256,
            }],
            methods: vec![
                Method {
                    name: "increment".into(),
                    mutates: true,
                    params: vec![],
                    ret: None,
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
                    body: vec![Stmt::Return(Expr::StorageLoad("count".into()))],
                },
            ],
        }
    }
}
