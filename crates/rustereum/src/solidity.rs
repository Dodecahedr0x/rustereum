use crate::ir::{AssignOp, BinOp, Constructor, Contract, Expr, Method, Parent, Place, Stmt, Type};

/// Lower a `Contract` IR to Solidity source, including inheritance
/// (imports + `is Parent`), an optional constructor with base initializers,
/// and per-method modifiers. rustereum identifiers (including modifier names)
/// are converted to camelCase; parent contract names are emitted verbatim.
pub fn lower_solidity(c: &Contract) -> String {
    let mut out = String::new();
    out.push_str("// SPDX-License-Identifier: MIT\n");
    out.push_str("pragma solidity ^0.8.28;\n\n");

    for parent in &c.inherits {
        out.push_str(&format!("import \"{}\";\n", parent.import_path));
    }
    if !c.inherits.is_empty() {
        out.push('\n');
    }

    out.push_str(&format!("contract {}", c.name));
    if !c.inherits.is_empty() {
        let parents: Vec<&str> = c.inherits.iter().map(|p| p.name.as_str()).collect();
        out.push_str(&format!(" is {}", parents.join(", ")));
    }
    out.push_str(" {\n");

    for field in &c.fields {
        out.push_str(&format!(
            "    {} {};\n",
            sol_type(&field.ty),
            to_camel_case(&field.name)
        ));
    }

    let mut needs_blank = !c.fields.is_empty();

    if let Some(ctor) = &c.constructor {
        if needs_blank {
            out.push('\n');
        }
        out.push_str(&lower_constructor(ctor, &c.inherits));
        needs_blank = true;
    }

    for method in &c.methods {
        // Blank line between the fields/constructor block and the first
        // method, and between consecutive methods.
        if needs_blank {
            out.push('\n');
        }
        out.push_str(&lower_method(method));
        needs_blank = true;
    }

    out.push_str("}\n");
    out
}

/// Split on `_`, keep the first segment as-is, capitalize the first letter of
/// each subsequent segment, and drop the underscores.
/// `initial_owner` -> `initialOwner`, `count` -> `count`.
fn to_camel_case(s: &str) -> String {
    let mut parts = s.split('_');
    let mut out = String::new();
    if let Some(first) = parts.next() {
        out.push_str(first);
    }
    for part in parts {
        let mut chars = part.chars();
        if let Some(c) = chars.next() {
            out.extend(c.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

fn lower_constructor(ctor: &Constructor, inherits: &[Parent]) -> String {
    let params: Vec<String> = ctor
        .params
        .iter()
        .map(|p| format!("{} {}", sol_type(&p.ty), to_camel_case(&p.name)))
        .collect();
    let mut sig = format!("    constructor({})", params.join(", "));

    for parent in inherits {
        if parent.base_args.is_empty() {
            continue;
        }
        // `base_args` are already rendered to their final Solidity form by the
        // macro (identifiers camelCased, literals verbatim), so emit as-is.
        sig.push_str(&format!(
            " {}({})",
            parent.name,
            parent.base_args.join(", ")
        ));
    }

    if ctor.body.is_empty() {
        sig.push_str(" {}\n");
    } else {
        sig.push_str(" {\n");
        for stmt in &ctor.body {
            sig.push_str(&format!("        {}\n", lower_stmt(stmt)));
        }
        sig.push_str("    }\n");
    }
    sig
}

fn sol_type(ty: &Type) -> &'static str {
    match ty {
        Type::U256 => "uint256",
        Type::Address => "address",
        Type::Bool => "bool",
    }
}

fn lower_method(m: &Method) -> String {
    let params: Vec<String> = m
        .params
        .iter()
        .map(|p| format!("{} {}", sol_type(&p.ty), to_camel_case(&p.name)))
        .collect();
    let mut sig = format!(
        "    function {}({}) public",
        to_camel_case(&m.name),
        params.join(", ")
    );
    if !m.mutates && m.ret.is_some() {
        sig.push_str(" view");
    }
    for modifier in &m.modifiers {
        // Modifiers are written snake_case in the DSL and camelCased here to
        // match the inherited Solidity modifier name (e.g. only_owner → onlyOwner).
        sig.push_str(&format!(" {}", to_camel_case(modifier)));
    }
    if let Some(ret) = &m.ret {
        sig.push_str(&format!(" returns ({})", sol_type(ret)));
    }
    sig.push_str(" {\n");

    for stmt in &m.body {
        sig.push_str(&format!("        {}\n", lower_stmt(stmt)));
    }

    sig.push_str("    }\n");
    sig
}

fn lower_stmt(s: &Stmt) -> String {
    match s {
        Stmt::Assign { target, op, value } => {
            let Place::Storage(f) = target;
            let op_str = match op {
                AssignOp::Add => "+=",
                AssignOp::Set => "=",
            };
            format!("{} {} {};", to_camel_case(f), op_str, lower_expr(value))
        }
        Stmt::Return(e) => format!("return {};", lower_expr(e)),
        Stmt::ExprStmt(e) => format!("{};", lower_expr(e)),
    }
}

fn lower_expr(e: &Expr) -> String {
    match e {
        Expr::Literal(n) => n.to_string(),
        Expr::StorageLoad(f) => to_camel_case(f),
        Expr::Binary { op, lhs, rhs } => match op {
            BinOp::Add => format!("{} + {}", lower_expr(lhs), lower_expr(rhs)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    fn counter() -> Contract {
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
    fn emits_solidity_contract() {
        let src = lower_solidity(&counter());
        assert!(src.contains("pragma solidity"));
        assert!(src.contains("contract Counter {"));
        assert!(src.contains("uint256 count;"));
        assert!(src.contains("function increment() public {"));
        assert!(src.contains("count += 1;"));
        assert!(src.contains("function get() public view returns (uint256) {"));
        assert!(src.contains("return count;"));
    }

    fn ownable_counter() -> Contract {
        Contract {
            name: "Counter".into(),
            inherits: vec![Parent {
                name: "Ownable".into(),
                import_path: "@openzeppelin/contracts/access/Ownable.sol".into(),
                base_args: vec!["initialOwner".into()],
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
            methods: vec![
                Method {
                    name: "increment".into(),
                    params: vec![],
                    mutates: true,
                    ret: None,
                    modifiers: vec!["only_owner".into()],
                    body: vec![Stmt::Assign {
                        target: Place::Storage("count".into()),
                        op: AssignOp::Add,
                        value: Expr::Literal(1),
                    }],
                },
                Method {
                    name: "get".into(),
                    params: vec![],
                    mutates: false,
                    ret: Some(Type::U256),
                    modifiers: vec![],
                    body: vec![Stmt::Return(Expr::StorageLoad("count".into()))],
                },
            ],
        }
    }

    #[test]
    fn emits_inheriting_contract() {
        let src = lower_solidity(&ownable_counter());
        assert!(src.contains(r#"import "@openzeppelin/contracts/access/Ownable.sol";"#));
        assert!(src.contains("contract Counter is Ownable {"));
        assert!(src.contains("constructor(address initialOwner) Ownable(initialOwner) {}"));
        assert!(src.contains("function increment() public onlyOwner {"));
        assert!(src.contains("count += 1;"));
        assert!(src.contains("function get() public view returns (uint256) {"));
    }

    #[test]
    fn camel_cases_identifiers() {
        // A field and param in snake_case become camelCase; the snake_case
        // modifier only_owner is camelCased to onlyOwner; parent names stay verbatim.
        let mut c = ownable_counter();
        c.fields[0].name = "total_count".into();
        c.methods[1].body = vec![Stmt::Return(Expr::StorageLoad("total_count".into()))];
        let src = lower_solidity(&c);
        assert!(src.contains("uint256 totalCount;"));
        assert!(src.contains("return totalCount;")); // body ref camelCased consistently
        assert!(src.contains("Ownable")); // parent name verbatim
        assert!(src.contains("onlyOwner")); // snake_case only_owner → camelCase onlyOwner
    }

    #[test]
    fn emits_literal_base_args() {
        // Base-initializer args that are literals (e.g. an ERC20 name/symbol)
        // are emitted verbatim into the Solidity base initializer.
        let c = Contract {
            name: "MyToken".into(),
            inherits: vec![Parent {
                name: "ERC20".into(),
                import_path: "@openzeppelin/contracts/token/ERC20/ERC20.sol".into(),
                base_args: vec![r#""MyToken""#.into(), r#""MTK""#.into()],
            }],
            fields: vec![],
            constructor: Some(Constructor {
                params: vec![],
                body: vec![],
            }),
            methods: vec![],
        };
        let src = lower_solidity(&c);
        assert!(src.contains("contract MyToken is ERC20 {"));
        assert!(
            src.contains(r#"constructor() ERC20("MyToken", "MTK") {}"#),
            "got: {src}"
        );
    }
}
