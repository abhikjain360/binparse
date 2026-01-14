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

pub(crate) struct StructAccum<'a> {
    pub(crate) name: syn::Ident,
    pub(crate) offset: GeneratedLen,
    pub(crate) done_fields: Vec<DoneField>,
    pub(crate) done: &'a HashMap<&'a str, GeneratedStruct>,
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

impl<'a> StructAccum<'a> {
    pub(crate) fn new(name: &str, done: &'a HashMap<&'a str, GeneratedStruct>) -> Self {
        Self {
            name: format_ident!("{}", name),
            offset: GeneratedLen::Fixed(Len { byte: 0, bit: 0 }),
            done_fields: vec![],
            done,
            other_entities: TokenStream::new(),
            field_definitions: TokenStream::new(),
            functions: TokenStream::new(),
            last_offset_getter_fn_name: None,
        }
    }
}

pub(crate) fn generate<'a>(
    ast: &'a ast::Struct<'a>,
    done: &'a mut HashMap<&'a str, GeneratedStruct>,
) -> Result<(), Error> {
    let mut accum = StructAccum::new(ast.name, done);

    for item in &ast.items {
        if let ast::StructItem::Field(ast_field) = item {
            field::generate(ast_field, &mut accum).map_err(|error| Error::Field {
                name: ast_field.name.to_string(),
                error,
            })?;
        } else {
            todo!("conditional fields");
        }
    }

    let name = &accum.name;
    let other_entities = &accum.other_entities;

    let parse_fn = if let Some(fn_name) = accum.last_offset_getter_fn_name {
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

    let field_definitions = accum.field_definitions;
    let functions = accum.functions;
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
            len: accum.offset.clone(),
            tokens,
        },
    );

    Ok(())
}
