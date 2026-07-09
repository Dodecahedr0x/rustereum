use crate::ir::{AssignOp, BinOp, Contract, Expr, Method, Place, Stmt, Type};
use tiny_keccak::{Hasher, Keccak};

/// Compute the 4-byte EVM function selector for `name(params)`.
pub fn selector(name: &str, params: &[Type]) -> u32 {
    let sig = signature(name, params);
    let mut k = Keccak::v256();
    k.update(sig.as_bytes());
    let mut out = [0u8; 32];
    k.finalize(&mut out);
    u32::from_be_bytes([out[0], out[1], out[2], out[3]])
}

fn signature(name: &str, params: &[Type]) -> String {
    let types: Vec<&str> = params.iter().map(type_name).collect();
    format!("{}({})", name, types.join(","))
}

fn type_name(ty: &Type) -> &'static str {
    match ty {
        Type::U256 => "uint256",
    }
}

/// Lower a contract IR value into a Yul source string.
pub fn lower(c: &Contract) -> String {
    let mut runtime = String::new();
    runtime.push_str("            let selector := shr(224, calldataload(0))\n");
    runtime.push_str("            switch selector\n");
    for m in &c.methods {
        let sel = selector(&m.name, &param_types(m));
        let call = if m.ret.is_some() {
            format!("return_u256(fn_{}())", m.name)
        } else {
            format!("fn_{}() stop()", m.name)
        };
        runtime.push_str(&format!("            case 0x{:08x} {{ {} }}\n", sel, call));
    }
    runtime.push_str("            default { revert(0, 0) }\n\n");

    for m in &c.methods {
        runtime.push_str(&lower_method(c, m));
        runtime.push('\n');
    }
    runtime.push_str("            function return_u256(v) { mstore(0, v) return(0, 32) }\n");

    format!(
        r#"object "{name}" {{
    code {{
        datacopy(0, dataoffset("runtime"), datasize("runtime"))
        return(0, datasize("runtime"))
    }}
    object "runtime" {{
        code {{
{runtime}        }}
    }}
}}
"#,
        name = c.name,
        runtime = runtime,
    )
}

fn param_types(m: &Method) -> Vec<Type> {
    m.params.iter().map(|(_, ty)| ty.clone()).collect()
}

fn lower_method(c: &Contract, m: &Method) -> String {
    let mut body = String::new();
    for stmt in &m.body {
        body.push_str(&lower_stmt(c, stmt));
    }
    if m.ret.is_some() {
        format!("            function fn_{}() -> r {{ {}}}\n", m.name, body)
    } else {
        format!("            function fn_{}() {{ {}}}\n", m.name, body)
    }
}

fn lower_stmt(c: &Contract, stmt: &Stmt) -> String {
    match stmt {
        Stmt::Assign { target, op, value } => {
            let Place::Storage(f) = target;
            let slot = slot_of(c, f);
            let v = lower_expr(c, value);
            match op {
                AssignOp::Add => format!("sstore({slot}, add(sload({slot}), {v})) "),
                AssignOp::Set => format!("sstore({slot}, {v}) "),
            }
        }
        Stmt::Return(expr) => format!("r := {} ", lower_expr(c, expr)),
        Stmt::ExprStmt(expr) => format!("{} ", lower_expr(c, expr)),
    }
}

fn lower_expr(c: &Contract, expr: &Expr) -> String {
    match expr {
        Expr::Literal(n) => n.to_string(),
        Expr::StorageLoad(f) => format!("sload({})", slot_of(c, f)),
        Expr::Binary { op, lhs, rhs } => {
            let l = lower_expr(c, lhs);
            let r = lower_expr(c, rhs);
            match op {
                BinOp::Add => format!("add({l}, {r})"),
            }
        }
    }
}

fn slot_of(c: &Contract, field: &str) -> usize {
    c.fields
        .iter()
        .position(|f| f.name == field)
        .unwrap_or_else(|| panic!("unknown storage field: {field}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    fn counter() -> Contract {
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

    #[test]
    fn selector_matches_known_values() {
        assert_eq!(selector("increment", &[]), 0xd09de08a);
        assert_eq!(selector("get", &[]), 0x6d4ce63c);
    }

    #[test]
    fn yul_contains_object_dispatch_and_bodies() {
        let yul = lower(&counter());
        assert!(yul.contains(r#"object "Counter""#));
        assert!(yul.contains(r#"object "runtime""#));
        assert!(yul.contains("datacopy(0, dataoffset(\"runtime\")"));
        assert!(yul.contains("switch selector"));
        assert!(yul.contains("case 0xd09de08a"));
        assert!(yul.contains("case 0x6d4ce63c"));
        assert!(yul.contains("sstore(0, add(sload(0), 1))"));
        assert!(yul.contains("function fn_get() -> r"));
        assert!(yul.contains("sload(0)"));
    }
}
