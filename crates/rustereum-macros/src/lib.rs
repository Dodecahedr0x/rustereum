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
    if i.trait_.is_some() {
        expand_trait_impl(i)
    } else {
        expand_inherent_impl(i)
    }
}

/// A trait impl (`impl Ownable for Counter {}`) declares a single inherited
/// parent. The parent's `base_args` are left empty here and merged in later
/// from the `#[constructor(Parent(args...))]` on the inherent impl.
///
/// Limitation (v1): supports exactly ONE parent (a single trait impl).
/// Multiple parents would require accumulating across `ContractInherits`
/// impls, which is out of scope for this milestone.
fn expand_trait_impl(i: syn::ItemImpl) -> TokenStream {
    let self_ty = &i.self_ty;
    // `i.trait_` is `Some((bang, path, for))`; the path is the trait, e.g. `Ownable`.
    let trait_path = &i.trait_.as_ref().unwrap().1;
    quote! {
        #i
        impl ::rustereum::ir::ContractInherits for #self_ty {
            fn parents() -> Vec<::rustereum::ir::Parent> {
                vec![ ::rustereum::ir::Parent {
                    name: <#self_ty as #trait_path>::SOL_NAME.to_string(),
                    import_path: <#self_ty as #trait_path>::SOL_IMPORT.to_string(),
                    base_args: vec![],
                } ]
            }
        }
    }
    .into()
}

/// Lowered contents of an inherent `#[contract] impl`.
struct ImplLowering {
    methods: Vec<TokenStream2>,
    constructor: Option<TokenStream2>,
    base_inits: Vec<TokenStream2>,
}

fn expand_inherent_impl(i: syn::ItemImpl) -> TokenStream {
    let self_ty = i.self_ty.clone();
    let lowering = match build_methods(&i) {
        Ok(l) => l,
        Err(e) => {
            let err = e.to_compile_error();
            // Re-emit the original impl so downstream "no method named…"
            // errors don't cascade from the method bodies going missing.
            return quote! { #i #err }.into();
        }
    };

    // Re-emit the impl with the helper attributes (`#[modifier]`,
    // `#[constructor]`) stripped so rustc doesn't reject them as unknown
    // attributes. The bodies (e.g. `self.count += 1`) still compile as
    // native Rust against `u256`, while being lowered to IR below.
    let mut stripped = i.clone();
    for item in &mut stripped.items {
        if let syn::ImplItem::Fn(m) = item {
            m.attrs
                .retain(|a| !a.path().is_ident("modifier") && !a.path().is_ident("constructor"));
        }
    }

    let methods = &lowering.methods;
    let base_inits = &lowering.base_inits;
    let constructor = match &lowering.constructor {
        Some(c) => quote! { Some(#c) },
        None => quote! { None },
    };

    quote! {
        #stripped
        impl ::rustereum::ir::ContractMethods for #self_ty {
            fn methods() -> Vec<::rustereum::ir::Method> {
                vec![ #(#methods),* ]
            }
            fn constructor() -> Option<::rustereum::ir::Constructor> {
                #constructor
            }
            fn base_inits() -> Vec<(String, Vec<String>)> {
                vec![ #(#base_inits),* ]
            }
        }
    }
    .into()
}

fn build_methods(i: &syn::ItemImpl) -> Result<ImplLowering, syn::Error> {
    let mut lowering = ImplLowering {
        methods: Vec::new(),
        constructor: None,
        base_inits: Vec::new(),
    };
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

        // Collect helper attributes.
        let mut modifiers = Vec::new();
        let mut ctor_attr = None;
        for attr in &m.attrs {
            if attr.path().is_ident("modifier") {
                let ident: syn::Ident = attr.parse_args()?;
                modifiers.push(ident.to_string());
            } else if attr.path().is_ident("constructor") {
                ctor_attr = Some(parse_constructor_attr(attr)?);
            }
        }

        if let Some((parent, args)) = ctor_attr {
            // This method is the constructor: excluded from methods().
            let params = lower_params(m.sig.inputs.iter())?;
            let mut body = Vec::new();
            for stmt in &m.block.stmts {
                body.push(lower_stmt(stmt)?);
            }
            lowering.constructor = Some(quote! {
                ::rustereum::ir::Constructor {
                    params: vec![ #(#params),* ],
                    body: vec![ #(#body),* ],
                }
            });
            let arg_lits = args.iter().map(|a| quote! { #a.to_string() });
            lowering
                .base_inits
                .push(quote! { (#parent.to_string(), vec![ #(#arg_lits),* ]) });
        } else {
            lowering.methods.push(lower_method(m, &modifiers)?);
        }
    }
    Ok(lowering)
}

/// Parse `#[constructor(Parent(arg1, arg2, ...))]` into the parent name and
/// the list of argument identifiers.
fn parse_constructor_attr(attr: &syn::Attribute) -> Result<(String, Vec<String>), syn::Error> {
    let call: syn::ExprCall = attr.parse_args()?;
    let parent = match &*call.func {
        syn::Expr::Path(p) => p
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .ok_or_else(|| {
                syn::Error::new(
                    p.span(),
                    "rustereum: #[constructor(..)] needs a parent name",
                )
            })?,
        other => {
            return Err(syn::Error::new(
                other.span(),
                "rustereum: #[constructor(..)] must be of the form Parent(args...)",
            ))
        }
    };
    let mut args = Vec::new();
    for a in &call.args {
        match a {
            syn::Expr::Path(p) => {
                let id = p.path.segments.last().unwrap().ident.to_string();
                args.push(id);
            }
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    "rustereum: #[constructor(..)] arguments must be plain identifiers",
                ))
            }
        }
    }
    Ok((parent, args))
}

/// Lower a sequence of non-receiver function args to `ir::Param` literals.
fn lower_params<'a>(
    args: impl Iterator<Item = &'a syn::FnArg>,
) -> Result<Vec<TokenStream2>, syn::Error> {
    let mut out = Vec::new();
    for arg in args {
        match arg {
            syn::FnArg::Typed(pat_ty) => {
                let name = match &*pat_ty.pat {
                    syn::Pat::Ident(pi) => pi.ident.to_string(),
                    other => {
                        return Err(syn::Error::new(
                            other.span(),
                            "rustereum: parameters must be simple identifiers in v1",
                        ))
                    }
                };
                let ty = map_type(&pat_ty.ty)?;
                out.push(quote! {
                    ::rustereum::ir::Param { name: #name.to_string(), ty: #ty }
                });
            }
            syn::FnArg::Receiver(r) => {
                return Err(syn::Error::new(
                    r.span(),
                    "rustereum: unexpected self receiver",
                ))
            }
        }
    }
    Ok(out)
}

/// Map a Rust type to an `ir::Type` token by its last path segment.
fn map_type(ty: &syn::Type) -> Result<TokenStream2, syn::Error> {
    if let syn::Type::Path(tp) = ty {
        if let Some(seg) = tp.path.segments.last() {
            match seg.ident.to_string().as_str() {
                "u256" | "U256" => return Ok(quote! { ::rustereum::ir::Type::U256 }),
                "Address" => return Ok(quote! { ::rustereum::ir::Type::Address }),
                "bool" | "Bool" => return Ok(quote! { ::rustereum::ir::Type::Bool }),
                _ => {}
            }
        }
    }
    Err(syn::Error::new(
        ty.span(),
        "rustereum: unsupported parameter type; only u256, Address, bool in v1",
    ))
}

fn lower_method(m: &syn::ImplItemFn, modifiers: &[String]) -> Result<TokenStream2, syn::Error> {
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

    // Remaining args → typed params.
    let params = lower_params(inputs)?;

    // Return type.
    let ret = match &sig.output {
        syn::ReturnType::Default => quote! { None },
        syn::ReturnType::Type(_, ty) => {
            let mapped = map_type(ty)?;
            quote! { Some(#mapped) }
        }
    };

    // Body.
    let mut body = Vec::new();
    for stmt in &m.block.stmts {
        body.push(lower_stmt(stmt)?);
    }

    let modifier_lits = modifiers.iter().map(|s| quote! { #s.to_string() });

    Ok(quote! {
        ::rustereum::ir::Method {
            name: #name.to_string(),
            mutates: #mutates,
            params: vec![ #(#params),* ],
            ret: #ret,
            modifiers: vec![ #(#modifier_lits),* ],
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
