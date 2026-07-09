use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::spanned::Spanned;
use syn::{parse_macro_input, Expr, Item};

const UNSUPPORTED_EXPR: &str = "rustereum: unsupported expression in v1; only self.<field>, integer literals, and + are supported";

/// The `#[contract]` attribute macro. On a struct it generates a
/// `ContractStorage` impl; on an `impl` block it generates a
/// `ContractMethods` impl lowering each method to `::rustereum::ir` IR.
#[proc_macro_attribute]
pub fn contract(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as Item);
    match item {
        Item::Struct(s) => expand_struct(s),
        Item::Impl(i) => expand_impl(i),
        other => syn::Error::new(
            other.span(),
            "rustereum: #[contract] can only be applied to a struct or an impl block",
        )
        .to_compile_error()
        .into(),
    }
}

fn expand_struct(s: syn::ItemStruct) -> TokenStream {
    let name = &s.ident;
    let name_str = name.to_string();

    let fields = match build_fields(&s) {
        Ok(fields) => fields,
        Err(e) => {
            let err = e.to_compile_error();
            // Re-emit the original struct so the user's type still exists.
            return quote! { #s #err }.into();
        }
    };

    quote! {
        #s

        impl ::rustereum::ir::ContractStorage for #name {
            fn name() -> String {
                #name_str.to_string()
            }
            fn fields() -> Vec<::rustereum::ir::Field> {
                vec![ #(#fields),* ]
            }
        }
    }
    .into()
}

fn build_fields(s: &syn::ItemStruct) -> Result<Vec<TokenStream2>, syn::Error> {
    let named = match &s.fields {
        syn::Fields::Named(named) => &named.named,
        _ => {
            return Err(syn::Error::new(
                s.fields.span(),
                "rustereum: only structs with named fields are supported",
            ))
        }
    };

    let mut out = Vec::new();
    for f in named {
        if !is_u256(&f.ty) {
            return Err(syn::Error::new(
                f.ty.span(),
                "rustereum: unsupported field type; only u256 is supported in v1",
            ));
        }
        let fname = f.ident.as_ref().unwrap().to_string();
        out.push(quote! {
            ::rustereum::ir::Field { name: #fname.to_string(), ty: ::rustereum::ir::Type::U256 }
        });
    }
    Ok(out)
}

fn expand_impl(i: syn::ItemImpl) -> TokenStream {
    let self_ty = &i.self_ty;
    match build_methods(&i) {
        // The original impl is re-emitted unchanged so its methods are
        // natively callable and rust-analyzer works; its bodies (e.g.
        // `self.count += 1`) are a DSL that also compiles as native Rust
        // against `u256`, while the same bodies are lowered to IR (and
        // ultimately Yul) in the generated `ContractMethods` impl.
        Ok(methods) => quote! {
            #i
            impl ::rustereum::ir::ContractMethods for #self_ty {
                fn methods() -> Vec<::rustereum::ir::Method> {
                    vec![ #(#methods),* ]
                }
            }
        }
        .into(),
        Err(e) => {
            let err = e.to_compile_error();
            // Re-emit the original impl so downstream "no method named…"
            // errors don't cascade from the method bodies going missing.
            quote! { #i #err }.into()
        }
    }
}

fn build_methods(i: &syn::ItemImpl) -> Result<Vec<TokenStream2>, syn::Error> {
    let mut out = Vec::new();
    for item in &i.items {
        let m = match item {
            syn::ImplItem::Fn(m) => m,
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    "rustereum: only methods are supported inside a #[contract] impl in v1",
                ))
            }
        };
        out.push(lower_method(m)?);
    }
    Ok(out)
}

fn lower_method(m: &syn::ImplItemFn) -> Result<TokenStream2, syn::Error> {
    let sig = &m.sig;
    let name = sig.ident.to_string();

    // Receiver handling.
    let mut inputs = sig.inputs.iter();
    let receiver = match inputs.next() {
        Some(syn::FnArg::Receiver(r)) => r,
        _ => {
            return Err(syn::Error::new(
                sig.span(),
                "rustereum: methods must take a self receiver; associated functions are not supported in v1",
            ))
        }
    };
    let mutates = receiver.reference.is_some() && receiver.mutability.is_some();

    // No extra params allowed in v1.
    if let Some(param) = inputs.next() {
        return Err(syn::Error::new(
            param.span(),
            "rustereum: function parameters are not supported in v1",
        ));
    }

    // Return type.
    let ret = match &sig.output {
        syn::ReturnType::Default => quote! { None },
        syn::ReturnType::Type(_, ty) => {
            if is_u256(ty) {
                quote! { Some(::rustereum::ir::Type::U256) }
            } else {
                return Err(syn::Error::new(
                    ty.span(),
                    "rustereum: unsupported return type; only u256 is supported in v1",
                ));
            }
        }
    };

    // Body.
    let mut body = Vec::new();
    for stmt in &m.block.stmts {
        body.push(lower_stmt(stmt)?);
    }

    Ok(quote! {
        ::rustereum::ir::Method {
            name: #name.to_string(),
            mutates: #mutates,
            params: vec![],
            ret: #ret,
            body: vec![ #(#body),* ],
        }
    })
}

fn lower_stmt(stmt: &syn::Stmt) -> Result<TokenStream2, syn::Error> {
    match stmt {
        syn::Stmt::Expr(expr, semi) => lower_expr_stmt(expr, semi.is_some()),
        other => Err(syn::Error::new(other.span(), UNSUPPORTED_EXPR)),
    }
}

fn lower_expr_stmt(expr: &Expr, has_semi: bool) -> Result<TokenStream2, syn::Error> {
    match expr {
        Expr::Return(r) => {
            let inner = match &r.expr {
                Some(e) => lower_value(e)?,
                None => return Err(syn::Error::new(r.span(), UNSUPPORTED_EXPR)),
            };
            Ok(quote! { ::rustereum::ir::Stmt::Return(#inner) })
        }
        Expr::Assign(a) => {
            let target = lower_place(&a.left)?;
            let value = lower_value(&a.right)?;
            Ok(quote! {
                ::rustereum::ir::Stmt::Assign {
                    target: #target,
                    op: ::rustereum::ir::AssignOp::Set,
                    value: #value,
                }
            })
        }
        Expr::Binary(b) if matches!(b.op, syn::BinOp::AddAssign(_)) => {
            let target = lower_place(&b.left)?;
            let value = lower_value(&b.right)?;
            Ok(quote! {
                ::rustereum::ir::Stmt::Assign {
                    target: #target,
                    op: ::rustereum::ir::AssignOp::Add,
                    value: #value,
                }
            })
        }
        _ if !has_semi => {
            // Trailing tail expression → return value.
            let value = lower_value(expr)?;
            Ok(quote! { ::rustereum::ir::Stmt::Return(#value) })
        }
        _ => {
            let value = lower_value(expr)?;
            Ok(quote! { ::rustereum::ir::Stmt::ExprStmt(#value) })
        }
    }
}

fn lower_place(expr: &Expr) -> Result<TokenStream2, syn::Error> {
    let field = self_field(expr)?;
    Ok(quote! { ::rustereum::ir::Place::Storage(#field.to_string()) })
}

fn lower_value(expr: &Expr) -> Result<TokenStream2, syn::Error> {
    match expr {
        Expr::Lit(lit) => {
            if let syn::Lit::Int(i) = &lit.lit {
                let n: u64 = i.base10_parse()?;
                Ok(quote! { ::rustereum::ir::Expr::Literal(#n) })
            } else {
                Err(syn::Error::new(lit.span(), UNSUPPORTED_EXPR))
            }
        }
        Expr::Field(_) => {
            let field = self_field(expr)?;
            Ok(quote! { ::rustereum::ir::Expr::StorageLoad(#field.to_string()) })
        }
        Expr::Binary(b) if matches!(b.op, syn::BinOp::Add(_)) => {
            let lhs = lower_value(&b.left)?;
            let rhs = lower_value(&b.right)?;
            Ok(quote! {
                ::rustereum::ir::Expr::Binary {
                    op: ::rustereum::ir::BinOp::Add,
                    lhs: Box::new(#lhs),
                    rhs: Box::new(#rhs),
                }
            })
        }
        other => Err(syn::Error::new(other.span(), UNSUPPORTED_EXPR)),
    }
}

/// Returns the field name if `expr` is exactly `self.<field>`.
fn self_field(expr: &Expr) -> Result<String, syn::Error> {
    if let Expr::Field(fe) = expr {
        if let Expr::Path(p) = &*fe.base {
            if p.path.is_ident("self") {
                if let syn::Member::Named(id) = &fe.member {
                    return Ok(id.to_string());
                }
            }
        }
    }
    Err(syn::Error::new(expr.span(), UNSUPPORTED_EXPR))
}

/// Matches a type by its last path segment being `u256` or `U256`.
fn is_u256(ty: &syn::Type) -> bool {
    if let syn::Type::Path(tp) = ty {
        if let Some(seg) = tp.path.segments.last() {
            let id = seg.ident.to_string();
            return id == "u256" || id == "U256";
        }
    }
    false
}
