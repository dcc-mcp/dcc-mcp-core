//! Parser for the `#[py_wrapper(...)]` attribute (issue #528 M2).
//!
//! Grammar:
//!
//! ```text
//! py_wrapper(
//!     [ inner = "InnerType" , ]   // optional; absent = direct pyclass pattern
//!     fields(
//!         <ident> : <type> => [ <mode> (, <mode>)* ] ,
//!         ...
//!     )
//! )
//! ```
//!
//! `<mode>` is one of: `get` | `get(by_str)` | `get(clone)` |
//! `get(to_string)` | `set` | `repr` | `dict`. See [`FieldMode`] for
//! semantics.

use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Ident, LitStr, Token, Type, bracketed, parenthesized};

/// How a field is exposed to Python. A single field can carry multiple
/// modes (eg `[get, set, repr]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldMode {
    /// `#[getter] fn x(&self) -> T { self.<base>.x }` for `Copy` types.
    Get,
    /// `#[getter] fn x(&self) -> &str { &self.<base>.x }` for `String`.
    GetByStr,
    /// `#[getter] fn x(&self) -> T { self.<base>.x.clone() }` for owned types.
    GetClone,
    /// `#[getter] fn x(&self) -> T { self.<base>.x.to_string() }` for
    /// types whose `Display` impl is the canonical Python serialisation
    /// (e.g. `Url`, `IpAddr`, `PathBuf`).
    GetToString,
    /// `#[setter] fn set_x(&mut self, value: T) { self.<base>.x = value; }`.
    Set,
    /// Field appears in the auto-generated `__repr__` body via `{:?}`.
    Repr,
    /// Field appears in the auto-generated `to_dict` body.
    Dict,
}

/// One row of the `fields(...)` table.
#[derive(Clone, Debug)]
pub struct FieldDecl {
    /// Outer attributes captured before the field declaration. `///` doc
    /// comments are picked up here (they desugar to `#[doc = "..."]`) so
    /// the generated getter/setter can carry them through to PyO3 and on
    /// to Python's `help()` output.
    pub attrs: Vec<Attribute>,
    pub name: Ident,
    pub ty: Type,
    pub modes: Vec<FieldMode>,
}

/// The entire parsed `#[py_wrapper(...)]` attribute.
#[derive(Debug)]
pub struct PyWrapperAttr {
    /// Name of the inner field on the wrapper struct that the macro
    /// forwards to. `None` means the struct is itself `#[pyclass]` and
    /// fields are accessed as `self.<field>` directly.
    pub inner_field: Option<Ident>,
    pub fields: Vec<FieldDecl>,
}

impl Parse for PyWrapperAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut inner_field: Option<Ident> = None;
        let mut fields: Vec<FieldDecl> = Vec::new();
        // Top-level args are comma-separated `key = value` or `fields(...)`.
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            if key == "inner" {
                input.parse::<Token![=]>()?;
                let value: LitStr = input.parse()?;
                // Treat the literal string as an identifier so the user
                // can keep their existing `inner: <Type>` field name.
                inner_field = Some(syn::Ident::new("inner", value.span()));
                let _ = value;
            } else if key == "fields" {
                let body;
                parenthesized!(body in input);
                let parsed: Punctuated<FieldDecl, Token![,]> =
                    body.parse_terminated(FieldDecl::parse, Token![,])?;
                fields.extend(parsed);
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    format!("unknown py_wrapper key `{key}`; expected `inner` or `fields`"),
                ));
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        if fields.is_empty() {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "py_wrapper: `fields(...)` is required and must not be empty",
            ));
        }
        Ok(Self {
            inner_field,
            fields,
        })
    }
}

impl Parse for FieldDecl {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        let name: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let ty: Type = input.parse()?;
        input.parse::<Token![=>]>()?;
        let body;
        bracketed!(body in input);
        let parsed: Punctuated<FieldMode, Token![,]> =
            body.parse_terminated(FieldMode::parse, Token![,])?;
        let modes: Vec<FieldMode> = parsed.into_iter().collect();
        if modes.is_empty() {
            return Err(syn::Error::new(
                name.span(),
                format!("field `{name}`: at least one mode required (get/set/repr/dict)"),
            ));
        }
        Ok(Self {
            attrs,
            name,
            ty,
            modes,
        })
    }
}

impl Parse for FieldMode {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let kw: Ident = input.parse()?;
        let mode = match kw.to_string().as_str() {
            "get" => {
                if input.peek(syn::token::Paren) {
                    let body;
                    parenthesized!(body in input);
                    let variant: Ident = body.parse()?;
                    match variant.to_string().as_str() {
                        "by_str" => FieldMode::GetByStr,
                        "clone" => FieldMode::GetClone,
                        "to_string" => FieldMode::GetToString,
                        other => {
                            return Err(syn::Error::new(
                                variant.span(),
                                format!(
                                    "unknown get variant `{other}`; expected `by_str`, `clone`, or `to_string`"
                                ),
                            ));
                        }
                    }
                } else {
                    FieldMode::Get
                }
            }
            "set" => FieldMode::Set,
            "repr" => FieldMode::Repr,
            "dict" => FieldMode::Dict,
            other => {
                return Err(syn::Error::new(
                    kw.span(),
                    format!("unknown field mode `{other}`; expected get/set/repr/dict"),
                ));
            }
        };
        Ok(mode)
    }
}

// Unit tests for the parser. The proc-macro itself compiles as a
// normal lib when invoked from `cargo test --lib`, so syn types can be
// exercised without requiring a downstream crate.
#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> syn::Result<PyWrapperAttr> {
        syn::parse_str::<PyWrapperAttr>(src)
    }

    #[test]
    fn delegation_with_inner_field() {
        let attr = parse(
            r#"inner = "Inner", fields(port: u16 => [get, set, repr], host: String => [get(by_str)])"#,
        )
        .unwrap();
        assert!(attr.inner_field.is_some());
        assert_eq!(attr.fields.len(), 2);
        assert_eq!(
            attr.fields[0].modes,
            vec![FieldMode::Get, FieldMode::Set, FieldMode::Repr]
        );
        assert_eq!(attr.fields[1].modes, vec![FieldMode::GetByStr]);
    }

    #[test]
    fn direct_pattern_no_inner() {
        let attr = parse(r#"fields(name: String => [get(clone), dict])"#).unwrap();
        assert!(attr.inner_field.is_none());
        assert_eq!(attr.fields.len(), 1);
        assert_eq!(
            attr.fields[0].modes,
            vec![FieldMode::GetClone, FieldMode::Dict]
        );
    }

    #[test]
    fn rejects_empty_fields() {
        let err = parse(r#"fields()"#).unwrap_err();
        assert!(err.to_string().contains("fields(...)"));
    }

    #[test]
    fn rejects_unknown_mode() {
        let err = parse(r#"fields(x: u8 => [foo])"#).unwrap_err();
        assert!(err.to_string().contains("unknown field mode"));
    }

    #[test]
    fn rejects_unknown_get_variant() {
        let err = parse(r#"fields(x: u8 => [get(weird)])"#).unwrap_err();
        assert!(err.to_string().contains("unknown get variant"));
    }

    #[test]
    fn parses_get_to_string() {
        let attr = parse(r#"fields(host: String => [get(to_string)])"#).unwrap();
        assert_eq!(attr.fields[0].modes, vec![FieldMode::GetToString]);
    }

    #[test]
    fn rejects_unknown_top_level_key() {
        let err = parse(r#"foo = "bar", fields(x: u8 => [get])"#).unwrap_err();
        assert!(err.to_string().contains("unknown py_wrapper key"));
    }

    #[test]
    fn rejects_field_without_modes() {
        let err = parse(r#"fields(x: u8 => [])"#).unwrap_err();
        assert!(err.to_string().contains("at least one mode required"));
    }

    #[test]
    fn captures_doc_comment_on_field() {
        let attr = parse(
            r#"fields(
                /// Enable the Prometheus endpoint.
                enable_prometheus: bool => [get, set]
            )"#,
        )
        .unwrap();
        assert_eq!(attr.fields.len(), 1);
        assert_eq!(attr.fields[0].attrs.len(), 1);
        assert!(attr.fields[0].attrs[0].path().is_ident("doc"));
    }
}
