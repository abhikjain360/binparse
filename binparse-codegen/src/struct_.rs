use std::collections::HashMap;

use binparse::Len;
use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::field::FieldCtx;

pub(crate) struct StructCtx<'a> {
    pub(crate) origin: &'a ast::Struct<'a>,
    pub(crate) offset: Option<Len>,
    done_fields: Vec<DoneField<'a>>,
    #[expect(dead_code)]
    pub(crate) done: &'a HashMap<&'a str, GeneratedStruct>,
}

#[expect(dead_code)]
pub(crate) struct DoneField<'a> {
    origin: &'a ast::Field<'a>,
    len: Option<Len>,
    offset_getter_fn_name: syn::Ident,
}

pub(crate) struct GeneratedStruct {
    #[expect(dead_code)]
    pub(crate) len: Option<Len>,
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

impl<'a> StructCtx<'a> {
    pub(crate) fn new(
        origin: &'a ast::Struct<'a>,
        done: &'a HashMap<&'a str, GeneratedStruct>,
    ) -> Self {
        Self {
            origin,
            offset: Some(Len { byte: 0, bit: 0 }),
            done_fields: vec![],
            done,
        }
    }

    pub(crate) fn generate(mut self) -> Result<GeneratedStruct, Error> {
        let mut field_definitions = TokenStream::new();
        let mut functions = TokenStream::new();

        let mut parser_impl = TokenStream::new();
        let parser_ret = TokenStream::new();

        for item in &self.origin.items {
            if let ast::StructItem::Field(field) = item {
                let current_offset = self.offset;
                let field_ctx = FieldCtx::new(field, current_offset, &self.done_fields);
                let generated = field_ctx.generate().map_err(|error| Error::Field {
                    name: field.name.to_string(),
                    error,
                })?;

                field_definitions.extend(generated.definitions);
                functions.extend(generated.field_getter);
                functions.extend(generated.offset_getter);

                self.offset = match (self.offset, generated.len) {
                    (Some(current), Some(field_len)) => {
                        let fn_name = &generated.offset_getter_fn_name;
                        parser_impl.extend(quote! {
                            let len = len + me.#fn_name();
                        });
                        Some(current + field_len)
                    }
                    _ => None,
                };

                self.done_fields.push(DoneField {
                    origin: field,
                    len: generated.len,
                    offset_getter_fn_name: generated.offset_getter_fn_name,
                });
            }
        }

        let name = format_ident!("{}", self.origin.name);
        let tokens = quote! {
            pub struct #name<'a> {
                data: &'a [u8],
                #field_definitions
            }

            impl<'a> #name<'a> {
                pub fn parse(data: &'a [u8]) -> Option<(Self, &'a [u8])> {
                    #parser_impl
                    Self { #parser_ret }
                }

                #functions
            }
        };

        Ok(GeneratedStruct {
            len: self.offset,
            tokens,
        })
    }
}
