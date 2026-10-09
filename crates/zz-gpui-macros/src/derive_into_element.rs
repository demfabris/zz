use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

pub fn derive_into_element(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    let type_name = &ast.ident;
    let (impl_generics, type_generics, where_clause) = ast.generics.split_for_impl();

    let r#gen = quote! {
        impl #impl_generics zz_gpui::IntoElement for #type_name #type_generics
        #where_clause
        {
            type Element = zz_gpui::ViewElement<Self>;

            #[track_caller]
            fn into_element(self) -> Self::Element {
                zz_gpui::ViewElement::new(self)
            }

            #[track_caller]
            #[inline(never)]
            fn into_any_element(self) -> zz_gpui::AnyElement {
                zz_gpui::Element::into_any(self.into_element())
            }
        }
    };

    r#gen.into()
}
