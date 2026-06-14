use std::collections::HashMap;

use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{
    GeneratedLen,
    attr::{self, Hook, ParsedAttrs},
    expr,
    struct_::{DoneField, DoneFieldType, GeneratedStruct, Payload, StructAccum},
    type_,
};

pub(crate) struct FieldAccum {
    pub(crate) field_name: syn::Ident,
    pub(crate) tree_getter: syn::Ident,
    pub(crate) len: GeneratedLen,
    pub(crate) field_type: DoneFieldType,
    pub(crate) offset_getter_fn_name: syn::Ident,
    pub(crate) definitions: TokenStream,
    pub(crate) helper_fns: TokenStream,
    pub(crate) field_getter: TokenStream,
    pub(crate) offset_getter: TokenStream,
    pub(crate) parse_checks: TokenStream,
    pub(crate) pre_length_checks: TokenStream,
    pub(crate) tree_body: TokenStream,
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
    #[error("concat fields are not supported inside conditionals")]
    ConcatInConditional,
    #[error("@len fields are not supported inside conditionals")]
    LenInConditional,
    #[error("non-literal constraint fields are not supported")]
    NonLiteralConstraint,
    #[error("validations are not supported on conditional fields")]
    ValidationOnConditional,
}

impl FieldAccum {
    pub(crate) fn new(field_name: &str) -> Self {
        let field_name_ident = format_ident!("{}", field_name);
        let offset_getter_fn_name = format_ident!("{}_end_offset", field_name);
        Self {
            field_name: field_name_ident.clone(),
            tree_getter: field_name_ident,
            len: GeneratedLen::Fixed(binparse::Len { byte: 0, bit: 0 }),
            field_type: DoneFieldType::Other,
            offset_getter_fn_name,
            definitions: TokenStream::new(),
            helper_fns: TokenStream::new(),
            field_getter: TokenStream::new(),
            offset_getter: TokenStream::new(),
            parse_checks: TokenStream::new(),
            pre_length_checks: TokenStream::new(),
            tree_body: TokenStream::new(),
        }
    }
}

pub(crate) fn generate<'a>(
    ast: &ast::Field<'a>,
    done: &HashMap<&'a str, GeneratedStruct>,
    struct_accum: &mut StructAccum<'a>,
    errors: &[ast::ErrorVariant<'_>],
) -> Result<(), Error> {
    let attrs = ParsedAttrs::parse(&ast.attributes)?;
    let field_inherited = attrs.merge_inherited(struct_accum.inherited);
    let mut field_accum = FieldAccum::new(ast.name);

    let has_padding = attrs.pad.is_some() || attrs.pad_to.is_some();
    if let Some(pad) = attrs.pad {
        struct_accum.offset =
            struct_accum.offset.clone() + GeneratedLen::Fixed(binparse::Len { byte: pad, bit: 0 });
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
                return Err(Error::MisalignedField {
                    offset: *len,
                    align,
                });
            }
            None
        }
        (Some(align), GeneratedLen::Dynamic(_)) => Some(align),
        (None, _) => None,
    };

    let mut constant = None;
    let stub_label;
    match &ast.value {
        ast::FieldValue::Type(ty) => {
            stub_label = type_::type_label(ty);
            if attrs.endian.is_some() {
                check_endian_applies(ty)?;
            }
            if attrs.bit_order.is_some() {
                check_bit_order_applies(ty)?;
            }
            check_array_attrs_apply(&attrs, ty)?;
            check_len_applies(&attrs, ty)?;
            check_handoff_applies(&attrs, ty, struct_accum)?;

            if struct_accum.condition.is_some() && matches!(ty, ast::Type::Concat(_)) {
                return Err(Error::ConcatInConditional);
            }
            if struct_accum.condition.is_some() && attrs.len.is_some() {
                return Err(Error::LenInConditional);
            }

            match (&attrs.hook, is_vla(ty)) {
                (Some(hook), true) => {
                    if !matches!(
                        ty,
                        ast::Type::Array(ast::ArrayType {
                            elem_ty: ast::ArrayElemType::Primitive(ast::Primitive::U8),
                            ..
                        })
                    ) {
                        return Err(attr::Error::HookVlaNotU8.into());
                    }
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
                return Err(Error::NonLiteralConstraint);
            };
            let ty = constant_type(lit)?;
            stub_label = type_::type_label(&ty);
            if attrs.endian.is_some() {
                check_endian_applies(&ty)?;
            }
            if attrs.bit_order.is_some() {
                check_bit_order_applies(&ty)?;
            }
            check_array_attrs_apply(&attrs, &ty)?;
            check_len_applies(&attrs, &ty)?;
            check_handoff_applies(&attrs, &ty, struct_accum)?;
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

    if attrs.discriminator {
        struct_accum
            .discriminators
            .push(field_accum.field_name.clone());
    }
    if attrs.payload {
        struct_accum.payload = Some(Payload {
            start_offset_fn: start_offset_getter_fn_name.clone(),
            end_offset_fn: offset_getter_fn_name.clone(),
        });
    }

    let tree_body = if field_accum.tree_body.is_empty() {
        type_::opaque_node(ast.name, &stub_label)
    } else {
        std::mem::take(&mut field_accum.tree_body)
    };
    let hide = attrs.skip.then(|| quote! { .hide() });

    let pad_node_fn_name = format_ident!("{}_pad_node", ast.name);
    let pad_push = if has_padding {
        let pad_name = format!("{}_pad", ast.name);
        let prev_end = match &struct_accum.last_offset_getter_fn_name {
            Some(prev) => quote! { self.#prev() },
            None => quote! { binparse::Len::ZERO },
        };
        struct_accum.functions.extend(quote! {
            #[allow(dead_code)]
            fn #pad_node_fn_name(&self) -> Option<::binparse::FieldNode<'a>> {
                let bit_range = #prev_end.bits()..self.#start_offset_getter_fn_name().bits();
                if bit_range.start < bit_range.end {
                    Some(
                        ::binparse::FieldNode::new(
                                #pad_name,
                                "pad",
                                bit_range.clone(),
                                ::binparse::Value::bytes(self.data, &bit_range),
                            )
                            .hide(),
                    )
                } else {
                    None
                }
            }
        });
        quote! {
            if let Some(node) = me.#pad_node_fn_name() {
                children.push(node);
            }
        }
    } else {
        TokenStream::new()
    };

    let present_node_fn_name = format_ident!("{}_present_node", ast.name);
    struct_accum.functions.extend(quote! {
        #[allow(dead_code)]
        fn #present_node_fn_name(&self) -> ::binparse::FieldNode<'a> {
            let bit_range = self.#bit_range_fn_name();
            #tree_body #hide
        }
    });
    let present_push = quote! {
        children.push(me.#present_node_fn_name());
    };

    let absent_node_fn_name = format_ident!("{}_absent_node", ast.name);
    let absent_push = if struct_accum.condition.is_some() {
        let absent_name = ast.name;
        struct_accum.functions.extend(quote! {
            #[allow(dead_code)]
            fn #absent_node_fn_name(&self) -> ::binparse::FieldNode<'a> {
                let start = self.#start_offset_getter_fn_name().bits();
                ::binparse::FieldNode::new(
                        #absent_name,
                        #stub_label,
                        start..start,
                        ::binparse::Value::Absent,
                    )
                    .hide()
            }
        });
        quote! {
            children.push(me.#absent_node_fn_name());
        }
    } else {
        TokenStream::new()
    };

    match &struct_accum.condition {
        Some(gate) => {
            struct_accum.tree_stmts.extend(quote! {
                {
                    let me = self;
                    #pad_push
                    if me.#gate() {
                        #present_push
                    } else {
                        #absent_push
                    }
                }
            });
        }
        None => struct_accum.tree_stmts.extend(quote! {
            {
                let me = self;
                #pad_push
                #present_push
            }
        }),
    }

    struct_accum.offset = end_offset;
    struct_accum
        .field_definitions
        .extend(field_accum.definitions);
    struct_accum.functions.extend(field_accum.helper_fns);
    struct_accum.functions.extend(field_accum.field_getter);
    struct_accum.functions.extend(field_accum.offset_getter);

    let fatal_check_fn_name = format_ident!("{}_fatal_check", ast.name);
    let mut fatal_check = TokenStream::new();
    if let Some(align) = dynamic_align {
        let field_path = format!("{}.{}", struct_accum.name, ast.name);
        fatal_check.extend(quote! {
            {
                let offset = self.#start_offset_getter_fn_name();
                if !offset.is_byte_aligned() || !offset.byte.is_multiple_of(#align) {
                    return Err(::binparse::ParseError::Misaligned {
                        field: #field_path,
                        align: #align,
                        offset,
                    });
                }
            }
        });
    }
    fatal_check.extend(std::mem::take(&mut field_accum.pre_length_checks));
    fatal_check.extend(quote! {
        {
            let len = self.#offset_getter_fn_name();
            let expected = len.byte_ceil();
            if self.data.len() < expected {
                return Err(::binparse::ParseError::NotEnoughData {
                    expected,
                    got: self.data.len(),
                });
            }
        }
    });
    struct_accum.functions.extend(quote! {
        #[allow(dead_code)]
        fn #fatal_check_fn_name(&self) -> Result<(), ::binparse::ParseError> {
            #fatal_check
            Ok(())
        }
    });

    struct_accum.done_fields.push(DoneField {
        name: ast.name.to_string(),
        field_type,
        conditional: struct_accum.condition.is_some(),
    });

    let recoverable_check_fn_name = format_ident!("{}_recoverable_check", ast.name);
    let mut recoverable_check = std::mem::take(&mut field_accum.parse_checks);
    generate_validations(
        ast,
        &attrs,
        constant,
        field_type,
        struct_accum,
        &mut recoverable_check,
    )?;
    struct_accum.functions.extend(quote! {
        #[allow(dead_code)]
        fn #recoverable_check_fn_name(&self) -> Result<(), ::binparse::ParseError> {
            #recoverable_check
            Ok(())
        }
    });

    let field_checks = quote! {
        me.#fatal_check_fn_name()?;
        me.#recoverable_check_fn_name()?;
    };
    match &struct_accum.condition {
        Some(gate) => struct_accum.parse_checks.extend(quote! {
            if me.#gate() {
                #field_checks
            }
        }),
        None => struct_accum.parse_checks.extend(field_checks),
    }

    let field_name_str = ast.name;
    let dissect_field = quote! {
        match me.#fatal_check_fn_name() {
            Err(error) => {
                let start = me.#start_offset_getter_fn_name().bits();
                children.push(
                    ::binparse::FieldNode::new(
                            #field_name_str,
                            #stub_label,
                            start..start,
                            ::binparse::Value::Opaque,
                        )
                        .with_status(::binparse::Status::Error(error)),
                );
                fatal = Some(error);
            }
            Ok(()) => match me.#recoverable_check_fn_name() {
                Err(error) => {
                    let bit_range = me.#bit_range_fn_name();
                    children.push(
                        ::binparse::FieldNode::new(
                                #field_name_str,
                                #stub_label,
                                bit_range,
                                ::binparse::Value::Opaque,
                            )
                            .with_status(::binparse::Status::Error(error)),
                    );
                }
                Ok(()) => {
                    #present_push
                }
            },
        }
    };
    match &struct_accum.condition {
        Some(gate) => struct_accum.dissect_stmts.extend(quote! {
            if fatal.is_none() {
                #pad_push
                if me.#gate() {
                    #dissect_field
                } else {
                    #absent_push
                }
            }
        }),
        None => struct_accum.dissect_stmts.extend(quote! {
            if fatal.is_none() {
                #pad_push
                #dissect_field
            }
        }),
    }

    struct_accum.last_offset_getter_fn_name = Some(offset_getter_fn_name);

    Ok(())
}

fn generate_validations<'a>(
    ast: &ast::Field<'a>,
    attrs: &ParsedAttrs<'a>,
    constant: Option<usize>,
    field_type: DoneFieldType,
    struct_accum: &mut StructAccum<'a>,
    recoverable: &mut TokenStream,
) -> Result<(), Error> {
    if constant.is_none() && attrs.check.is_none() && attrs.range.is_none() {
        return Ok(());
    }

    if struct_accum.condition.is_some() {
        return Err(Error::ValidationOnConditional);
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
        ::binparse::ParseError::ValidationFailed {
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
        fn #validate_fn_name(&self) -> Result<(), ::binparse::ParseError> {
            #validations
            Ok(())
        }
    });
    recoverable.extend(quote! {
        self.#validate_fn_name()?;
    });

    Ok(())
}

fn generate_plain<'a>(
    ty: &ast::Type<'a>,
    done: &HashMap<&'a str, GeneratedStruct>,
    struct_accum: &mut StructAccum<'a>,
    field_accum: &mut FieldAccum,
    inherited: attr::Inherited,
    attrs: &ParsedAttrs<'a>,
    errors: &[ast::ErrorVariant<'_>],
) -> Result<(), Error> {
    let field_name = field_accum.field_name.clone();
    if struct_accum.condition.is_some() {
        field_accum.tree_getter = format_ident!("{}_raw", field_name);
    }
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

    let tree_getter = field_accum.tree_getter.clone();
    field_accum.tree_body = info.tree_node(&field_accum.field_name, &tree_getter);
    field_accum.len = info.len;
    field_accum.field_type = info.field_type;

    let return_ty = info.return_ty;
    let field_getter_body = info.field_getter_body;
    let (vis, dead_code) = getter_visibility(attrs);
    match &struct_accum.condition {
        Some(gate) => {
            let raw_fn_name = tree_getter.clone();
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
        if is_vla(ty) {
            return Ok(());
        }
        return Err(attr::Error::LenWithFixedHook);
    }
    match ty {
        ast::Type::StructRef(_) | ast::Type::Union(_) => Ok(()),
        ast::Type::Array(array) => {
            if matches!(array.elem_ty, ast::ArrayElemType::BitField(_)) {
                return Err(attr::Error::LenOnBitfieldArray);
            }
            if array.size.is_some() {
                return Err(attr::Error::LenOnSizedArray);
            }
            Ok(())
        }
        ast::Type::Concat(_) => Err(attr::Error::LenOnConcat),
        ast::Type::Primitive(_) | ast::Type::BitField(_) => Err(attr::Error::LenOnUnsupportedType),
    }
}

fn check_handoff_applies(
    attrs: &ParsedAttrs<'_>,
    ty: &ast::Type<'_>,
    struct_accum: &StructAccum<'_>,
) -> Result<(), attr::Error> {
    if !attrs.discriminator && !attrs.payload {
        return Ok(());
    }
    let name = if attrs.discriminator {
        "discriminator"
    } else {
        "payload"
    };
    if attrs.skip {
        return Err(attr::Error::HandoffOnSkip(name));
    }
    if struct_accum.condition.is_some() {
        return Err(attr::Error::HandoffInConditional(name));
    }
    if attrs.hook.is_some() {
        return Err(attr::Error::HandoffWithHook(name));
    }
    if attrs.discriminator {
        match ty {
            ast::Type::Primitive(_) | ast::Type::BitField(_) => {}
            ast::Type::Concat(_) | ast::Type::Union(_) => {
                return Err(attr::Error::DiscriminatorOnConcatOrUnion);
            }
            ast::Type::Array(_) | ast::Type::StructRef(_) => {
                return Err(attr::Error::DiscriminatorOnNonNumeric);
            }
        }
    }
    if attrs.payload {
        if struct_accum.payload.is_some() {
            return Err(attr::Error::MultiplePayloads);
        }
        match ty {
            ast::Type::Array(ast::ArrayType {
                elem_ty: ast::ArrayElemType::Primitive(ast::Primitive::U8),
                ..
            })
            | ast::Type::StructRef(_) => {}
            ast::Type::Concat(_) | ast::Type::Union(_) => {
                return Err(attr::Error::PayloadOnConcatOrUnion);
            }
            ast::Type::Primitive(_) | ast::Type::BitField(_) | ast::Type::Array(_) => {
                return Err(attr::Error::PayloadOnNonByteArray);
            }
        }
    }
    Ok(())
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

fn generate_vla_hook<'a>(
    hook: &Hook,
    struct_accum: &mut StructAccum<'a>,
    field_accum: &mut FieldAccum,
    attrs: &ParsedAttrs<'a>,
) -> Result<(), Error> {
    let field_name = &field_accum.field_name;
    let hook_fn = &hook.fn_path;
    let return_ty = &hook.return_ty;
    let field_path = format!("{}.{}", struct_accum.name, field_name);
    let raw_fn_name = format_ident!("{}_raw", field_name);

    let start = match &struct_accum.offset {
        GeneratedLen::Fixed(len) => {
            if len.bit != 0 {
                return Err(type_::Error::InvalidAlignment(*len).into());
            }
            let byte = len.byte;
            quote! { #byte }
        }
        GeneratedLen::Dynamic(tokens) => quote! {{
            let len = #tokens;
            if len.bit > 0 { return Err(::binparse::ParseError::UnalignedLength(len)) };
            len.byte
        }},
    };

    let (vis, dead_code) = getter_visibility(attrs);

    if let Some(len_expr) = &attrs.len {
        let lowered = expr::lower(len_expr, expr::ExprType::Numeric, &struct_accum.done_fields)?;
        let len_tokens = lowered.tokens;
        let rest_fn_name = format_ident!("{}_rest", field_name);

        field_accum.helper_fns = quote! {
            fn #raw_fn_name(&self) -> ::binparse::ParseResult<(#return_ty, usize)> {
                let start = #start;
                let window = start.saturating_add(#len_tokens).min(self.data.len());
                let (value, consumed) = #hook_fn(
                    &self.data[start.min(window)..window],
                    ::binparse::HookContext {
                        field: #field_path,
                        offset: start,
                        enclosing: self.data,
                    },
                )?;
                let end = start.saturating_add(consumed);
                if end > window {
                    return Err(::binparse::ParseError::NotEnoughData {
                        expected: end,
                        got: window,
                    });
                }
                Ok((value, consumed))
            }

            #dead_code
            #vis fn #rest_fn_name(&self) -> ::binparse::ParseResult<&'a [u8]> {
                let start = #start;
                let window = start.saturating_add(#len_tokens).min(self.data.len());
                let (_, consumed) = self.#raw_fn_name()?;
                let rest_start = start.saturating_add(consumed).min(window);
                Ok(&self.data[rest_start..window])
            }
        };

        field_accum.field_getter = quote! {
            #dead_code
            #vis fn #field_name(&self) -> ::binparse::ParseResult<#return_ty> {
                self.#raw_fn_name().map(|(value, _)| value)
            }
        };

        field_accum.len = match lowered.const_value {
            Some(byte) => GeneratedLen::Fixed(binparse::Len { byte, bit: 0 }),
            None => GeneratedLen::Dynamic(quote! {
                ::binparse::Len { byte: #len_tokens, bit: 0 }
            }),
        };
        field_accum.field_type = if hook_return_is_numeric(&hook.return_ty.to_string()) {
            DoneFieldType::Hook
        } else {
            DoneFieldType::Other
        };
        field_accum.pre_length_checks = quote! {
            self.#raw_fn_name()?;
        };

        let name_str = field_accum.field_name.to_string();
        let type_name = hook.return_ty.to_string();
        let bind = hook_value_binding(&type_name);
        let value = hook_value(&type_name, quote! { value });
        field_accum.tree_body = quote! {
            match self.#raw_fn_name() {
                Ok((#bind, consumed)) => {
                    let consumed_end = bit_range
                        .start
                        .saturating_add(consumed.saturating_mul(8))
                        .min(bit_range.end);
                    let mut node = ::binparse::FieldNode::new(
                        #name_str,
                        #type_name,
                        bit_range.clone(),
                        #value,
                    );
                    if let Ok(rest) = self.#rest_fn_name()
                        && !rest.is_empty()
                    {
                        node.children.push(::binparse::FieldNode::new(
                            "rest",
                            "[u8]",
                            consumed_end..bit_range.end,
                            ::binparse::Value::Bytes(rest),
                        ));
                    }
                    node
                }
                Err(error) => ::binparse::FieldNode::new(
                        #name_str,
                        #type_name,
                        bit_range.clone(),
                        ::binparse::Value::Opaque,
                    )
                    .with_status(::binparse::Status::Error(error)),
            }
        };

        return Ok(());
    }

    field_accum.helper_fns = quote! {
        fn #raw_fn_name(&self) -> ::binparse::ParseResult<(#return_ty, usize)> {
            let start = #start;
            let (value, consumed) = #hook_fn(
                &self.data[start..],
                ::binparse::HookContext {
                    field: #field_path,
                    offset: start,
                    enclosing: self.data,
                },
            )?;
            let end = start.saturating_add(consumed);
            if end > self.data.len() {
                return Err(::binparse::ParseError::NotEnoughData {
                    expected: end,
                    got: self.data.len(),
                });
            }
            Ok((value, consumed))
        }
    };

    field_accum.field_getter = match &struct_accum.condition {
        Some(gate) => quote! {
            #dead_code
            #vis fn #field_name(&self) -> Option<::binparse::ParseResult<#return_ty>> {
                if self.#gate() {
                    Some(self.#raw_fn_name().map(|(value, _)| value))
                } else {
                    None
                }
            }
        },
        None => quote! {
            #dead_code
            #vis fn #field_name(&self) -> ::binparse::ParseResult<#return_ty> {
                self.#raw_fn_name().map(|(value, _)| value)
            }
        },
    };

    field_accum.len = GeneratedLen::Dynamic(quote! {
        match self.#raw_fn_name() {
            Ok((_, consumed)) => binparse::Len { byte: consumed, bit: 0 },
            Err(_) => binparse::Len::ZERO,
        }
    });
    field_accum.field_type = if hook_return_is_numeric(&hook.return_ty.to_string()) {
        DoneFieldType::Hook
    } else {
        DoneFieldType::Other
    };
    field_accum.pre_length_checks = quote! {
        self.#raw_fn_name()?;
    };
    let name_str = field_accum.field_name.to_string();
    let type_name = hook.return_ty.to_string();
    let bind = hook_value_binding(&type_name);
    let value = hook_value(&type_name, quote! { value });
    field_accum.tree_body = quote! {
        match self.#raw_fn_name() {
            Ok((#bind, _)) => ::binparse::FieldNode::new(
                #name_str,
                #type_name,
                bit_range.clone(),
                #value,
            ),
            Err(error) => ::binparse::FieldNode::new(
                    #name_str,
                    #type_name,
                    bit_range.clone(),
                    ::binparse::Value::Opaque,
                )
                .with_status(::binparse::Status::Error(error)),
        }
    };

    Ok(())
}

fn hook_return_is_numeric(type_name: &str) -> bool {
    matches!(
        type_name,
        "u8" | "u16" | "u32" | "u64" | "u128" | "i8" | "i16" | "i32" | "i64" | "i128"
    )
}

fn hook_value(type_name: &str, value: TokenStream) -> TokenStream {
    match type_name {
        "u8" | "u16" | "u32" | "u64" | "u128" => {
            quote! { ::binparse::Value::UInt(u128::from(#value)) }
        }
        "i8" | "i16" | "i32" | "i64" | "i128" => {
            quote! { ::binparse::Value::Int(i128::from(#value)) }
        }
        _ => quote! { ::binparse::Value::Opaque },
    }
}

fn hook_value_binding(type_name: &str) -> TokenStream {
    match type_name {
        "u8" | "u16" | "u32" | "u64" | "u128" | "i8" | "i16" | "i32" | "i64" | "i128" => {
            quote! { value }
        }
        _ => quote! { _ },
    }
}

#[allow(clippy::too_many_arguments)]
fn generate_fixed_hook<'a>(
    hook: &Hook,
    ty: &ast::Type<'a>,
    done: &HashMap<&'a str, GeneratedStruct>,
    struct_accum: &mut StructAccum<'a>,
    field_accum: &mut FieldAccum,
    inherited: attr::Inherited,
    attrs: &ParsedAttrs<'a>,
    errors: &[ast::ErrorVariant<'_>],
) -> Result<(), Error> {
    if attrs.len.is_some() {
        return Err(attr::Error::LenWithFixedHook.into());
    }

    let start_offset = struct_accum.offset.clone();
    let start = match &start_offset {
        GeneratedLen::Fixed(len) => {
            if len.bit != 0 {
                return Err(type_::Error::InvalidAlignment(*len).into());
            }
            let byte = len.byte;
            quote! { #byte }
        }
        GeneratedLen::Dynamic(tokens) => quote! {{
            let len = #tokens;
            if len.bit > 0 { return Err(::binparse::ParseError::UnalignedLength(len)) };
            len.byte
        }},
    };

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
    field_accum.field_type = if hook_return_is_numeric(&hook.return_ty.to_string()) {
        DoneFieldType::Hook
    } else {
        DoneFieldType::Other
    };

    let field_name = field_accum.field_name.clone();
    let hook_fn = &hook.fn_path;
    let return_ty = &hook.return_ty;
    let field_path = format!("{}.{}", struct_accum.name, field_name);
    let field_getter_body = info.field_getter_body;

    let (vis, dead_code) = getter_visibility(attrs);
    let hook_body = quote! {
        let start = #start;
        #hook_fn(
            #field_getter_body,
            ::binparse::HookContext {
                field: #field_path,
                offset: start,
                enclosing: self.data,
            },
        )
    };

    let raw_fn_name;
    field_accum.field_getter = match &struct_accum.condition {
        Some(gate) => {
            let inner_fn_name = format_ident!("{}_raw", field_name);
            raw_fn_name = inner_fn_name.clone();
            quote! {
                #[allow(clippy::identity_op)]
                fn #inner_fn_name(&self) -> ::binparse::ParseResult<#return_ty> {
                    #hook_body
                }

                #dead_code
                #vis fn #field_name(&self) -> Option<::binparse::ParseResult<#return_ty>> {
                    if self.#gate() {
                        Some(self.#inner_fn_name())
                    } else {
                        None
                    }
                }
            }
        }
        None => {
            raw_fn_name = field_name.clone();
            quote! {
                #dead_code
                #[allow(clippy::identity_op)]
                #vis fn #field_name(&self) -> ::binparse::ParseResult<#return_ty> {
                    #hook_body
                }
            }
        }
    };

    field_accum.parse_checks = quote! {
        self.#raw_fn_name()?;
    };

    let name_str = field_accum.field_name.to_string();
    let type_name = hook.return_ty.to_string();
    let bind = hook_value_binding(&type_name);
    let value = hook_value(&type_name, quote! { value });
    field_accum.tree_body = quote! {
        match self.#raw_fn_name() {
            Ok(#bind) => ::binparse::FieldNode::new(
                #name_str,
                #type_name,
                bit_range.clone(),
                #value,
            ),
            Err(error) => ::binparse::FieldNode::new(
                    #name_str,
                    #type_name,
                    bit_range.clone(),
                    ::binparse::Value::Opaque,
                )
                .with_status(::binparse::Status::Error(error)),
        }
    };

    Ok(())
}
