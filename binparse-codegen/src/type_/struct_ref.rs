use std::collections::HashMap;

use quote::{format_ident, quote};

use crate::{
    GeneratedLen,
    field::FieldAccum,
    struct_::{DoneFieldType, GeneratedStruct},
    type_::{Error, GeneratedTypeInfo},
};

pub(crate) fn generate(
    struct_name: &str,
    done: &HashMap<&str, GeneratedStruct>,
    _accum: &mut FieldAccum,
    start_offset: GeneratedLen,
) -> Result<GeneratedTypeInfo, Error> {
    let generated_struct = done
        .get(struct_name)
        .ok_or_else(|| Error::UnknownType(struct_name.to_string()))?;

    let len = generated_struct.len.clone();
    let struct_ident = format_ident!("{}", struct_name);
    let return_ty = quote! { #struct_ident<'_> };

    match start_offset {
        GeneratedLen::Fixed(offset) => {
            if offset.bit != 0 {
                return Err(Error::InvalidAlignment(offset));
            }
            let start_byte = offset.byte;

            let field_getter_body = quote! {
                #struct_ident::parse(&self.data[#start_byte..]).unwrap().0
            };

            Ok(GeneratedTypeInfo {
                len,
                field_getter_body,
                return_ty,
                field_type: DoneFieldType::Other,
            })
        }

        GeneratedLen::Dynamic(_) => todo!(),
    }
}
