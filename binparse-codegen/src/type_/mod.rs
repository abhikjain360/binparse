use binparse::Len;
use binparse_dsl as ast;
use proc_macro2::TokenStream;

use crate::{GeneratedLen, field::FieldAccum, struct_::DoneFieldType};

pub(crate) mod array;
pub(crate) mod bitfield;
pub(crate) mod concat;
pub(crate) mod primitive;
pub(crate) mod struct_ref;
pub(crate) mod union_;

pub(crate) struct TypeAccum<'a> {
    pub(crate) field_accum: &'a mut FieldAccum<'a>,
    pub(crate) definitions: TokenStream,
    pub(crate) helper_fns: TokenStream,
}

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

impl<'a> TypeAccum<'a> {
    pub(crate) fn new(field_accum: &'a mut FieldAccum<'a>) -> Self {
        Self {
            field_accum,
            definitions: TokenStream::new(),
            helper_fns: TokenStream::new(),
        }
    }
}

pub(crate) fn generate(
    ast: &ast::Type<'_>,
    field_accum: &mut FieldAccum<'_>,
) -> Result<GeneratedTypeInfo, Error> {
    let start_offset = field_accum.struct_accum.offset.clone() + field_accum.len.clone();
    match ast {
        ast::Type::Primitive(p) => primitive::generate(*p, start_offset),
        ast::Type::BitField(width) => bitfield::generate(*width as usize, start_offset),
        ast::Type::Concat(items) => concat::generate(items, field_accum, start_offset),
        ast::Type::StructRef(struct_name) => {
            struct_ref::generate(struct_name, field_accum, start_offset)
        }
        ast::Type::Array(array_type) => array::generate(array_type, field_accum, start_offset),
        ast::Type::Union(u) => union_::generate(u, field_accum, start_offset),
    }
}
