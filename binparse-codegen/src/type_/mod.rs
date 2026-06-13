use std::collections::HashMap;

use binparse::Len;
use binparse_dsl as ast;
use proc_macro2::TokenStream;

use crate::{
    GeneratedLen,
    attr::{Inherited, ParsedAttrs},
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
    #[error("array without size requires @until, @greedy, or @hook")]
    UnsizedArray,
    #[error("@greedy element type has zero length")]
    GreedyZeroSizedElem,
    #[error(transparent)]
    Concat(#[from] concat::Error),
    #[error(transparent)]
    Expr(#[from] crate::expr::Error),
    #[error(transparent)]
    Union(#[from] union_::Error),
    #[error("field error: {0}")]
    Field(Box<crate::field::Error>),
    #[error(transparent)]
    Attr(#[from] crate::attr::Error),
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn generate<'a>(
    ast: &ast::Type<'a>,
    done: &HashMap<&'a str, GeneratedStruct>,
    struct_accum: &mut StructAccum,
    field_accum: &mut FieldAccum,
    start_offset: GeneratedLen,
    inherited: Inherited,
    attrs: &ParsedAttrs<'a>,
    errors: &[ast::ErrorVariant<'_>],
) -> Result<GeneratedTypeInfo, Error> {
    match ast {
        ast::Type::Primitive(p) => primitive::generate(*p, start_offset, inherited.endian),
        ast::Type::BitField(width) => bitfield::generate(*width as usize, start_offset, inherited.bit_order),
        ast::Type::Concat(items) => {
            concat::generate(items, done, struct_accum, field_accum, start_offset, inherited, errors)
        }
        ast::Type::StructRef(struct_name) => {
            struct_ref::generate(struct_name, done, field_accum, start_offset)
        }
        ast::Type::Array(array_type) => {
            array::generate(array_type, attrs, done, struct_accum, field_accum, start_offset, inherited)
        }
        ast::Type::Union(u) => union_::generate(u, done, struct_accum, field_accum, start_offset, inherited, errors),
    }
}
