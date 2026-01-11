use std::collections::HashMap;

use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::Len;
use crate::field::FieldCtx;

pub(crate) struct StructCtx<'a> {
    pub(crate) origin: &'a ast::Struct<'a>,
    pub(crate) offset: Option<Len>,
    pub(crate) done: &'a HashMap<&'a str, GeneratedStruct>,
}

pub(crate) struct GeneratedStruct {
    pub(crate) len: Option<Len>,
    pub(crate) tokens: TokenStream,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("failed to generate field '{name}': {error}")]
    Field {
        name: String,
        #[source]
        error: crate::field::Error,
    },
}

/// Information collected per field during generation
struct FieldInfo {
    name: String,
    /// Whether this field needs its offset stored in the struct
    needs_stored_offset: bool,
    /// The static offset of this field (if known)
    static_offset: Option<Len>,
    /// The length of this field (if known)
    len: Option<Len>,
}

impl<'a> StructCtx<'a> {
    pub(crate) fn new(
        origin: &'a ast::Struct<'a>,
        done: &'a HashMap<&'a str, GeneratedStruct>,
    ) -> Self {
        Self {
            origin,
            offset: Some(Len { byte: 0, bit: 0 }),
            done,
        }
    }

    pub(crate) fn generate(mut self) -> Result<GeneratedStruct, Error> {
        let mut field_definitions = TokenStream::new();
        let mut functions = TokenStream::new();
        let mut field_infos = Vec::new();

        for item in &self.origin.items {
            if let ast::StructItem::Field(field) = item {
                let current_offset = self.offset;
                let field_ctx = FieldCtx::new(field, current_offset);
                let generated = field_ctx.generate().map_err(|error| Error::Field {
                    name: field.name.to_string(),
                    error,
                })?;

                // Track field info for parse function generation
                field_infos.push(FieldInfo {
                    name: field.name.to_string(),
                    needs_stored_offset: current_offset.is_none(),
                    static_offset: current_offset,
                    len: generated.len,
                });

                field_definitions.extend(generated.definitions);
                functions.extend(generated.field_getter);
                functions.extend(generated.offset_getter);

                // Update offset for next field
                self.offset = match (self.offset, generated.len) {
                    (Some(current), Some(field_len)) => Some(current + field_len),
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
                    todo!("implement the parse function");
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
