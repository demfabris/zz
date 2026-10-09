use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

pub fn derive_render(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    let type_name = &ast.ident;
    let (impl_generics, type_generics, where_clause) = ast.generics.split_for_impl();

    let r#gen = quote! {
        impl #impl_generics zz_gpui::Render for #type_name #type_generics
        #where_clause
        {
            fn render(&mut self, _window: &mut zz_gpui::Window, _cx: &mut zz_gpui::Context<Self>) -> impl zz_gpui::Element {
                zz_gpui::Empty
            }
        }
    };

    r#gen.into()
}
