//! Proc-macro attributes for `js_engine` GC integration.
//!
//! ## `#[gc_struct]` (re-exported from `js_engine` as `gc_struct`)
//!
//! Apply to a struct or enum definition to derive the correct GC traits for the
//! active JS engine backend.  The actual implementation is chosen at
//! compile time by `js_engine`:
//!
//! - **Boa** (`feature = "boa"`): `gc_struct_boa` emits
//!   `#[derive(boa_gc::Finalize, boa_gc::Trace, boa_engine::JsData)]`
//!   and translates `#[ignore_trace]` -> `#[unsafe_ignore_trace]`.
//! - **JSC / other** (`not(feature = "boa")`): `gc_struct_jsc` emits
//!   no-op `Trace`/`Finalize` impls and strips `#[ignore_trace]`.
//!
//! ## `#[ignore_trace]` (field-level)
//!
//! Marks a field as not participating in GC tracing.  On Boa this becomes
//! `#[unsafe_ignore_trace]` (consumed by `boa_gc::Trace` derive); on JSC
//! it is stripped (no GC tracing needed).  Only valid inside a `#[gc_struct]`.
//!
//! Usage:
//! ```ignore
//! use js_engine::gc_struct;
//!
//! #[gc_struct]
//! pub struct MyWidget {
//!     title: String,
//!     #[ignore_trace]
//!     callback: GcRootHandle<BoaTypes>,
//! }
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::{Item, parse_macro_input};

// Boa backend: replaces #[ignore_trace] with #[unsafe_ignore_trace]
fn transform_boa(fields: &mut syn::Fields) {
    fn transform_field(field: &mut syn::Field) {
        let mut new_attrs = Vec::new();
        for attr in field.attrs.drain(..) {
            if attr.path().is_ident("ignore_trace") {
                new_attrs.push(syn::parse_quote!(#[unsafe_ignore_trace]));
            } else {
                new_attrs.push(attr);
            }
        }
        field.attrs = new_attrs;
    }
    match fields {
        syn::Fields::Named(named) => {
            for field in named.named.iter_mut() {
                transform_field(field);
            }
        }
        syn::Fields::Unnamed(unnamed) => {
            for field in unnamed.unnamed.iter_mut() {
                transform_field(field);
            }
        }
        syn::Fields::Unit => {}
    }
}

// JSC: strips #[ignore_trace]
fn transform_jsc(fields: &mut syn::Fields) {
    match fields {
        syn::Fields::Named(named) => {
            for field in named.named.iter_mut() {
                field
                    .attrs
                    .retain(|attr| !attr.path().is_ident("ignore_trace"));
            }
        }
        syn::Fields::Unnamed(unnamed) => {
            for field in unnamed.unnamed.iter_mut() {
                field
                    .attrs
                    .retain(|attr| !attr.path().is_ident("ignore_trace"));
            }
        }
        syn::Fields::Unit => {}
    }
}

#[proc_macro_attribute]
pub fn gc_struct_boa(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as Item);
    match &mut input {
        Item::Struct(item_struct) => {
            transform_boa(&mut item_struct.fields);
            let attrs = &item_struct.attrs;
            let vis = &item_struct.vis;
            let ident = &item_struct.ident;
            let generics = &item_struct.generics;
            let fields = &item_struct.fields;
            let semi = &item_struct.semi_token;
            let expanded = quote! {
                #(#attrs)*
                #[derive(Clone, boa_gc::Finalize, boa_gc::Trace, boa_engine::JsData)]
                #vis struct #ident #generics #fields #semi
            };
            expanded.into()
        }
        Item::Enum(item_enum) => {
            // Transform fields in each variant
            for variant in &mut item_enum.variants {
                transform_boa(&mut variant.fields);
            }
            let attrs = &item_enum.attrs;
            let vis = &item_enum.vis;
            let ident = &item_enum.ident;
            let generics = &item_enum.generics;
            let variants = &item_enum.variants;
            let expanded = quote! {
                #(#attrs)*
                #[derive(Clone, boa_gc::Finalize, boa_gc::Trace)]
                #vis enum #ident #generics {
                    #variants
                }
            };
            expanded.into()
        }
        _ => syn::Error::new_spanned(
            &input,
            "#[gc_struct] can only be applied to structs and enums",
        )
        .to_compile_error()
        .into(),
    }
}

#[proc_macro_attribute]
pub fn gc_struct_jsc(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as Item);
    match &mut input {
        Item::Struct(item_struct) => {
            transform_jsc(&mut item_struct.fields);
            let attrs = &item_struct.attrs;
            let vis = &item_struct.vis;
            let ident = &item_struct.ident;
            let generics = &item_struct.generics;
            let fields = &item_struct.fields;
            let semi = &item_struct.semi_token;

            let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

            let expanded = quote! {
                #(#attrs)*
                #[derive(Clone)]
                #vis struct #ident #generics #fields #semi

                unsafe impl #impl_generics ::js_engine::gc::Trace for #ident #ty_generics #where_clause {}
                impl #impl_generics ::js_engine::gc::Finalize for #ident #ty_generics #where_clause {}
            };
            expanded.into()
        }
        Item::Enum(item_enum) => {
            // Strip #[ignore_trace] from variant fields
            for variant in &mut item_enum.variants {
                transform_jsc(&mut variant.fields);
            }
            let attrs = &item_enum.attrs;
            let vis = &item_enum.vis;
            let ident = &item_enum.ident;
            let generics = &item_enum.generics;
            let variants = &item_enum.variants;

            let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

            let expanded = quote! {
                #(#attrs)*
                #[derive(Clone)]
                #vis enum #ident #generics {
                    #variants
                }

                unsafe impl #impl_generics ::js_engine::gc::Trace for #ident #ty_generics #where_clause {}
                impl #impl_generics ::js_engine::gc::Finalize for #ident #ty_generics #where_clause {}
            };
            expanded.into()
        }
        _ => syn::Error::new_spanned(
            &input,
            "#[gc_struct] can only be applied to structs and enums",
        )
        .to_compile_error()
        .into(),
    }
}

/// Stub attribute: `#[ignore_trace]` is consumed by `gc_struct_boa`
/// and `gc_struct_jsc`.  On its own it is a no-op pass-through.
#[proc_macro_attribute]
pub fn ignore_trace(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
