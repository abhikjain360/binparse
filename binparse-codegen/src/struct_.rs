use std::collections::HashMap;

use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::quote;

use crate::Len;
use crate::field::FieldCtx;

pub(crate) struct StructCtx<'a> {
    pub(crate) origin: &'a ast::Struct<'a>,
    pub(crate) offset: Option<Len>,
    done_fields: Vec<DoneField<'a>>,
    pub(crate) done: &'a HashMap<&'a str, GeneratedStruct>,
}

pub(crate) struct DoneField<'a> {
    origin: &'a ast::Field<'a>,
    len: Option<Len>,
}

pub(crate) struct GeneratedStruct {
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
        let mut parser_ret = TokenStream::new();

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

                // Update offset for next field
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
            }
        }

        let name = self.origin.name;
        let tokens = quote! {
            pub struct #name<'a> {
                data: &'a [u8],
                #field_definitions
            }

            impl<'a> #name<'a> {
                pub fn parse(data: &'a [u8]) -> Option<(Self, &'a [u8]) {
                    #parser_impl
                    #parser_ret
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
