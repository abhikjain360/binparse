use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{
    GeneratedLen,
    struct_::{DoneField, DoneFieldType, StructAccum},
    type_,
};

pub(crate) struct FieldAccum<'a> {
    pub(crate) struct_accum: &'a mut StructAccum<'a>,
    pub(crate) field_name: syn::Ident,
    pub(crate) len: GeneratedLen,
    pub(crate) field_type: DoneFieldType,
    pub(crate) offset_getter_fn_name: syn::Ident,
    pub(crate) definitions: TokenStream,
    pub(crate) helper_fns: TokenStream,
    pub(crate) field_getter: TokenStream,
    pub(crate) offset_getter: TokenStream,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("type generation error: {0}")]
    Type(#[from] type_::Error),
    #[error("cannot determine field offset: no start offset and no previous fields")]
    UnknownOffset,
}

impl<'a> FieldAccum<'a> {
    pub(crate) fn new(struct_accum: &'a mut StructAccum<'a>, field_name: &str) -> Self {
        let field_name_ident = format_ident!("{}", field_name);
        let offset_getter_fn_name = format_ident!("{}_end_offset", field_name);
        Self {
            struct_accum,
            field_name: field_name_ident,
            len: GeneratedLen::Fixed(binparse::Len { byte: 0, bit: 0 }),
            field_type: DoneFieldType::Other,
            offset_getter_fn_name,
            definitions: TokenStream::new(),
            helper_fns: TokenStream::new(),
            field_getter: TokenStream::new(),
            offset_getter: TokenStream::new(),
        }
    }
}

pub(crate) fn generate(
    ast: &ast::Field<'_>,
    struct_accum: &mut StructAccum<'_>,
) -> Result<(), Error> {
    let mut field_accum = FieldAccum::new(struct_accum, ast.name);

    match &ast.value {
        ast::FieldValue::Type(ty) => {
            let info = type_::generate(ty, &mut field_accum)?;

            let field_name = &field_accum.field_name;
            let return_ty = info.return_ty;
            let field_getter_body = info.field_getter_body;
            let field_getter = quote! {
                #[allow(clippy::identity_op)]
                pub fn #field_name(&self) -> #return_ty {
                    #field_getter_body
                }
            };
        }

        ast::FieldValue::Constraint(_) => todo!("handle constraint-type fields"),
    };

    let offset_getter_fn_name = field_accum.offset_getter_fn_name;
    let len = field_accum.len;
    let field_type = field_accum.field_type;

    let offset_getter = match len.clone() + field_accum.struct_accum.offset.clone() {
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

    struct_accum.offset = struct_accum.offset.clone() + len.clone();
    struct_accum.done_fields.push(DoneField {
        name: ast.name.to_string(),
        field_type,
        len,
        offset_getter_fn_name,
    });

    Ok(())
}
