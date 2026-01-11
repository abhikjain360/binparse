use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{Len, struct_::DoneField};

pub(crate) struct FieldCtx<'a> {
    pub(crate) field: &'a ast::Field<'a>,
    pub(crate) start_offset: Option<Len>,
    pub(crate) done_fields: &'a [DoneField<'a>],
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}

pub(crate) struct GeneratedField {
    pub(crate) len: Option<Len>,
    pub(crate) offset_getter_fn_name: syn::Ident,
    pub(crate) definitions: TokenStream,
    pub(crate) field_getter: TokenStream,
    pub(crate) offset_getter: TokenStream,
}

impl<'a> FieldCtx<'a> {
    pub(crate) fn new(
        field: &'a ast::Field<'a>,
        start_offset: Option<Len>,
        done_fields: &'a [DoneField<'a>],
    ) -> Self {
        Self {
            field,
            start_offset,
            done_fields,
        }
    }

    pub(crate) fn generate(self) -> Result<GeneratedField, Error> {
        // TODO: we need to handle lens which depend on some previous field, probably by making
        //       the `GeneratedField.len` it's own enum type
        let field_name = format_ident!("{}", self.field.name);

        let (len, definitions, field_getter) = match &self.field.value {
            ast::FieldValue::Type(ty) => match ty {
                ast::Type::Primitive(p) => {
                    let (len, def, _needs_alignment) = match_primitive(p);
                    (
                        Some(len),
                        quote! { #field_name: #def, },
                        quote! {
                            pub fn #field_name(&self) -> #def {
                                todo!()
                            }
                        },
                    )
                }
                _ => todo!(),
            },
            ast::FieldValue::Constraint(_) => todo!(),
        };

        let offset_getter_fn_name = format_ident!("{}_offset", self.field.name);
        let offset_getter = match self.start_offset {
            Some(Len {
                byte: offset_byte,
                bit: _offset_bit,
            }) => match &len {
                Some(Len {
                    byte: len_byte,
                    bit: _len_bit,
                }) => {
                    quote! {
                        pub fn #offset_getter_fn_name(&self) -> usize {
                            #offset_byte + #len_byte
                        }
                    }
                }
                None => todo!(),
            },
            _ => todo!(),
        };

        Ok(GeneratedField {
            len,
            definitions,
            offset_getter_fn_name,
            offset_getter,
            field_getter,
        })
    }
}

fn match_primitive(primitive: &ast::Primitive) -> (Len, TokenStream, bool) {
    match primitive {
        ast::Primitive::U8 => (Len { byte: 1, bit: 0 }, quote! { u8 }, true),
        ast::Primitive::U16 => (Len { byte: 2, bit: 0 }, quote! { u16 }, true),
        ast::Primitive::U32 => (Len { byte: 4, bit: 0 }, quote! { u32 }, true),
        ast::Primitive::U64 => (Len { byte: 8, bit: 0 }, quote! { u64 }, true),
        ast::Primitive::U128 => (Len { byte: 16, bit: 0 }, quote! { u128 }, true),
        ast::Primitive::BitField(width) => (
            Len {
                byte: 0,
                bit: *width as usize % 8,
            },
            quote! { u8 },
            false,
        ),
    }
}
