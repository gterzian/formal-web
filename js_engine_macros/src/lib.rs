//! Proc-macro attributes for `js_engine` GC integration.
//!
//! ## `#[gc_struct]`
//!
//! Replaces the `impl_gc_traits! { ... }` declarative macro.  Apply to a
//! struct or enum definition to derive the correct GC traits for the
//! active JS engine backend:
//!
//! - **Boa** (`feature = "boa"`): emits `#[derive(boa_gc::Finalize,
//!   boa_gc::Trace, boa_engine::JsData)]` (structs) or
//!   `#[derive(boa_gc::Finalize, boa_gc::Trace)]` (enums, no JsData).
//! - **JSC / other** (`not(feature = "boa")`): emits no-op `Trace` and
//!   `Finalize` impls.
//!
//! Usage:
//! ```ignore
//! use js_engine::gc_struct;
//!
//! #[gc_struct]
//! pub struct MyWidget {
//!     title: String,
//!     visible: bool,
//! }
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::{Item, parse_macro_input};

/// Attribute macro: apply to a struct or enum to derive GC traits.
///
/// For structs, emits `JsData` so the type can be stored as a platform
/// object.  For enums, no `JsData` is emitted.
#[proc_macro_attribute]
pub fn gc_struct(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as Item);

    let expanded = match &input {
        Item::Struct(item_struct) => {
            let attrs = &item_struct.attrs;
            let vis = &item_struct.vis;
            let struct_token = &item_struct.struct_token;
            let ident = &item_struct.ident;
            let generics = &item_struct.generics;
            let fields = &item_struct.fields;
            let semi = &item_struct.semi_token;

            let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

            quote! {
                #(#attrs)*
                #[cfg_attr(
                    feature = "boa",
                    derive(boa_gc::Finalize, boa_gc::Trace, boa_engine::JsData)
                )]
                #vis #struct_token #ident #generics #fields #semi

                #[cfg(not(feature = "boa"))]
                unsafe impl #impl_generics ::js_engine::gc::Trace for #ident #ty_generics #where_clause {}

                #[cfg(not(feature = "boa"))]
                impl #impl_generics ::js_engine::gc::Finalize for #ident #ty_generics #where_clause {}
            }
        }

        Item::Enum(item_enum) => {
            let attrs = &item_enum.attrs;
            let vis = &item_enum.vis;
            let enum_token = &item_enum.enum_token;
            let ident = &item_enum.ident;
            let generics = &item_enum.generics;
            let variants = &item_enum.variants;

            let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

            quote! {
                #(#attrs)*
                #[cfg_attr(
                    feature = "boa",
                    derive(boa_gc::Finalize, boa_gc::Trace)
                )]
                #vis #enum_token #ident #generics {
                    #variants
                }

                #[cfg(not(feature = "boa"))]
                unsafe impl #impl_generics ::js_engine::gc::Trace for #ident #ty_generics #where_clause {}

                #[cfg(not(feature = "boa"))]
                impl #impl_generics ::js_engine::gc::Finalize for #ident #ty_generics #where_clause {}
            }
        }

        _ => {
            return syn::Error::new_spanned(
                &input,
                "#[gc_struct] can only be applied to structs and enums",
            )
            .to_compile_error()
            .into();
        }
    };

    expanded.into()
}
