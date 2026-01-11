use binparse_dsl as ast;
use proc_macro2::TokenStream;

use crate::Len;

pub(crate) struct FieldCtx<'a> {
    pub(crate) field: &'a ast::Field<'a>,
    /// The offset of this field from the start of the struct.
    /// `None` means the offset is not statically known (there was a non-sized field before this one).
    pub(crate) offset: Option<Len>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {}

pub(crate) struct GeneratedField {
    /// The length of this field. `None` if not statically known.
    pub(crate) len: Option<Len>,
    /// Struct field definitions (only for fields that need stored offsets).
    pub(crate) definitions: TokenStream,
    /// The getter function for this field's value.
    pub(crate) field_getter: TokenStream,
    /// The getter function for this field's offset.
    pub(crate) offset_getter: TokenStream,
}

impl<'a> FieldCtx<'a> {
    pub(crate) fn new(field: &'a ast::Field<'a>, offset: Option<Len>) -> Self {
        Self { field, offset }
    }

    pub(crate) fn generate(self) -> Result<GeneratedField, Error> {
        todo!()
    }
}
