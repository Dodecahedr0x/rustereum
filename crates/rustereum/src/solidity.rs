use crate::ir::{AssignOp, BinOp, Contract, Expr, Method, Place, Stmt, Type};

/// Lower a standalone (non-inheritance) `Contract` IR to Solidity source.
pub fn lower_solidity(c: &Contract) -> String {
    let mut out = String::new();
    out.push_str("// SPDX-License-Identifier: MIT\n");
    out.push_str("pragma solidity ^0.8.28;\n\n");
    out.push_str(&format!("contract {} {{\n", c.name));

    for field in &c.fields {
        out.push_str(&format!("    {} {};\n", sol_type(&field.ty), field.name));
    }

    for (i, method) in c.methods.iter().enumerate() {
        // Blank line between the fields block and the first method, and
        // between consecutive methods.
        if i > 0 || !c.fields.is_empty() {
            out.push('\n');
        }
        out.push_str(&lower_method(method));
    }

    out.push_str("}\n");
    out
}

fn sol_type(ty: &Type) -> &'static str {
    match ty {
        Type::U256 => "uint256",
        Type::Address => "address",
        Type::Bool => "bool",
    }
}

fn lower_method(m: &Method) -> String {
    let mut sig = format!("    function {}() public", m.name);
    if !m.mutates && m.ret.is_some() {
        sig.push_str(" view");
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
            format!("{} {} {};", f, op_str, lower_expr(value))
        }
        Stmt::Return(e) => format!("return {};", lower_expr(e)),
        Stmt::ExprStmt(e) => format!("{};", lower_expr(e)),
    }
}

fn lower_expr(e: &Expr) -> String {
    match e {
        Expr::Literal(n) => n.to_string(),
        Expr::StorageLoad(f) => f.clone(),
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
}
