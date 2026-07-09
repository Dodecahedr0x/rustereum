use proc_macro::TokenStream;

/// Placeholder; real lowering is implemented in a later task.
#[proc_macro_attribute]
pub fn contract(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
