use std::collections::HashMap;

use binparse::Len;
use binparse_dsl as ast;
use quote::{format_ident, quote};

use crate::{
    GeneratedLen,
    field::FieldAccum,
    struct_::{DoneField, DoneFieldType, GeneratedStruct, StructAccum},
    type_::{self, GeneratedTypeInfo},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("array size path is empty")]
    EmptyPath,
    #[error("array size path references unknown field '{0}'")]
    UnknownField(String),
    #[error("array size path must reference a primitive or bitfield, not '{0}'")]
    InvalidSizeType(String),
}

pub(crate) fn generate(
    array_type: &ast::ArrayType<'_>,
    done: &HashMap<&str, GeneratedStruct>,
    struct_accum: &mut StructAccum,
    accum: &mut FieldAccum,
    start_offset: GeneratedLen,
) -> Result<GeneratedTypeInfo, type_::Error> {
    match start_offset {
        GeneratedLen::Fixed(start_offset_len) => {
            if start_offset_len.bit != 0 {
                return Err(type_::Error::InvalidAlignment(start_offset_len));
            }

            let done_fields = &struct_accum.done_fields;
            let field_name = &accum.field_name;

            let (count, static_count) = generate_array_size(&array_type.size, done_fields)?;

            let offset = match done_fields.last() {
                Some(DoneField {
                    offset_getter_fn_name,
                    ..
                }) => quote! {{
                    let len = self.#offset_getter_fn_name();
                    if len.bit > 0 { return Err(::binparse::ParseError::UnalignedLength(len)) };
                    len.byte
                }},
                None => quote! { 0 },
            };

            let iterator_name = format_ident!("{}_Iterator", field_name);

            let (elem_len, elem_return_ty, iterator_fields, iterator_init, next_body) =
                match &array_type.elem_ty {
                    ast::ArrayElemType::Primitive(prim) => {
                        let (prim_len, prim_ty) = crate::match_primitive(prim);
                        let byte_len = prim_len.byte;
                        let iterator_fields = quote! {
                            idx: usize,
                            count: usize,
                            data: &'a [u8]
                        };
                        let iterator_init = quote! {
                            idx: 0,
                            count: #count,
                            data: &self.data[#offset..],
                        };
                        let next_body = quote! {
                            let value = #prim_ty::from_ne_bytes(self.data[..#byte_len].try_into().unwrap());
                            self.data = &self.data[#byte_len..];
                            Some(Ok(value))
                        };
                        (
                            GeneratedLen::Fixed(prim_len),
                            prim_ty,
                            iterator_fields,
                            iterator_init,
                            next_body,
                        )
                    }

                    ast::ArrayElemType::StructRef(struct_name) => {
                        let generated_struct = done
                            .get(*struct_name)
                            .ok_or_else(|| type_::Error::UnknownType(struct_name.to_string()))?;
                        let struct_ident = format_ident!("{}", struct_name);
                        let return_ty = quote! { #struct_ident<'_> };
                        let iterator_fields = quote! {
                            idx: usize,
                            count: usize,
                            data: &'a [u8]
                        };
                        let iterator_init = quote! {
                            idx: 0,
                            count: #count,
                            data: &self.data[#offset..],
                        };
                        let next_body = quote! {
                            let ret = #struct_ident::parse(self.data);
                            if let Ok((rem, _)) = &ret {
                                self.data = rem;
                            }
                            Some(ret)
                        };
                        (
                            generated_struct.len.clone(),
                            return_ty,
                            iterator_fields,
                            iterator_init,
                            next_body,
                        )
                    }

                    ast::ArrayElemType::BitField(width) => {
                        let width = *width as usize;
                        let len = Len {
                            byte: 0,
                            bit: width,
                        };
                        let return_ty = quote! { u8 };
                        let iterator_fields = quote! {
                            idx: usize,
                            count: usize,
                            data: &'a [u8],
                            bit_offset: usize
                        };
                        let iterator_init = quote! {
                            idx: 0,
                            count: #count,
                            data: &self.data[#offset..],
                            bit_offset: 0,
                        };
                        let next_body = quote! {
                            let byte_idx = self.bit_offset / 8;
                            let bit_idx = self.bit_offset % 8;
                            let value = if bit_idx + #width <= 8 {
                                let mask = (1u8 << #width) - 1;
                                (self.data[byte_idx] >> bit_idx) & mask
                            } else {
                                let bits_in_first = 8 - bit_idx;
                                let bits_in_second = #width - bits_in_first;
                                let first_mask = (1u8 << bits_in_first) - 1;
                                let second_mask = (1u8 << bits_in_second) - 1;
                                let first_part = (self.data[byte_idx] >> bit_idx) & first_mask;
                                let second_part = self.data[byte_idx + 1] & second_mask;
                                first_part | (second_part << bits_in_first)
                            };
                            self.bit_offset += #width;
                            Some(Ok(value))
                        };
                        (
                            GeneratedLen::Fixed(len),
                            return_ty,
                            iterator_fields,
                            iterator_init,
                            next_body,
                        )
                    }
                };

            struct_accum.other_entities.extend(quote! {
                #[allow(non_camel_case_types)]
                struct #iterator_name<'a> {
                    #iterator_fields
                }

                impl<'a> ::std::iter::Iterator for #iterator_name<'a> {
                    type Item = ::binparse::ParseResult<#elem_return_ty>;

                    fn next(&mut self) -> std::option::Option<Self::Item> {
                        if self.idx == self.count { return None; }
                        self.idx += 1;
                        #next_body
                    }
                }
            });

            let field_getter_body = quote! {
                Ok(#iterator_name {
                    #iterator_init
                })
            };

            let len = match static_count {
                Some(count) => elem_len * count,
                None => match elem_len {
                    GeneratedLen::Fixed(Len { byte, bit }) => GeneratedLen::Dynamic(quote! {
                        let count = #count;
                        let extra_bits = #bit * count;
                        ::binparse::Len {
                            byte: #byte * count + (extra_bits / 8),
                            bit: extra_bits % 8,
                        }
                    }),
                    GeneratedLen::Dynamic(a) => GeneratedLen::Dynamic(quote! { #a * #count }),
                },
            };

            Ok(GeneratedTypeInfo {
                len,
                field_getter_body,
                return_ty: quote! { ::binparse::ParseResult<#iterator_name> },
                field_type: DoneFieldType::Other,
            })
        }

        GeneratedLen::Dynamic(_) => todo!(),
    }
}

fn generate_array_size(
    size: &ast::ArraySize,
    done_fields: &[DoneField],
) -> Result<(proc_macro2::TokenStream, Option<usize>), Error> {
    match size {
        ast::ArraySize::Dynamic => todo!("try from attributes"),

        ast::ArraySize::Path(path) => {
            let field_name = path.first().ok_or(Error::EmptyPath)?;
            let done_field = done_fields
                .iter()
                .find(|f| f.name == *field_name)
                .ok_or_else(|| Error::UnknownField(field_name.to_string()))?;

            match done_field.field_type {
                DoneFieldType::Primitive | DoneFieldType::BitField => {
                    let getter = format_ident!("{}", field_name);
                    Ok((quote! { self.#getter() as usize }, None))
                }
                DoneFieldType::Other => Err(Error::InvalidSizeType(field_name.to_string())),
            }
        }

        ast::ArraySize::Int(ast::IntLiteral { value, .. }) => {
            let v = *value;
            Ok((quote! { #v }, Some(v)))
        }

        ast::ArraySize::Binary(array_size) => {
            let (lhs_tokens, lhs_count) = generate_array_size(&array_size.lhs, done_fields)?;
            let (rhs_tokens, rhs_count) = generate_array_size(&array_size.rhs, done_fields)?;
            let (op_tokens, op_fn) = crate::match_binop(array_size.op);

            Ok((
                quote! { (#lhs_tokens #op_tokens #rhs_tokens) as usize },
                lhs_count
                    .and_then(|lhs_count| rhs_count.map(|rhs_count| op_fn(lhs_count, rhs_count))),
            ))
        }
    }
}
