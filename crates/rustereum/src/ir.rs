//! The **intermediate representation** (IR) that sits between the `#[contract]`
//! macro and Solidity codegen.
//!
//! The macro doesn't emit Solidity directly. Instead it implements three traits
//! on your contract type — [`ContractStorage`], [`ContractMethods`], and (when a
//! parent is declared) [`ContractInherits`] — that describe the contract in
//! plain data. [`assemble`](crate::assemble) and friends reconstruct a single
//! [`Contract`] value from those trait impls, and
//! [`lower_solidity`](crate::solidity::lower_solidity) turns that [`Contract`]
//! into Solidity source.
//!
//! Everything here is deliberately small: the IR only models the
//! [supported language subset](crate#supported-language-subset) — [`u256`](crate::u256),
//! [`Address`](crate::Address), and `bool` types, struct-field storage,
//! `+`/`+=`/`=`, typed
//! methods, single inheritance, constructors with base initializers, and
//! modifiers.
//!
//! ```text
//! #[contract]  ──►  ContractStorage / ContractMethods / ContractInherits impls
//!                        │  (assemble / assemble_inheriting)
//!                        ▼
//!                     Contract  ──►  lower_solidity  ──►  Solidity source
//! ```

/// A scalar type usable for a storage [`Field`] or a [`Param`].
///
/// Each variant maps to a Solidity type: [`U256`](Type::U256) → `uint256`,
/// [`Address`](Type::Address) → `address`, [`Bool`](Type::Bool) → `bool`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// A 256-bit unsigned integer (`uint256`). Corresponds to the DSL
    /// [`u256`](crate::u256) type.
    U256,
    /// An EVM address (`address`). Corresponds to the DSL
    /// [`Address`](crate::Address) type.
    Address,
    /// A boolean (`bool`).
    Bool,
}

/// A storage variable: one field of the contract struct, becoming a Solidity
/// state variable of the corresponding [`Type`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The field name (snake_case; camelCased in the generated Solidity).
    pub name: String,
    /// The field's scalar type.
    pub ty: Type,
}

/// A named, typed parameter for a [`Method`] or [`Constructor`]. Becomes a
/// `<soltype> <camelCasedName>` entry in the generated Solidity signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// The parameter name (snake_case; camelCased in the generated Solidity).
    pub name: String,
    /// The parameter's scalar type.
    pub ty: Type,
}

/// A base contract this contract inherits from.
///
/// Lowered to an `import "<import_path>";` plus an `is <name>` clause, and — if
/// [`base_args`](Parent::base_args) is non-empty — a `<name>(args...)` base
/// initializer on the constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parent {
    /// The parent contract name, emitted verbatim (e.g. `Ownable`, `ERC20`).
    pub name: String,
    /// The Solidity import path resolved via `remappings.txt`
    /// (e.g. `@openzeppelin/contracts/access/Ownable.sol`).
    pub import_path: String,
    /// Arguments passed to the parent's constructor via `Base(args...)`. These
    /// are already rendered to their final Solidity form by the macro
    /// (identifiers camelCased, string/number literals verbatim). Populated from
    /// [`ContractMethods::base_inits`] during [`assemble_from`](crate::assemble_from).
    pub base_args: Vec<String>,
}

/// A contract constructor: its typed [`Param`]s and lowered body statements. The
/// base initializers themselves live on each [`Parent`], not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constructor {
    /// The constructor's parameters.
    pub params: Vec<Param>,
    /// The constructor body statements (currently always empty in the v1
    /// subset).
    pub body: Vec<Stmt>,
}

/// A complete contract in IR form — the single value
/// [`lower_solidity`](crate::solidity::lower_solidity) consumes. Produced by
/// [`assemble`](crate::assemble) / [`assemble_inheriting`](crate::assemble_inheriting)
/// from the macro-generated trait impls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    /// The contract (struct) name, used as the Solidity contract name and
    /// artifact file stem.
    pub name: String,
    /// Base contracts, in declaration order (single-parent in the v1 subset).
    pub inherits: Vec<Parent>,
    /// Storage variables (the struct fields).
    pub fields: Vec<Field>,
    /// The constructor, if the contract declares one.
    pub constructor: Option<Constructor>,
    /// The contract's public methods.
    pub methods: Vec<Method>,
}

/// A public contract method, lowered to a `public` Solidity function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    /// The method name (snake_case; camelCased in Solidity so ABI selectors are
    /// idiomatic).
    pub name: String,
    /// Whether the method mutates storage. `&mut self` methods set this;
    /// non-mutating (`&self`) methods with a return type are emitted `view`.
    pub mutates: bool,
    /// The method's parameters.
    pub params: Vec<Param>,
    /// The return type, if any.
    pub ret: Option<Type>,
    /// Inherited modifiers applied to the method, from `#[modifier(name)]`
    /// attributes (camelCased to match the Solidity modifier, e.g. `only_owner`
    /// → `onlyOwner`).
    pub modifiers: Vec<String>,
    /// The method body statements.
    pub body: Vec<Stmt>,
}

/// A single statement in a [`Method`] or [`Constructor`] body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// An assignment to a storage [`Place`], e.g. `self.count += 1`.
    Assign {
        /// The storage location being written.
        target: Place,
        /// The assignment operator (`=` or `+=`).
        op: AssignOp,
        /// The right-hand-side expression.
        value: Expr,
    },
    /// A `return <expr>;` statement.
    Return(Expr),
    /// A bare expression statement (`<expr>;`).
    ExprStmt(Expr),
}

/// An assignable location. Only storage fields are addressable in the v1 subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// A storage field, addressed by name (e.g. `count`).
    Storage(String),
}

/// An expression in a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// A read of a storage field by name (`self.field`).
    StorageLoad(String),
    /// An integer literal.
    Literal(u64),
    /// A binary operation (only `+` in the v1 subset).
    Binary {
        /// The binary operator.
        op: BinOp,
        /// The left operand.
        lhs: Box<Expr>,
        /// The right operand.
        rhs: Box<Expr>,
    },
}

/// The operator of an [`Stmt::Assign`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssignOp {
    /// Plain assignment (`=`).
    Set,
    /// Add-assign (`+=`).
    Add,
}

/// A binary operator in an [`Expr::Binary`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinOp {
    /// Addition (`+`).
    Add,
}

/// The storage half of a contract, generated by `#[contract]` on the struct.
///
/// Describes the contract's name and its state variables ([`Field`]s).
pub trait ContractStorage {
    /// The contract's storage fields, in declaration order.
    fn fields() -> Vec<Field>;
    /// The contract (struct) name.
    fn name() -> String;
}

/// The behaviour half of a contract, generated by `#[contract]` on the `impl`.
///
/// Describes the contract's [`Method`]s, its optional [`Constructor`], and the
/// base-constructor argument bindings.
pub trait ContractMethods {
    /// The contract's public methods.
    fn methods() -> Vec<Method>;
    /// The contract's constructor, if it declares one. Defaults to `None`.
    fn constructor() -> Option<Constructor> {
        None
    }
    /// Base-contract constructor argument bindings declared via
    /// `#[constructor(Parent(args...))]`, as `(parent_name, args)` pairs. Merged
    /// into the matching [`Parent::base_args`] by
    /// [`assemble_from`](crate::assemble_from). Defaults to empty.
    fn base_inits() -> Vec<(String, Vec<String>)> {
        vec![]
    }
}

/// The inheritance declaration of a contract, generated by `#[contract]` on an
/// `impl Parent for Contract {}` block. Its presence is what
/// [`assemble_inheriting`](crate::assemble_inheriting) keys off.
pub trait ContractInherits {
    /// The contract's base contracts (single-parent in the v1 subset). The
    /// returned [`Parent`]s have empty [`base_args`](Parent::base_args) until
    /// [`assemble_from`](crate::assemble_from) merges in
    /// [`ContractMethods::base_inits`].
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
