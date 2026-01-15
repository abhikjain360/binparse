use std::collections::HashMap;

use binparse::Len;
use binparse_dsl as ast;
use proc_macro2::TokenStream;

use crate::{
    GeneratedLen,
    field::FieldAccum,
    struct_::{DoneFieldType, GeneratedStruct, StructAccum},
};

pub(crate) mod array;
pub(crate) mod bitfield;
pub(crate) mod concat;
pub(crate) mod primitive;
pub(crate) mod struct_ref;
pub(crate) mod union_;

pub(crate) struct GeneratedTypeInfo {
    pub(crate) len: GeneratedLen,
    pub(crate) field_getter_body: TokenStream,
    pub(crate) return_ty: TokenStream,
    pub(crate) field_type: DoneFieldType,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("type needs alignment but is not aligned itself")]
    UnalignedType,
    #[error("type must have a known size")]
    UnsizedType,
    #[error("type needs alignment, but the start offset ({0:?}) is not aligned")]
    InvalidAlignment(Len),
    #[error("unknown type: {0}")]
    UnknownType(String),
    #[error(transparent)]
    Concat(#[from] concat::Error),
    #[error(transparent)]
    Array(#[from] array::Error),
    #[error(transparent)]
    Union(#[from] union_::Error),
    #[error("field error: {0}")]
    Field(Box<crate::field::Error>),
}

pub(crate) fn generate<'a>(
    ast: &ast::Type<'a>,
    done: &HashMap<&'a str, GeneratedStruct>,
    struct_accum: &mut StructAccum,
    field_accum: &mut FieldAccum,
    start_offset: GeneratedLen,
) -> Result<GeneratedTypeInfo, Error> {
    match ast {
        ast::Type::Primitive(p) => primitive::generate(*p, start_offset),
        ast::Type::BitField(width) => bitfield::generate(*width as usize, start_offset),
        ast::Type::Concat(items) => {
            concat::generate(items, done, struct_accum, field_accum, start_offset)
        }
        ast::Type::StructRef(struct_name) => {
            struct_ref::generate(struct_name, done, field_accum, start_offset)
        }
        ast::Type::Array(array_type) => {
            array::generate(array_type, done, struct_accum, field_accum, start_offset)
        }
        ast::Type::Union(u) => union_::generate(u, done, struct_accum, field_accum, start_offset),
    }
}
