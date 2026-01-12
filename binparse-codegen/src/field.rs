use std::collections::HashMap;

use binparse::Len;
use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::struct_::{DoneField, GeneratedStruct};

pub(crate) struct FieldCtx<'a> {
    pub(crate) field: &'a ast::Field<'a>,
    pub(crate) start_offset: Option<Len>,
    pub(crate) done_fields: &'a [DoneField<'a>],
    pub(crate) done: &'a HashMap<&'a str, GeneratedStruct>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("type needs alignment but is not aligned itself")]
    UnalignedType,
    #[error("type needs alignment, but the start offset ({0:?}) is not aligned")]
    InvalidAlignment(Len),
    #[error("cannot determine field offset: no start offset and no previous fields")]
    UnknownOffset,
    #[error("unknown type: {0}")]
    UnknownType(String),
}

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
        done: &'a HashMap<&'a str, GeneratedStruct>,
    ) -> Self {
        Self {
            field,
            start_offset,
            done_fields,
            done,
        }
    }

    pub(crate) fn generate(self) -> Result<GeneratedField, Error> {
        // TODO: we need to handle lens which depend on some previous field, probably by making
        //       the `GeneratedField.len` it's own enum type
        let field_name = format_ident!("{}", self.field.name);
        let offset_getter_fn_name = format_ident!("{}_end_offset", field_name);

        let (len, definitions, field_getter) = match &self.field.value {
            ast::FieldValue::Type(ty) => match ty {
                ast::Type::Primitive(p) => {
                    let (len, def) = match_primitive(p);

                    match (&self.start_offset, self.done_fields.last()) {
                        (Some(offset), _) => {
                            let end = *offset + len;

                            let start_byte = offset.byte;
                            let end_byte = end.byte;

                            let field_getter = quote! {
                                pub fn #field_name(&self) -> #def {
                                    let field_data = self.data[#start_byte..#end_byte];
                                    #def::from_ne_bytes(field_data)
                                }
                            };

                            (Some(len), quote! {}, field_getter)
                        }

                        (None, None) => return Err(Error::UnknownOffset),

                        _ => todo!(),
                    }
                }

                ast::Type::BitField(width) => {
                    let width = *width as usize;
                    let len = Len {
                        byte: 0,
                        bit: width,
                    };

                    match (&self.start_offset, self.done_fields.last()) {
                        (Some(offset), _) => {
                            let start_byte = offset.byte;
                            let start_bit = offset.bit;

                            let field_getter = if start_bit + width <= 8 {
                                let mask = (1u8 << width) - 1;
                                quote! {
                                    pub fn #field_name(&self) -> u8 {
                                        (self.data[#start_byte] >> #start_bit) & #mask
                                    }
                                }
                            } else {
                                let bits_in_first_byte = 8 - start_bit;
                                let bits_in_second_byte = width - bits_in_first_byte;
                                let first_mask = (1u8 << bits_in_first_byte) - 1;
                                let second_mask = (1u8 << bits_in_second_byte) - 1;
                                let second_byte = start_byte + 1;

                                quote! {
                                    pub fn #field_name(&self) -> u8 {
                                        let first_part = (self.data[#start_byte] >> #start_bit) & #first_mask;
                                        let second_part = self.data[#second_byte] & #second_mask;
                                        first_part | (second_part << #bits_in_first_byte)
                                    }
                                }
                            };

                            (Some(len), quote! {}, field_getter)
                        }

                        (None, None) => return Err(Error::UnknownOffset),

                        _ => todo!(),
                    }
                }

                ast::Type::StructRef(struct_name) => {
                    let generated_struct = self
                        .done
                        .get(struct_name)
                        .ok_or_else(|| Error::UnknownType(struct_name.to_string()))?;

                    let len = generated_struct.len;
                    let struct_ident = format_ident!("{}", struct_name);

                    match (&self.start_offset, self.done_fields.last()) {
                        (Some(offset), _) => {
                            let start_byte = offset.byte;

                            let field_getter = quote! {
                                pub fn #field_name(&self) -> #struct_ident<'_> {
                                    #struct_ident { data: &self.data[#start_byte..] }
                                }
                            };

                            (len, quote! {}, field_getter)
                        }

                        (None, None) => return Err(Error::UnknownOffset),

                        _ => todo!(),
                    }
                }

                _ => todo!(),
            },
            ast::FieldValue::Constraint(_) => todo!(),
        };

        let offset_getter = match (&self.start_offset, self.done_fields.last()) {
            (Some(offset), _) => match &len {
                Some(len) => {
                    let total_len = *offset + *len;
                    let total_byte = total_len.byte;
                    let total_bit = total_len.bit;

                    quote! {
                        pub fn #offset_getter_fn_name(&self) -> binparse::Len {
                            binparse::Len {
                                byte: #total_byte,
                                bit: #total_bit,
                            }
                        }
                    }
                }

                None => todo!(),
            },

            (None, Some(prev_field)) => {
                let prev_offset_getter = &prev_field.offset_getter_fn_name;
                match &len {
                    Some(len) => {
                        let len_byte = len.byte;
                        let len_bit = len.bit;

                        quote! {
                            pub fn #offset_getter_fn_name(&self) -> binparse::Len {
                                let prev = self.#prev_offset_getter();
                                binparse::Len {
                                    byte: prev.byte + #len_byte,
                                    bit: prev.bit + #len_bit,
                                }
                            }
                        }
                    }

                    None => todo!(),
                }
            }

            (None, None) => return Err(Error::UnknownOffset),
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

fn match_primitive(primitive: &ast::Primitive) -> (Len, TokenStream) {
    match primitive {
        ast::Primitive::U8 => (Len { byte: 1, bit: 0 }, quote! { u8 }),
        ast::Primitive::U16 => (Len { byte: 2, bit: 0 }, quote! { u16 }),
        ast::Primitive::U32 => (Len { byte: 4, bit: 0 }, quote! { u32 }),
        ast::Primitive::U64 => (Len { byte: 8, bit: 0 }, quote! { u64 }),
        ast::Primitive::U128 => (Len { byte: 16, bit: 0 }, quote! { u128 }),
    }
}
