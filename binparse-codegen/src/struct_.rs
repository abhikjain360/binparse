use std::collections::HashMap;

use binparse::Len;
use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{GeneratedLen, field};

#[derive(Clone, Copy)]
pub(crate) enum DoneFieldType {
    Primitive,
    BitField,
    Other,
}

pub(crate) struct DoneField {
    pub(crate) name: String,
    pub(crate) field_type: DoneFieldType,
    pub(crate) len: GeneratedLen,
    pub(crate) offset_getter_fn_name: syn::Ident,
}

pub(crate) struct StructAccum {
    pub(crate) name: syn::Ident,
    pub(crate) offset: GeneratedLen,
    pub(crate) done_fields: Vec<DoneField>,
    pub(crate) other_entities: TokenStream,
    pub(crate) field_definitions: TokenStream,
    pub(crate) functions: TokenStream,
    pub(crate) last_offset_getter_fn_name: Option<syn::Ident>,
}

pub(crate) struct GeneratedStruct {
    pub(crate) len: GeneratedLen,
    pub(crate) tokens: TokenStream,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to generate field '{name}': {error}")]
    Field {
        name: String,
        #[source]
        error: crate::field::Error,
    },
    #[error("'field {field}' needs byte-alignment, but previous fields didn't align")]
    Unaligned { field: String },
}

impl StructAccum {
    pub(crate) fn new(name: &str) -> Self {
        Self {
            name: format_ident!("{}", name),
            offset: GeneratedLen::Fixed(Len { byte: 0, bit: 0 }),
            done_fields: vec![],
            other_entities: TokenStream::new(),
            field_definitions: TokenStream::new(),
            functions: TokenStream::new(),
            last_offset_getter_fn_name: None,
        }
    }
}

pub(crate) fn generate<'a>(
    ast: &'a ast::Struct<'a>,
    done: &mut HashMap<&'a str, GeneratedStruct>,
) -> Result<(), Error> {
    let mut accum = StructAccum::new(ast.name);

    for item in &ast.items {
        if let ast::StructItem::Field(ast_field) = item {
            field::generate(ast_field, done, &mut accum).map_err(|error| Error::Field {
                name: ast_field.name.to_string(),
                error,
            })?;
        } else {
            todo!("conditional fields");
        }
    }

    let StructAccum {
        name,
        offset,
        done_fields: _,
        other_entities,
        field_definitions,
        functions,
        last_offset_getter_fn_name,
    } = accum;

    let parse_fn = if let Some(fn_name) = last_offset_getter_fn_name {
        quote! {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                let len = me.#fn_name();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData { expected: len.byte, got: data.len() });
                }
                Ok((me, &data[len.byte..]))
            }
        }
    } else {
        quote! {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                Ok((Self { data }, data))
            }
        }
    };

    let tokens = quote! {
        #other_entities

        pub struct #name<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
            #field_definitions
        }

        impl<'a> #name<'a> {
            #parse_fn

            #functions
        }
    };

    done.insert(
        ast.name,
        GeneratedStruct {
            len: offset,
            tokens,
        },
    );

    Ok(())
}
