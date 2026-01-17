use std::collections::HashMap;

use binparse::Len;
use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{
    GeneratedLen,
    attr::{Endian, ParsedAttrs},
    field::FieldAccum,
    struct_::{DoneFieldType, GeneratedStruct, StructAccum},
    type_::{self, GeneratedTypeInfo},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("concat item {0} must have known length")]
    UnknownItemLen(usize),
    #[error(transparent)]
    Attr(#[from] crate::attr::Error),
}

pub(crate) fn generate<'a>(
    items: &[ast::ConcatItem<'a>],
    done: &HashMap<&'a str, GeneratedStruct>,
    struct_accum: &mut StructAccum,
    field_accum: &mut FieldAccum,
    start_offset: GeneratedLen,
    endian: Endian,
) -> Result<GeneratedTypeInfo, type_::Error> {
    let mut total_len = GeneratedLen::Fixed(Len::default());
    let mut field_types = Vec::new();
    let mut field_exprs = TokenStream::new();

    let mut current_offset = start_offset;

    for (i, item) in items.iter().enumerate() {
        let item_name = {
            let field_name = &field_accum.field_name;
            format_ident!("{}_{}", field_name, i)
        };

        let item_attrs = ParsedAttrs::parse(&item.attributes)?;
        let item_endian = item_attrs.merge_endian(endian);

        let info = type_::generate(
            &item.ty,
            done,
            struct_accum,
            field_accum,
            current_offset.clone(),
            item_endian,
        )?;

        let return_ty = info.return_ty;
        let field_getter_body = info.field_getter_body;
        field_accum.helper_fns.extend(quote! {
            #[allow(clippy::identity_op)]
            pub fn #item_name(&self) -> #return_ty {
                #field_getter_body
            }
        });

        field_types.push(return_ty);
        field_exprs.extend(quote! { self.#item_name(), });

        let item_len = info.len;
        total_len = total_len + item_len.clone();
        current_offset = item_len + current_offset;
    }

    let field_getter_body = quote! {
        ( #field_exprs )
    };

    let return_ty = quote! { ( #(#field_types),* ) };

    Ok(GeneratedTypeInfo {
        len: total_len,
        field_getter_body,
        return_ty,
        field_type: DoneFieldType::Other,
    })
}
