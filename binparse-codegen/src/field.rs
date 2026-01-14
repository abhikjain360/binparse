use std::collections::HashMap;

use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{
    GeneratedLen,
    struct_::{DoneField, GeneratedStruct},
    type_,
};

pub(crate) struct FieldCtx<'a, 'b> {
    pub(crate) field: &'a ast::Field<'a>,
    pub(crate) start_offset: GeneratedLen,
    pub(crate) done_fields: &'a [DoneField<'a>],
    pub(crate) done: &'a HashMap<&'a str, GeneratedStruct>,
    pub(crate) parent_struct_name: &'b syn::Ident,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("type generation error: {0}")]
    Type(#[from] type_::Error),
    #[error("cannot determine field offset: no start offset and no previous fields")]
    UnknownOffset,
}

pub(crate) struct GeneratedField {
    pub(crate) len: GeneratedLen,
    pub(crate) offset_getter_fn_name: syn::Ident,
    pub(crate) definitions: TokenStream,
    pub(crate) helper_fns: TokenStream,
    pub(crate) helper_entities: TokenStream,
    pub(crate) field_getter: TokenStream,
    pub(crate) offset_getter: TokenStream,
}

impl<'a, 'b> FieldCtx<'a, 'b> {
    pub(crate) fn new(
        field: &'a ast::Field<'a>,
        start_offset: GeneratedLen,
        done_fields: &'a [DoneField<'a>],
        done: &'a HashMap<&'a str, GeneratedStruct>,
        parent_struct_name: &'b syn::Ident,
    ) -> Self {
        Self {
            field,
            start_offset,
            done_fields,
            done,
            parent_struct_name,
        }
    }

    pub(crate) fn generate(self) -> Result<GeneratedField, Error> {
        let field_name = format_ident!("{}", self.field.name);
        let offset_getter_fn_name = format_ident!("{}_end_offset", field_name);

        let (len, definitions, helper_fns, helper_entities, field_getter) = match &self.field.value
        {
            ast::FieldValue::Type(ty) => {
                let generated = type_::TypeCtx {
                    done: self.done,
                    parent_struct_name: self.parent_struct_name,
                }
                .generate(
                    ty,
                    &field_name,
                    self.start_offset.clone(),
                    self.done_fields,
                )?;
                let return_ty = generated.return_ty;
                let field_getter_body = generated.field_getter_body;
                let field_getter = quote! {
                    #[allow(clippy::identity_op)]
                    pub fn #field_name(&self) -> #return_ty {
                        #field_getter_body
                    }
                };
                (
                    generated.len,
                    generated.definitions,
                    generated.helper_fns,
                    generated.helper_entities,
                    field_getter,
                )
            }

            ast::FieldValue::Constraint(_) => todo!("handle constraint-type fields"),
        };

        let offset_getter = match len.clone() + self.start_offset {
            GeneratedLen::Fixed(total_len) => {
                let total_byte = total_len.byte;
                let total_bit = total_len.bit;
                quote! {
                    pub fn #offset_getter_fn_name(&self) -> binparse::Len {
                        binparse::Len { byte: #total_byte, bit: #total_bit }
                    }
                }
            }
            GeneratedLen::Dynamic(total_len) => {
                quote! {
                    pub fn #offset_getter_fn_name(&self) -> binparse::Len {
                        #total_len
                    }
                }
            }
        };

        Ok(GeneratedField {
            len,
            definitions,
            helper_fns,
            helper_entities,
            offset_getter_fn_name,
            offset_getter,
            field_getter,
        })
    }
}
