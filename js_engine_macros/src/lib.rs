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

/// Per-field metadata captured before `#[ignore_trace]` is stripped.
struct FieldInfo {
    /// Member access path within the struct/variant (`self.0`, `self.name`).
    access: syn::Member,
    /// Whether the field carried `#[ignore_trace]` (or a `cfg`).
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
/// These are the fields whose managed edges must be re-pointed on adoption.
/// Embedded composite fields are deliberately not matched: their cells belong
/// to other platform objects and keep their own owners.
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

// ═══════════════════════════════════════════════════════════════════════════
// V8 backend — cppgc tracing
// ═══════════════════════════════════════════════════════════════════════════
//
// `#[gc_struct]` types on V8 implement:
// - `::js_engine::gc::Trace` — the generic trace trait with a field-walking
//   trace method (mirroring the `boa_gc::Trace` derive). Used both when the
//   value is traced inside a heap object (cell contents, nested domain
//   structs) and for the backend-independent `Trace` bounds.
// - `::js_engine::v8_gc::GarbageCollected` — heap-object trait, used when the
//   value is allocated on the cppgc heap (platform objects); its trace
//   delegates to the field-walking `Trace`.
// - `::js_engine::gc::Finalize` — the generic lifecycle marker.
//
// Every non-`#[ignore_trace]` field is visited during marking, and each
// generic parameter that appears in a traced field must itself be `Trace`.

fn v8_field_is_ignored(field: &syn::Field) -> bool {
    field.attrs.iter().any(|attr| {
        attr.path().is_ident("ignore_trace")
            // Fields gated behind `cfg`/`cfg_attr` are engine-specific state
            // (e.g. the boa-only wasm state) and may not exist on V8; they are
            // not traced.
            || attr.path().is_ident("cfg")
            || attr.path().is_ident("cfg_attr")
    })
}

/// Whether the type token stream mentions the given type parameter.
fn v8_type_mentions_param(ty: &syn::Type, param: &syn::Ident) -> bool {
    let type_string = quote::quote!(#ty).to_string();
    type_string
        .split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .any(|word| *param == word)
}

/// Where-clause for the V8 tracing impls: the item's own predicates plus
/// `T: ::js_engine::gc::Trace` for every type parameter that appears in a
/// traced (non-`#[ignore_trace]`) field. Parameters used only in ignored
/// fields get no bound.
fn v8_traced_where_clause(
    generics: &syn::Generics,
    traced_field_types: &[syn::Type],
) -> Option<syn::WhereClause> {
    let mut where_clause = generics
        .where_clause
        .clone()
        .unwrap_or_else(|| syn::WhereClause {
            where_token: Default::default(),
            predicates: Default::default(),
        });
    for type_param in generics.type_params() {
        let mentions = traced_field_types
            .iter()
            .any(|ty| v8_type_mentions_param(ty, &type_param.ident));
        if mentions {
            let ident = &type_param.ident;
            where_clause
                .predicates
                .push(syn::parse_quote!(#ident: ::js_engine::gc::Trace));
        }
    }
    Some(where_clause)
}

/// The types of the non-ignored fields, used for generic bound generation.
fn v8_traced_field_types(fields: &syn::Fields) -> Vec<syn::Type> {
    let mut types = Vec::new();
    match fields {
        syn::Fields::Named(named) => {
            for field in named.named.iter() {
                if !v8_field_is_ignored(field) {
                    types.push(field.ty.clone());
                }
            }
        }
        syn::Fields::Unnamed(unnamed) => {
            for field in unnamed.unnamed.iter() {
                if !v8_field_is_ignored(field) {
                    types.push(field.ty.clone());
                }
            }
        }
        syn::Fields::Unit => {}
    }
    types
}

/// Struct trace and store bodies: one statement per non-ignored field.
/// `trace` visits the field's edges; `store` converts its rooted handles into
/// edges when the value is stored into traced storage.
fn v8_struct_trace_body(
    fields: &syn::Fields,
) -> (Vec<proc_macro2::TokenStream>, Vec<proc_macro2::TokenStream>) {
    let mut traces = Vec::new();
    let mut stores = Vec::new();
    match fields {
        syn::Fields::Named(named) => {
            for field in named.named.iter() {
                if v8_field_is_ignored(field) {
                    continue;
                }
                let ident = field.ident.as_ref().expect("named field has an identifier");
                traces.push(quote! {
                    // SAFETY: The field's own trace implementation visits its
                    // edges; this call is generated for a non-ignored field.
                    unsafe { ::js_engine::gc::Trace::trace(&self.#ident, visitor) };
                });
                stores.push(quote! {
                    ::js_engine::gc::Trace::store(&mut self.#ident, ec);
                });
            }
        }
        syn::Fields::Unnamed(unnamed) => {
            for (index, field) in unnamed.unnamed.iter().enumerate() {
                if v8_field_is_ignored(field) {
                    continue;
                }
                let index = syn::Index::from(index);
                traces.push(quote! {
                    // SAFETY: The field's own trace implementation visits its
                    // edges; this call is generated for a non-ignored field.
                    unsafe { ::js_engine::gc::Trace::trace(&self.#index, visitor) };
                });
                stores.push(quote! {
                    ::js_engine::gc::Trace::store(&mut self.#index, ec);
                });
            }
        }
        syn::Fields::Unit => {}
    }
    (traces, stores)
}

/// Enum trace and store bodies: one match arm per variant for each.
/// Returns the trace arms, the store arms, and whether any traced field
/// exists.
fn v8_enum_trace_body(
    item_enum: &syn::ItemEnum,
) -> (
    Vec<proc_macro2::TokenStream>,
    Vec<proc_macro2::TokenStream>,
    bool,
) {
    let mut trace_arms = Vec::new();
    let mut store_arms = Vec::new();
    let mut has_traced_fields = false;
    for variant in &item_enum.variants {
        let variant_ident = &variant.ident;
        match &variant.fields {
            syn::Fields::Unit => {
                trace_arms.push(quote! { Self::#variant_ident => {} });
                store_arms.push(quote! { Self::#variant_ident => {} });
            }
            syn::Fields::Named(named) => {
                let traced: Vec<&syn::Ident> = named
                    .named
                    .iter()
                    .filter(|field| !v8_field_is_ignored(field))
                    .filter_map(|field| field.ident.as_ref())
                    .collect();
                has_traced_fields |= !traced.is_empty();
                let traces = traced.iter().map(|ident| {
                    quote! {
                        // SAFETY: The field's own trace implementation visits
                        // its edges; this call is generated for a non-ignored
                        // field.
                        unsafe { ::js_engine::gc::Trace::trace(#ident, visitor) };
                    }
                });
                let stores = traced.iter().map(|ident| {
                    quote! {
                        ::js_engine::gc::Trace::store(#ident, ec);
                    }
                });
                if traced.is_empty() {
                    trace_arms.push(quote! { Self::#variant_ident { .. } => {} });
                    store_arms.push(quote! { Self::#variant_ident { .. } => {} });
                } else {
                    trace_arms.push(quote! {
                        Self::#variant_ident { #(#traced),* , .. } => {
                            #(#traces)*
                        }
                    });
                    store_arms.push(quote! {
                        Self::#variant_ident { #(#traced),* , .. } => {
                            #(#stores)*
                        }
                    });
                }
            }
            syn::Fields::Unnamed(unnamed) => {
                let mut bindings = Vec::new();
                let mut traces = Vec::new();
                let mut stores = Vec::new();
                for (index, field) in unnamed.unnamed.iter().enumerate() {
                    if v8_field_is_ignored(field) {
                        bindings.push(quote! { _ });
                    } else {
                        has_traced_fields = true;
                        let binding = syn::Ident::new(
                            &format!("__v8_trace_{index}"),
                            proc_macro2::Span::call_site(),
                        );
                        bindings.push(quote! { #binding });
                        traces.push(quote! {
                            // SAFETY: The field's own trace implementation
                            // visits its edges; this call is generated for a
                            // non-ignored field.
                            unsafe { ::js_engine::gc::Trace::trace(#binding, visitor) };
                        });
                        stores.push(quote! {
                            ::js_engine::gc::Trace::store(#binding, ec);
                        });
                    }
                }
                trace_arms.push(quote! {
                    Self::#variant_ident(#(#bindings),*) => {
                        #(#traces)*
                    }
                });
                store_arms.push(quote! {
                    Self::#variant_ident(#(#bindings),*) => {
                        #(#stores)*
                    }
                });
            }
        }
    }
    (trace_arms, store_arms, has_traced_fields)
}

/// V8 backend: `#[gc_struct]` types implement cppgc `Traced` (field-walking
/// trace), `GarbageCollected` (heap allocation for platform objects), and the
/// generic marker `Trace`/`Finalize`.
#[proc_macro_attribute]
pub fn gc_struct_v8(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as Item);
    match &mut input {
        Item::Struct(item_struct) => {
            let (trace_statements, store_statements) = v8_struct_trace_body(&item_struct.fields);
            let traced_field_types = v8_traced_field_types(&item_struct.fields);
            transform_jsc(&mut item_struct.fields);
            let attrs = &item_struct.attrs;
            let vis = &item_struct.vis;
            let ident = &item_struct.ident;
            let generics = &item_struct.generics;
            let fields = &item_struct.fields;
            let semi = &item_struct.semi_token;
            let (impl_generics, ty_generics, _) = generics.split_for_impl();

            let where_clause = v8_traced_where_clause(generics, &traced_field_types);
            let type_name = ident.to_string();
            let get_name = proc_macro2::Literal::c_string(
                &std::ffi::CString::new(type_name).expect("type name contains no NUL bytes"),
            );

            let expanded = quote! {
                #(#attrs)*
                #[derive(Clone)]
                #vis struct #ident #generics #fields #semi

                impl #impl_generics ::js_engine::gc::Finalize for #ident #ty_generics #where_clause {}

                unsafe impl #impl_generics ::js_engine::gc::Trace for #ident #ty_generics #where_clause {
                    unsafe fn trace(&self, visitor: &mut ::js_engine::v8_gc::Visitor) {
                        #(#trace_statements)*
                    }

                    fn store(&mut self, ec: &mut dyn ::js_engine::ExecutionContext<::js_engine::v8::V8Types>) {
                        #(#store_statements)*
                    }
                }

                unsafe impl #impl_generics ::js_engine::v8_gc::GarbageCollected for #ident #ty_generics #where_clause {
                    fn trace(&self, visitor: &mut ::js_engine::v8_gc::Visitor) {
                        // SAFETY: Delegated to the field-walking trace.
                        unsafe { ::js_engine::gc::Trace::trace(self, visitor) }
                    }

                    fn get_name(&self) -> &'static std::ffi::CStr {
                        #get_name
                    }
                }
            };
            expanded.into()
        }
        Item::Enum(item_enum) => {
            let (trace_arms, store_arms, _) = v8_enum_trace_body(item_enum);
            let mut traced_field_types = Vec::new();
            for variant in &item_enum.variants {
                traced_field_types.extend(v8_traced_field_types(&variant.fields));
            }
            for variant in &mut item_enum.variants {
                transform_jsc(&mut variant.fields);
            }
            let attrs = &item_enum.attrs;
            let vis = &item_enum.vis;
            let ident = &item_enum.ident;
            let generics = &item_enum.generics;
            let variants = &item_enum.variants;
            let (impl_generics, ty_generics, _) = generics.split_for_impl();

            let where_clause = v8_traced_where_clause(generics, &traced_field_types);
            let type_name = ident.to_string();
            let get_name = proc_macro2::Literal::c_string(
                &std::ffi::CString::new(type_name).expect("type name contains no NUL bytes"),
            );

            let expanded = quote! {
                #(#attrs)*
                #[derive(Clone)]
                #vis enum #ident #generics {
                    #variants
                }

                impl #impl_generics ::js_engine::gc::Finalize for #ident #ty_generics #where_clause {}

                unsafe impl #impl_generics ::js_engine::gc::Trace for #ident #ty_generics #where_clause {
                    unsafe fn trace(&self, visitor: &mut ::js_engine::v8_gc::Visitor) {
                        match self {
                            #(#trace_arms)*
                        }
                    }

                    fn store(&mut self, ec: &mut dyn ::js_engine::ExecutionContext<::js_engine::v8::V8Types>) {
                        match self {
                            #(#store_arms)*
                        }
                    }
                }

                unsafe impl #impl_generics ::js_engine::v8_gc::GarbageCollected for #ident #ty_generics #where_clause {
                    fn trace(&self, visitor: &mut ::js_engine::v8_gc::Visitor) {
                        // SAFETY: Delegated to the field-walking trace.
                        unsafe { ::js_engine::gc::Trace::trace(self, visitor) }
                    }

                    fn get_name(&self) -> &'static std::ffi::CStr {
                        #get_name
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
