use binparse_dsl as ast;
use quote::quote;

use crate::{
    GeneratedLen,
    attr::Endian,
    struct_::DoneFieldType,
    type_::{Error, GeneratedTypeInfo},
};

pub(crate) fn generate(
    primitive: ast::Primitive,
    start_offset: GeneratedLen,
    endian: Endian,
) -> Result<GeneratedTypeInfo, Error> {
    let (len, def) = crate::match_primitive(&primitive);
    let return_ty = def.clone();

    match start_offset {
        GeneratedLen::Fixed(offset) => {
            if offset.bit != 0 {
                return Err(Error::InvalidAlignment(offset));
            }

            let start_byte = offset.byte;

            let field_getter_body = if matches!(primitive, ast::Primitive::U8) {
                quote! { self.data[#start_byte] }
            } else {
                let end = offset + len;
                let end_byte = end.byte;
                match endian {
                    Endian::Big => quote! {
                        #def::from_be_bytes(self.data[#start_byte..#end_byte].try_into().unwrap())
                    },
                    Endian::Little => quote! {
                        #def::from_le_bytes(self.data[#start_byte..#end_byte].try_into().unwrap())
                    },
                }
            };

            Ok(GeneratedTypeInfo {
                len: GeneratedLen::Fixed(len),
                field_getter_body,
                return_ty,
                field_type: DoneFieldType::Primitive,
            })
        }

        GeneratedLen::Dynamic(_) => todo!(),
    }
}
