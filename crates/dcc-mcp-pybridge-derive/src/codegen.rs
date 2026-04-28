//! Code generation for `#[derive(PyWrapper)]` (issue #528 M2).
//!
//! Consumes a [`PyWrapperAttr`] and the wrapper struct's name, emits a
//! `#[pymethods] impl <Wrapper>` block containing one accessor per
//! requested mode plus aggregated `__repr__` / `to_dict` if any field
//! requested those.
//!
//! Relies on PyO3's `multiple-pymethods` feature (already enabled
//! workspace-wide in `Cargo.toml`) so the generated impl block can sit
//! alongside the user's hand-written `#[pymethods] impl` block without
//! conflict.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::Ident;

use crate::parse::{FieldDecl, FieldMode, PyWrapperAttr};

/// Generate the full `impl` body for the wrapper struct.
pub fn generate(struct_ident: &Ident, attr: &PyWrapperAttr) -> TokenStream2 {
    let base = base_path(attr.inner_field.as_ref());
    let mut items: Vec<TokenStream2> = Vec::new();

    for field in &attr.fields {
        for &mode in &field.modes {
            if let Some(item) = emit_accessor(field, mode, &base) {
                items.push(item);
            }
        }
    }

    if attr
        .fields
        .iter()
        .any(|f| f.modes.contains(&FieldMode::Repr))
    {
        items.push(emit_repr(struct_ident, &attr.fields, &base));
    }
    if attr
        .fields
        .iter()
        .any(|f| f.modes.contains(&FieldMode::Dict))
    {
        items.push(emit_to_dict(&attr.fields, &base));
    }

    quote! {
        #[pyo3::pymethods]
        impl #struct_ident {
            #( #items )*
        }
    }
}

/// Returns the path prefix used to access an inner field. Either
/// `self.inner.` (delegation) or `self.` (direct pyclass).
fn base_path(inner_field: Option<&Ident>) -> TokenStream2 {
    match inner_field {
        Some(ident) => quote! { self.#ident. },
        None => quote! { self. },
    }
}

fn emit_accessor(field: &FieldDecl, mode: FieldMode, base: &TokenStream2) -> Option<TokenStream2> {
    let name = &field.name;
    let ty = &field.ty;
    Some(match mode {
        FieldMode::Get => quote! {
            #[getter]
            fn #name(&self) -> #ty { #base #name }
        },
        FieldMode::GetByStr => quote! {
            #[getter]
            fn #name(&self) -> &str { & #base #name }
        },
        FieldMode::GetClone => quote! {
            #[getter]
            fn #name(&self) -> #ty { #base #name .clone() }
        },
        FieldMode::Set => {
            let setter = format_ident!("set_{}", name);
            quote! {
                #[setter]
                fn #setter (&mut self, value: #ty) { #base #name = value; }
            }
        }
        FieldMode::Repr | FieldMode::Dict => return None,
    })
}

fn emit_repr(struct_ident: &Ident, fields: &[FieldDecl], base: &TokenStream2) -> TokenStream2 {
    let type_name = struct_ident.to_string();
    let parts: Vec<TokenStream2> = fields
        .iter()
        .filter(|f| f.modes.contains(&FieldMode::Repr))
        .map(|f| {
            let n = &f.name;
            let key = n.to_string();
            quote! { format!("{}={:?}", #key, & #base #n) }
        })
        .collect();
    quote! {
        fn __repr__(&self) -> String {
            let _parts: Vec<String> = vec![ #( #parts ),* ];
            format!("{}({})", #type_name, _parts.join(", "))
        }
    }
}

fn emit_to_dict(fields: &[FieldDecl], base: &TokenStream2) -> TokenStream2 {
    let entries: Vec<TokenStream2> = fields
        .iter()
        .filter(|f| f.modes.contains(&FieldMode::Dict))
        .map(|f| {
            let n = &f.name;
            let key = n.to_string();
            quote! { _dict.set_item(#key, & #base #n)?; }
        })
        .collect();
    quote! {
        fn to_dict<'py>(
            &self,
            py: pyo3::Python<'py>,
        ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::types::PyDict>> {
            let _dict = pyo3::types::PyDict::new(py);
            #( #entries )*
            Ok(_dict)
        }
    }
}
