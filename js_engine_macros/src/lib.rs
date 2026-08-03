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
//! - **JSC / V8**: `gc_struct_jsc` emits
//!   no-op `Trace`/`Finalize` impls and strips `#[ignore_trace]`.
//!
//! ## `#[ignore_trace]` (field-level)
//!
//! Marks a field as not participating in GC tracing.  On Boa this becomes
//! `#[unsafe_ignore_trace]` (consumed by `boa_gc::Trace` derive); on JSC
//! it is stripped (persistent handles do not use tracing).  Only valid inside a `#[gc_struct]`.
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
use quote::{format_ident, quote};
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

/// Per-field metadata captured before `#[ignore_trace]` is stripped.
struct FieldInfo {
    /// Member access path within the struct/variant (`self.0`, `self.name`).
    access: syn::Member,
    /// Whether the field carried `#[ignore_trace]`.
    ignored: bool,
    /// Whether the field's type contains a `GcCell` (an adoption target).
    contains_gc_cell: bool,
}

fn collect_field_infos(fields: &syn::Fields) -> Vec<FieldInfo> {
    let is_skipped = |field: &syn::Field| {
        field
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("ignore_trace") || attr.path().is_ident("cfg"))
    };
    match fields {
        syn::Fields::Named(named) => named
            .named
            .iter()
            .map(|field| FieldInfo {
                access: syn::Member::Named(field.ident.clone().expect("named field")),
                ignored: is_skipped(field),
                contains_gc_cell: type_contains_gc_cell(&field.ty),
            })
            .collect(),
        syn::Fields::Unnamed(unnamed) => unnamed
            .unnamed
            .iter()
            .enumerate()
            .map(|(index, field)| FieldInfo {
                access: syn::Member::Unnamed(index.into()),
                ignored: is_skipped(field),
                contains_gc_cell: type_contains_gc_cell(&field.ty),
            })
            .collect(),
        syn::Fields::Unit => Vec::new(),
    }
}

/// True if the type path is `GcCell<...>` or a container (`Option`, `Vec`,
/// `VecDeque`, `Box`) whose contents contain a `GcCell` at any depth.
/// These are the fields whose managed edges must be re-pointed on
/// adoption.  Embedded composite fields (e.g. `branch1: Option<ReadableStream>`)
/// are deliberately NOT matched: their cells belong to other platform
/// objects and must keep their own owners.
fn type_contains_gc_cell(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Path(type_path) => {
            if let Some(last) = type_path.path.segments.last() {
                if last.ident == "GcCell" {
                    return true;
                }
                if matches!(
                    last.ident.to_string().as_str(),
                    "Option" | "Vec" | "VecDeque" | "Box"
                ) && let syn::PathArguments::AngleBracketed(args) = &last.arguments
                {
                    return args.args.iter().any(|arg| match arg {
                        syn::GenericArgument::Type(inner) => type_contains_gc_cell(inner),
                        _ => false,
                    });
                }
            }
            false
        }
        syn::Type::Reference(reference) => type_contains_gc_cell(&reference.elem),
        syn::Type::Paren(paren) => type_contains_gc_cell(&paren.elem),
        syn::Type::Tuple(tuple) => tuple.elems.iter().any(type_contains_gc_cell),
        _ => false,
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
            // Capture field metadata before #[ignore_trace] is stripped.
            let field_infos = collect_field_infos(&item_struct.fields);
            transform_jsc(&mut item_struct.fields);

            let attrs = &item_struct.attrs;
            let vis = &item_struct.vis;
            let ident = &item_struct.ident;
            let generics = &item_struct.generics;
            let fields = &item_struct.fields;
            let semi = &item_struct.semi_token;

            let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

            let trace_visits = field_infos.iter().filter(|info| !info.ignored).map(|info| {
                let access = &info.access;
                quote! {
                    ::js_engine::gc::GcTraceable::visit_js_values(&self.#access, visit);
                }
            });
            let owner_adopts = field_infos
                .iter()
                .filter(|info| !info.ignored && info.contains_gc_cell)
                .map(|info| {
                    let access = &info.access;
                    quote! {
                        ::js_engine::gc::GcOwner::adopt_gc_owner(&mut self.#access, owner);
                    }
                });

            let expanded = quote! {
                #(#attrs)*
                #[derive(Clone)]
                #vis struct #ident #generics #fields #semi

                unsafe impl #impl_generics ::js_engine::gc::Trace for #ident #ty_generics #where_clause {}
                impl #impl_generics ::js_engine::gc::Finalize for #ident #ty_generics #where_clause {}

                #[cfg(feature = "jsc")]
                impl #impl_generics ::js_engine::gc::GcTraceable for #ident #ty_generics #where_clause {
                    fn visit_js_values(&self, visit: &mut dyn FnMut(&::js_engine::jsc::JscValue)) {
                        #(#trace_visits)*
                    }
                }

                impl #impl_generics ::js_engine::gc::GcOwner for #ident #ty_generics #where_clause {
                    fn adopt_gc_owner(&mut self, owner: &::js_engine::gc::GcOwnerRef) {
                        #(#owner_adopts)*
                    }
                }
            };
            expanded.into()
        }
        Item::Enum(item_enum) => {
            // Capture per-variant field metadata before stripping.
            let variant_infos: Vec<Vec<FieldInfo>> = item_enum
                .variants
                .iter()
                .map(|variant| collect_field_infos(&variant.fields))
                .collect();
            for variant in item_enum.variants.iter_mut() {
                transform_jsc(&mut variant.fields);
            }
            let attrs = &item_enum.attrs;
            let vis = &item_enum.vis;
            let ident = &item_enum.ident;
            let generics = &item_enum.generics;
            let variants = &item_enum.variants;

            let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

            let trace_arms = variants.iter().zip(&variant_infos).map(|(variant, infos)| {
                let vident = &variant.ident;
                let non_ignored: Vec<&FieldInfo> =
                    infos.iter().filter(|info| !info.ignored).collect();
                match &variant.fields {
                    syn::Fields::Unit => quote! { Self::#vident => {} },
                    syn::Fields::Named(_) => {
                        let visited_names: Vec<&syn::Ident> = non_ignored
                            .iter()
                            .map(|info| match &info.access {
                                syn::Member::Named(name) => name,
                                syn::Member::Unnamed(_) => unreachable!(),
                            })
                            .collect();
                        let visits = non_ignored.iter().map(|info| match &info.access {
                            syn::Member::Named(name) => {
                                quote! { ::js_engine::gc::GcTraceable::visit_js_values(#name, visit); }
                            }
                            syn::Member::Unnamed(_) => unreachable!(),
                        });
                        quote! {
                            Self::#vident { #(#visited_names,)* .. } => { #(#visits)* }
                        }
                    }
                    syn::Fields::Unnamed(fields) => {
                        let total = fields.unnamed.len();
                        let bindings: Vec<proc_macro2::TokenStream> = (0..total)
                            .map(|index| {
                                if infos[index].ignored {
                                    quote! { _ }
                                } else {
                                    let binding = format_ident!("field{}", index);
                                    quote! { #binding }
                                }
                            })
                            .collect();
                        let visits = non_ignored.iter().map(|info| match &info.access {
                            syn::Member::Unnamed(index) => {
                                let binding = format_ident!("field{}", index.index);
                                quote! { ::js_engine::gc::GcTraceable::visit_js_values(#binding, visit); }
                            }
                            syn::Member::Named(_) => unreachable!(),
                        });
                        quote! {
                            Self::#vident(#(#bindings),*) => { #(#visits)* }
                        }
                    }
                }
            });

            let owner_arms = variants.iter().zip(&variant_infos).map(|(variant, infos)| {
                let vident = &variant.ident;
                let cell_fields: Vec<&FieldInfo> = infos
                    .iter()
                    .filter(|info| !info.ignored && info.contains_gc_cell)
                    .collect();
                match &variant.fields {
                    syn::Fields::Unit => quote! { Self::#vident => {} },
                    syn::Fields::Named(_) => {
                        let adopted_names: Vec<&syn::Ident> = cell_fields
                            .iter()
                            .map(|info| match &info.access {
                                syn::Member::Named(name) => name,
                                syn::Member::Unnamed(_) => unreachable!(),
                            })
                            .collect();
                        let adopts = cell_fields.iter().map(|info| match &info.access {
                            syn::Member::Named(name) => {
                                quote! { ::js_engine::gc::GcOwner::adopt_gc_owner(#name, owner); }
                            }
                            syn::Member::Unnamed(_) => unreachable!(),
                        });
                        quote! {
                            Self::#vident { #(#adopted_names,)* .. } => { #(#adopts)* }
                        }
                    }
                    syn::Fields::Unnamed(fields) => {
                        let total = fields.unnamed.len();
                        let bindings: Vec<proc_macro2::TokenStream> = (0..total)
                            .map(|index| {
                                if infos[index].ignored || !infos[index].contains_gc_cell {
                                    quote! { _ }
                                } else {
                                    let binding = format_ident!("field{}", index);
                                    quote! { #binding }
                                }
                            })
                            .collect();
                        let adopts = cell_fields.iter().map(|info| match &info.access {
                            syn::Member::Unnamed(index) => {
                                let binding = format_ident!("field{}", index.index);
                                quote! { ::js_engine::gc::GcOwner::adopt_gc_owner(#binding, owner); }
                            }
                            syn::Member::Named(_) => unreachable!(),
                        });
                        quote! {
                            Self::#vident(#(#bindings),*) => { #(#adopts)* }
                        }
                    }
                }
            });

            let expanded = quote! {
                #(#attrs)*
                #[derive(Clone)]
                #vis enum #ident #generics {
                    #variants
                }

                unsafe impl #impl_generics ::js_engine::gc::Trace for #ident #ty_generics #where_clause {}
                impl #impl_generics ::js_engine::gc::Finalize for #ident #ty_generics #where_clause {}

                #[cfg(feature = "jsc")]
                impl #impl_generics ::js_engine::gc::GcTraceable for #ident #ty_generics #where_clause {
                    fn visit_js_values(&self, visit: &mut dyn FnMut(&::js_engine::jsc::JscValue)) {
                        match self {
                            #(#trace_arms)*
                        }
                    }
                }

                impl #impl_generics ::js_engine::gc::GcOwner for #ident #ty_generics #where_clause {
                    fn adopt_gc_owner(&mut self, owner: &::js_engine::gc::GcOwnerRef) {
                        match self {
                            #(#owner_arms)*
                        }
                    }
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

/// Stub attribute: `#[ignore_trace]` is consumed by `gc_struct_boa`
/// and `gc_struct_jsc`.  On its own it is a no-op pass-through.
#[proc_macro_attribute]
pub fn ignore_trace(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
