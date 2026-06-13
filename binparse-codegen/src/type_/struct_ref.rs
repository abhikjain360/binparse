use std::collections::HashMap;

use binparse::Len;
use quote::{format_ident, quote};

use crate::{
    GeneratedLen,
    attr::ParsedAttrs,
    expr::{self, ExprType},
    field::{FieldAccum, getter_visibility},
    struct_::{DoneFieldType, GeneratedStruct, StructAccum},
    type_::{Error, GeneratedTypeInfo},
};

pub(crate) fn generate(
    struct_name: &str,
    done: &HashMap<&str, GeneratedStruct>,
    struct_accum: &mut StructAccum,
    accum: &mut FieldAccum,
    start_offset: GeneratedLen,
    attrs: &ParsedAttrs<'_>,
) -> Result<GeneratedTypeInfo, Error> {
    let generated_struct = done
        .get(struct_name)
        .ok_or_else(|| Error::UnknownType(struct_name.to_string()))?;

    let struct_ident = format_ident!("{}", struct_name);
    let return_ty = quote! { ::binparse::ParseResult<#struct_ident<'_>> };

    if let Some(len_expr) = &attrs.len {
        let lowered = expr::lower(len_expr, ExprType::Numeric, &struct_accum.done_fields)?;
        if let (Some(bound), GeneratedLen::Fixed(inner_len)) =
            (lowered.const_value, &generated_struct.len)
            && bound < inner_len.byte_ceil()
        {
            return Err(Error::LenBoundTooSmall {
                bound,
                needed: inner_len.byte_ceil(),
            });
        }

        let start = match &start_offset {
            GeneratedLen::Fixed(offset) => {
                if offset.bit != 0 {
                    return Err(Error::InvalidAlignment(*offset));
                }
                let byte = offset.byte;
                quote! { #byte }
            }
            GeneratedLen::Dynamic(tokens) => quote! {{
                let len = #tokens;
                if len.bit > 0 { return Err(::binparse::ParseError::UnalignedLength(len)) };
                len.byte
            }},
        };

        let len_tokens = lowered.tokens;
        let field_getter_body = quote! {
            let start = #start;
            let end = start.saturating_add(#len_tokens);
            #struct_ident::parse(&self.data[start..end]).map(|(value, _)| value)
        };

        let rest_fn_name = format_ident!("{}_rest", accum.field_name);
        let (vis, dead_code) = getter_visibility(attrs);
        accum.helper_fns.extend(quote! {
            #dead_code
            #vis fn #rest_fn_name(&self) -> ::binparse::ParseResult<&[u8]> {
                let start = #start;
                let end = start.saturating_add(#len_tokens);
                #struct_ident::parse(&self.data[start..end]).map(|(_, rest)| rest)
            }
        });

        let len = match lowered.const_value {
            Some(byte) => GeneratedLen::Fixed(Len { byte, bit: 0 }),
            None => GeneratedLen::Dynamic(quote! {
                ::binparse::Len { byte: #len_tokens, bit: 0 }
            }),
        };

        return Ok(GeneratedTypeInfo {
            len,
            field_getter_body,
            return_ty,
            field_type: DoneFieldType::Other,
        });
    }

    let len = generated_struct.len.clone();

    match start_offset {
        GeneratedLen::Fixed(offset) => {
            if offset.bit != 0 {
                return Err(Error::InvalidAlignment(offset));
            }
            let start_byte = offset.byte;

            let field_getter_body = quote! {
                #struct_ident::parse(&self.data[#start_byte..]).map(|(value, _)| value)
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
