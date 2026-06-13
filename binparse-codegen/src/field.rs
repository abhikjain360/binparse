use std::collections::HashMap;

use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{
    GeneratedLen,
    attr::{self, Hook, ParsedAttrs},
    expr,
    struct_::{DoneField, DoneFieldType, GeneratedStruct, StructAccum},
    type_,
};

pub(crate) struct FieldAccum {
    pub(crate) field_name: syn::Ident,
    pub(crate) len: GeneratedLen,
    pub(crate) field_type: DoneFieldType,
    pub(crate) offset_getter_fn_name: syn::Ident,
    pub(crate) definitions: TokenStream,
    pub(crate) helper_fns: TokenStream,
    pub(crate) field_getter: TokenStream,
    pub(crate) offset_getter: TokenStream,
    pub(crate) parse_checks: TokenStream,
    pub(crate) pre_length_checks: TokenStream,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("type generation error: {0}")]
    Type(#[from] type_::Error),
    #[error("cannot determine field offset: no start offset and no previous fields")]
    UnknownOffset,
    #[error(transparent)]
    Attr(#[from] attr::Error),
    #[error(transparent)]
    Expr(#[from] expr::Error),
    #[error("constant field literal width {width} is not supported")]
    ConstantTooWide { width: u8 },
    #[error("@range minimum {min} is greater than maximum {max}")]
    InvalidRange { min: usize, max: usize },
    #[error("@align({align}) field starts at misaligned offset {offset:?}")]
    MisalignedField { offset: binparse::Len, align: usize },
}

impl FieldAccum {
    pub(crate) fn new(field_name: &str) -> Self {
        let field_name_ident = format_ident!("{}", field_name);
        let offset_getter_fn_name = format_ident!("{}_end_offset", field_name);
        Self {
            field_name: field_name_ident,
            len: GeneratedLen::Fixed(binparse::Len { byte: 0, bit: 0 }),
            field_type: DoneFieldType::Other,
            offset_getter_fn_name,
            definitions: TokenStream::new(),
            helper_fns: TokenStream::new(),
            field_getter: TokenStream::new(),
            offset_getter: TokenStream::new(),
            parse_checks: TokenStream::new(),
            pre_length_checks: TokenStream::new(),
        }
    }
}

pub(crate) fn generate<'a>(
    ast: &ast::Field<'a>,
    done: &HashMap<&'a str, GeneratedStruct>,
    struct_accum: &mut StructAccum,
    errors: &[ast::ErrorVariant<'_>],
) -> Result<(), Error> {
    let attrs = ParsedAttrs::parse(&ast.attributes)?;
    let field_inherited = attrs.merge_inherited(struct_accum.inherited);
    let mut field_accum = FieldAccum::new(ast.name);

    let has_padding = attrs.pad.is_some() || attrs.pad_to.is_some();
    if let Some(pad) = attrs.pad {
        struct_accum.offset = struct_accum.offset.clone()
            + GeneratedLen::Fixed(binparse::Len { byte: pad, bit: 0 });
    }
    if let Some(pad_to) = attrs.pad_to {
        struct_accum.offset = match struct_accum.offset.clone() {
            GeneratedLen::Fixed(len) => GeneratedLen::Fixed(len.pad_to(pad_to)),
            GeneratedLen::Dynamic(tokens) => {
                GeneratedLen::Dynamic(quote! { ({ #tokens }).pad_to(#pad_to) })
            }
        };
    }
    let dynamic_align = match (attrs.align, &struct_accum.offset) {
        (Some(align), GeneratedLen::Fixed(len)) => {
            if len.bit != 0 || len.byte % align != 0 {
                return Err(Error::MisalignedField { offset: *len, align });
            }
            None
        }
        (Some(align), GeneratedLen::Dynamic(_)) => Some(align),
        (None, _) => None,
    };

    let mut constant = None;
    match &ast.value {
        ast::FieldValue::Type(ty) => {
            if attrs.endian.is_some() {
                check_endian_applies(ty)?;
            }
            if attrs.bit_order.is_some() {
                check_bit_order_applies(ty)?;
            }
            check_array_attrs_apply(&attrs, ty)?;
            check_len_applies(&attrs, ty)?;

            if struct_accum.condition.is_some() && matches!(ty, ast::Type::Concat(_)) {
                todo!("concat fields inside conditionals");
            }
            if struct_accum.condition.is_some() && attrs.len.is_some() {
                todo!("@len fields inside conditionals");
            }

            match (&attrs.hook, is_vla(ty)) {
                (Some(_), _) if struct_accum.condition.is_some() => {
                    todo!("hooks inside conditionals");
                }
                (Some(hook), true) => {
                    generate_vla_hook(hook, struct_accum, &mut field_accum, &attrs)?;
                }
                (Some(hook), false) => {
                    generate_fixed_hook(
                        hook,
                        ty,
                        done,
                        struct_accum,
                        &mut field_accum,
                        field_inherited,
                        &attrs,
                        errors,
                    )?;
                }
                (None, _) => {
                    generate_plain(
                        ty,
                        done,
                        struct_accum,
                        &mut field_accum,
                        field_inherited,
                        &attrs,
                        errors,
                    )?;
                }
            }
        }

        ast::FieldValue::Constraint(expr) => {
            let ast::Expr::Literal(ast::Literal::Int(lit)) = expr else {
                todo!("non-literal constraint fields")
            };
            let ty = constant_type(lit)?;
            if attrs.endian.is_some() {
                check_endian_applies(&ty)?;
            }
            if attrs.bit_order.is_some() {
                check_bit_order_applies(&ty)?;
            }
            check_array_attrs_apply(&attrs, &ty)?;
            check_len_applies(&attrs, &ty)?;
            generate_plain(
                &ty,
                done,
                struct_accum,
                &mut field_accum,
                field_inherited,
                &attrs,
                errors,
            )?;
            constant = Some(lit.value);
        }
    };

    let offset_getter_fn_name = field_accum.offset_getter_fn_name;
    let len = field_accum.len;
    let field_type = field_accum.field_type;

    let start_offset = std::mem::replace(
        &mut struct_accum.offset,
        GeneratedLen::Fixed(binparse::Len { byte: 0, bit: 0 }),
    );
    let end_offset = start_offset.clone() + len.clone();

    let start_offset_getter_fn_name = format_ident!("{}_start_offset", ast.name);
    let bit_range_fn_name = format_ident!("{}_bit_range", ast.name);
    let start_offset_body = if has_padding {
        match &start_offset {
            GeneratedLen::Fixed(len) => {
                let byte = len.byte;
                let bit = len.bit;
                quote! { binparse::Len { byte: #byte, bit: #bit } }
            }
            GeneratedLen::Dynamic(tokens) => quote! { #tokens },
        }
    } else {
        match &struct_accum.last_offset_getter_fn_name {
            Some(prev) => quote! { self.#prev() },
            None => quote! { binparse::Len::ZERO },
        }
    };

    let (vis, dead_code) = getter_visibility(&attrs);
    field_accum.offset_getter = match &end_offset {
        GeneratedLen::Fixed(total_len) => {
            let total_byte = total_len.byte;
            let total_bit = total_len.bit;
            quote! {
                #dead_code
                #vis fn #offset_getter_fn_name(&self) -> binparse::Len {
                    binparse::Len { byte: #total_byte, bit: #total_bit }
                }
            }
        }
        GeneratedLen::Dynamic(total_len) => {
            quote! {
                #dead_code
                #vis fn #offset_getter_fn_name(&self) -> binparse::Len {
                    #total_len
                }
            }
        }
    };
    field_accum.offset_getter.extend(quote! {
        #dead_code
        #vis fn #start_offset_getter_fn_name(&self) -> binparse::Len {
            #start_offset_body
        }

        #dead_code
        #vis fn #bit_range_fn_name(&self) -> ::core::ops::Range<usize> {
            self.#start_offset_getter_fn_name().bits()..self.#offset_getter_fn_name().bits()
        }
    });

    struct_accum.offset = end_offset;
    struct_accum.field_definitions.extend(field_accum.definitions);
    struct_accum.functions.extend(field_accum.helper_fns);
    struct_accum.functions.extend(field_accum.field_getter);
    struct_accum.functions.extend(field_accum.offset_getter);
    let mut length_check = TokenStream::new();
    if let Some(align) = dynamic_align {
        let field_path = format!("{}.{}", struct_accum.name, ast.name);
        length_check.extend(quote! {
            {
                let offset = me.#start_offset_getter_fn_name();
                if !offset.is_byte_aligned() || !offset.byte.is_multiple_of(#align) {
                    return Err(binparse::ParseError::Misaligned {
                        field: #field_path,
                        align: #align,
                        offset,
                    });
                }
            }
        });
    }
    length_check.extend(std::mem::take(&mut field_accum.pre_length_checks));
    length_check.extend(quote! {
        {
            let len = me.#offset_getter_fn_name();
            let expected = len.byte_ceil();
            if data.len() < expected {
                return Err(binparse::ParseError::NotEnoughData {
                    expected,
                    got: data.len(),
                });
            }
        }
    });
    match &struct_accum.condition {
        Some(gate) => struct_accum.parse_checks.extend(quote! {
            if me.#gate() {
                #length_check
            }
        }),
        None => struct_accum.parse_checks.extend(length_check),
    }
    struct_accum
        .parse_checks
        .extend(std::mem::take(&mut field_accum.parse_checks));
    struct_accum.last_offset_getter_fn_name = Some(offset_getter_fn_name);
    struct_accum.done_fields.push(DoneField {
        name: ast.name.to_string(),
        field_type,
        conditional: struct_accum.condition.is_some(),
    });

    generate_validations(ast, &attrs, constant, field_type, struct_accum)?;

    Ok(())
}

fn generate_validations<'a>(
    ast: &ast::Field<'a>,
    attrs: &ParsedAttrs<'a>,
    constant: Option<usize>,
    field_type: DoneFieldType,
    struct_accum: &mut StructAccum,
) -> Result<(), Error> {
    if constant.is_none() && attrs.check.is_none() && attrs.range.is_none() {
        return Ok(());
    }

    if struct_accum.condition.is_some() {
        todo!("validations on conditional fields");
    }

    if !matches!(
        field_type,
        DoneFieldType::Primitive | DoneFieldType::BitField
    ) {
        return Err(attr::Error::ValidationOnNonNumeric.into());
    }

    let field_name = format_ident!("{}", ast.name);
    let field_path = format!("{}.{}", struct_accum.name, ast.name);
    let validation_error = quote! {
        binparse::ParseError::ValidationFailed {
            field: #field_path,
            actual: self.#field_name() as u128,
        }
    };

    let mut validations = TokenStream::new();

    if let Some(value) = constant {
        let expected = proc_macro2::Literal::u128_unsuffixed(value as u128);
        validations.extend(quote! {
            if self.#field_name() != #expected {
                return Err(#validation_error);
            }
        });
    }

    if let Some((min, max)) = &attrs.range {
        let min = expr::lower(min, expr::ExprType::Numeric, &struct_accum.done_fields)?;
        let max = expr::lower(max, expr::ExprType::Numeric, &struct_accum.done_fields)?;
        if let (Some(min_value), Some(max_value)) = (min.const_value, max.const_value)
            && min_value > max_value
        {
            return Err(Error::InvalidRange {
                min: min_value,
                max: max_value,
            });
        }
        let min_tokens = min.tokens;
        let max_tokens = max.tokens;
        validations.extend(quote! {
            if !((#min_tokens)..=(#max_tokens)).contains(&(self.#field_name() as usize)) {
                return Err(#validation_error);
            }
        });
    }

    if let Some(check) = &attrs.check {
        let check = expr::lower(check, expr::ExprType::Bool, &struct_accum.done_fields)?;
        let check_tokens = check.tokens;
        validations.extend(quote! {
            if !#check_tokens {
                return Err(#validation_error);
            }
        });
    }

    let validate_fn_name = format_ident!("{}_validate", ast.name);
    struct_accum.functions.extend(quote! {
        #[allow(clippy::unnecessary_cast)]
        fn #validate_fn_name(&self) -> Result<(), binparse::ParseError> {
            #validations
            Ok(())
        }
    });
    struct_accum.parse_checks.extend(quote! {
        me.#validate_fn_name()?;
    });

    Ok(())
}

fn generate_plain<'a>(
    ty: &ast::Type<'a>,
    done: &HashMap<&'a str, GeneratedStruct>,
    struct_accum: &mut StructAccum,
    field_accum: &mut FieldAccum,
    inherited: attr::Inherited,
    attrs: &ParsedAttrs<'a>,
    errors: &[ast::ErrorVariant<'_>],
) -> Result<(), Error> {
    let start_offset = struct_accum.offset.clone();
    let info = type_::generate(
        ty,
        done,
        struct_accum,
        field_accum,
        start_offset,
        inherited,
        attrs,
        errors,
    )?;

    field_accum.len = info.len;
    field_accum.field_type = info.field_type;

    let field_name = field_accum.field_name.clone();
    let return_ty = info.return_ty;
    let field_getter_body = info.field_getter_body;
    let (vis, dead_code) = getter_visibility(attrs);
    match &struct_accum.condition {
        Some(gate) => {
            let raw_fn_name = format_ident!("{}_raw", field_name);
            field_accum.helper_fns.extend(quote! {
                #[allow(clippy::identity_op)]
                fn #raw_fn_name(&self) -> #return_ty {
                    #field_getter_body
                }
            });
            field_accum.field_getter = quote! {
                #dead_code
                #vis fn #field_name(&self) -> Option<#return_ty> {
                    if self.#gate() {
                        Some(self.#raw_fn_name())
                    } else {
                        None
                    }
                }
            };
        }
        None => {
            field_accum.field_getter = quote! {
                #dead_code
                #[allow(clippy::identity_op)]
                #vis fn #field_name(&self) -> #return_ty {
                    #field_getter_body
                }
            };
        }
    }

    Ok(())
}

pub(crate) fn getter_visibility(attrs: &ParsedAttrs<'_>) -> (TokenStream, TokenStream) {
    if attrs.skip {
        (TokenStream::new(), quote! { #[allow(dead_code)] })
    } else {
        (quote! { pub }, TokenStream::new())
    }
}

fn constant_type(lit: &ast::IntLiteral) -> Result<ast::Type<'static>, Error> {
    match lit.ty {
        ast::IntType::Binary => match lit.width {
            1..=8 => Ok(ast::Type::BitField(lit.width)),
            _ => Err(Error::ConstantTooWide { width: lit.width }),
        },
        ast::IntType::Hex => match usize::from(lit.width).div_ceil(2) {
            1 => Ok(ast::Type::Primitive(ast::Primitive::U8)),
            2 => Ok(ast::Type::Primitive(ast::Primitive::U16)),
            3..=4 => Ok(ast::Type::Primitive(ast::Primitive::U32)),
            5..=8 => Ok(ast::Type::Primitive(ast::Primitive::U64)),
            9..=16 => Ok(ast::Type::Primitive(ast::Primitive::U128)),
            _ => Err(Error::ConstantTooWide { width: lit.width }),
        },
        ast::IntType::Decimal => {
            let primitive = if lit.value <= usize::from(u8::MAX) {
                ast::Primitive::U8
            } else if lit.value <= usize::from(u16::MAX) {
                ast::Primitive::U16
            } else if u32::try_from(lit.value).is_ok() {
                ast::Primitive::U32
            } else {
                ast::Primitive::U64
            };
            Ok(ast::Type::Primitive(primitive))
        }
    }
}

fn is_vla(ty: &ast::Type<'_>) -> bool {
    matches!(ty, ast::Type::Array(ast::ArrayType { size: None, .. }))
}

fn check_endian_applies(ty: &ast::Type<'_>) -> Result<(), attr::Error> {
    match ty {
        ast::Type::Primitive(ast::Primitive::U8 | ast::Primitive::I8) => {
            Err(attr::Error::EndianOnSingleByte)
        }
        ast::Type::BitField(_) => Err(attr::Error::EndianOnBitfield),
        ast::Type::StructRef(_) => Err(attr::Error::EndianOnStructRef),
        ast::Type::Array(ast::ArrayType { elem_ty, .. }) => match elem_ty {
            ast::ArrayElemType::Primitive(ast::Primitive::U8 | ast::Primitive::I8) => {
                Err(attr::Error::EndianOnSingleByte)
            }
            ast::ArrayElemType::BitField(_) => Err(attr::Error::EndianOnBitfield),
            ast::ArrayElemType::StructRef(_) => Err(attr::Error::EndianOnStructRef),
            ast::ArrayElemType::Primitive(_) => Ok(()),
        },
        _ => Ok(()),
    }
}

fn check_array_attrs_apply(attrs: &ParsedAttrs<'_>, ty: &ast::Type<'_>) -> Result<(), attr::Error> {
    let array_attrs = [
        ("until", attrs.until.is_some()),
        ("greedy", attrs.greedy),
        ("max_iter", attrs.max_iter.is_some()),
    ];
    for (name, present) in array_attrs {
        if !present {
            continue;
        }
        if attrs.hook.is_some() {
            return Err(attr::Error::ArrayAttrWithHook(name));
        }
        if !matches!(ty, ast::Type::Array(_)) {
            return Err(attr::Error::ArrayAttrOnNonArray(name));
        }
    }
    if attrs.until.is_some() && attrs.greedy {
        return Err(attr::Error::UntilWithGreedy);
    }
    Ok(())
}

fn check_len_applies(attrs: &ParsedAttrs<'_>, ty: &ast::Type<'_>) -> Result<(), attr::Error> {
    if attrs.len.is_none() {
        return Ok(());
    }
    if attrs.hook.is_some() {
        todo!("@len with @hook");
    }
    match ty {
        ast::Type::StructRef(_) => Ok(()),
        ast::Type::Array(_) | ast::Type::Concat(_) | ast::Type::Union(_) => {
            todo!("@len on array, concat, and union fields")
        }
        ast::Type::Primitive(_) | ast::Type::BitField(_) => Err(attr::Error::LenOnNonStructRef),
    }
}

fn check_bit_order_applies(ty: &ast::Type<'_>) -> Result<(), attr::Error> {
    match ty {
        ast::Type::BitField(_)
        | ast::Type::Array(ast::ArrayType {
            elem_ty: ast::ArrayElemType::BitField(_),
            ..
        })
        | ast::Type::Concat(_)
        | ast::Type::Union(_) => Ok(()),
        _ => Err(attr::Error::BitOrderOnNonBitfield),
    }
}

fn generate_vla_hook(
    hook: &Hook,
    struct_accum: &mut StructAccum,
    field_accum: &mut FieldAccum,
    attrs: &ParsedAttrs<'_>,
) -> Result<(), Error> {
    let field_name = &field_accum.field_name;
    let hook_fn = &hook.fn_path;
    let return_ty = &hook.return_ty;

    let raw_fn_name = format_ident!("{}_raw", field_name);

    let start_offset = if attrs.pad.is_some() || attrs.pad_to.is_some() {
        match &struct_accum.offset {
            GeneratedLen::Fixed(len) => {
                let byte = len.byte;
                quote! { #byte }
            }
            GeneratedLen::Dynamic(tokens) => quote! { ({ #tokens }).byte },
        }
    } else {
        match &struct_accum.last_offset_getter_fn_name {
            Some(prev) => quote! { self.#prev().byte },
            None => quote! { 0 },
        }
    };

    field_accum.helper_fns = quote! {
        fn #raw_fn_name(&self) -> (#return_ty, usize) {
            #hook_fn(&self.data[#start_offset..])
        }
    };

    let (vis, dead_code) = getter_visibility(attrs);
    field_accum.field_getter = quote! {
        #dead_code
        #vis fn #field_name(&self) -> #return_ty {
            self.#raw_fn_name().0
        }
    };

    let len_expr = quote! {
        binparse::Len { byte: self.#raw_fn_name().1, bit: 0 }
    };
    field_accum.len = GeneratedLen::Dynamic(len_expr);
    field_accum.field_type = DoneFieldType::Other;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn generate_fixed_hook<'a>(
    hook: &Hook,
    ty: &ast::Type<'a>,
    done: &HashMap<&'a str, GeneratedStruct>,
    struct_accum: &mut StructAccum,
    field_accum: &mut FieldAccum,
    inherited: attr::Inherited,
    attrs: &ParsedAttrs<'a>,
    errors: &[ast::ErrorVariant<'_>],
) -> Result<(), Error> {
    let start_offset = struct_accum.offset.clone();
    let info = type_::generate(
        ty,
        done,
        struct_accum,
        field_accum,
        start_offset,
        inherited,
        attrs,
        errors,
    )?;

    field_accum.len = info.len;
    field_accum.field_type = DoneFieldType::Other;

    let field_name = &field_accum.field_name;
    let hook_fn = &hook.fn_path;
    let return_ty = &hook.return_ty;
    let field_getter_body = info.field_getter_body;

    let (vis, dead_code) = getter_visibility(attrs);
    field_accum.field_getter = quote! {
        #dead_code
        #[allow(clippy::identity_op)]
        #vis fn #field_name(&self) -> #return_ty {
            #hook_fn(#field_getter_body)
        }
    };

    Ok(())
}
