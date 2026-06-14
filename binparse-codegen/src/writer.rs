use std::collections::HashMap;

use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{
    attr::{BitOrder, Endian, Inherited, ParsedAttrs},
    expr::{self, ExprType},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "field '{field}' of struct '{struct_name}' uses @hook but has no @write_hook; writers require an inverse encoder"
    )]
    MissingWriteHook { struct_name: String, field: String },
}

enum FieldKind {
    Primitive {
        primitive: ast::Primitive,
        endian: Endian,
    },
    BitField {
        width: usize,
        bit_order: BitOrder,
    },
    ByteArray {
        len: usize,
    },
    StructRef {
        name: syn::Ident,
        size: usize,
    },
    Constant {
        value: usize,
        primitive: Option<ast::Primitive>,
        width: usize,
        bit_order: BitOrder,
        endian: Endian,
    },
}

struct WriterField {
    name: syn::Ident,
    kind: FieldKind,
    bit_offset: usize,
}

#[derive(Clone, Copy)]
enum LenAdjust {
    None,
    Add(usize),
    Sub(usize),
}

struct DynamicTail {
    array_field: syn::Ident,
    array_offset: usize,
    len_field_str: String,
    len_primitive: ast::Primitive,
    len_offset: usize,
    len_endian: Endian,
    len_adjust: LenAdjust,
}

struct DynamicTailHook {
    array_field: syn::Ident,
    prefix_size: usize,
    encode_fn: TokenStream,
    width_fn: TokenStream,
    return_ty: TokenStream,
}

struct DynamicTailOpen {
    array_field: syn::Ident,
    prefix_size: usize,
}

struct UnionVariantInfo {
    name: syn::Ident,
    reader_variant: syn::Ident,
    fields: Vec<WriterField>,
    disc_value: proc_macro2::Literal,
}

struct UnionLayout {
    field_name: String,
    prefix_fields: Vec<WriterField>,
    disc: WriterField,
    prefix_size: usize,
    variants: Vec<UnionVariantInfo>,
}

struct ForwardLayout {
    prefix_fields: Vec<WriterField>,
    region_field: syn::Ident,
    region_offset: usize,
    len_field_str: String,
    len_primitive: ast::Primitive,
    len_offset: usize,
    len_endian: Endian,
    len_adjust: LenAdjust,
    trailer_fields: Vec<WriterField>,
    trailer_prefix_size: usize,
}

enum Layout {
    Fixed { fields: Vec<WriterField> },
    DynamicTail { fields: Vec<WriterField>, tail: DynamicTail },
    DynamicTailHook { fields: Vec<WriterField>, tail: DynamicTailHook },
    DynamicTailOpen { fields: Vec<WriterField>, tail: DynamicTailOpen },
    Union(UnionLayout),
    Forward(ForwardLayout),
}

pub(crate) fn generate(
    ast: &ast::Struct,
    writer_sizes: &HashMap<&str, usize>,
) -> Result<(TokenStream, Option<usize>), Error> {
    Ok(match classify(ast, writer_sizes)? {
        Some(Layout::Fixed { fields }) => emit_fixed(ast.name, &fields),
        Some(Layout::DynamicTail { fields, tail }) => {
            (emit_dynamic_tail(ast.name, &fields, &tail), None)
        }
        Some(Layout::DynamicTailHook { fields, tail }) => {
            (emit_dynamic_tail_hook(ast.name, &fields, &tail), None)
        }
        Some(Layout::DynamicTailOpen { fields, tail }) => {
            (emit_dynamic_tail_open(ast.name, &fields, &tail), None)
        }
        Some(Layout::Union(layout)) => (emit_union(ast.name, &layout), None),
        Some(Layout::Forward(layout)) => (emit_forward(ast.name, &layout), None),
        None => (TokenStream::new(), None),
    })
}

struct PendingHookLen<'a> {
    field: &'a ast::Field<'a>,
    prefix_size: usize,
}

fn classify(
    ast: &ast::Struct,
    writer_sizes: &HashMap<&str, usize>,
) -> Result<Option<Layout>, Error> {
    let Ok(struct_attrs) = ParsedAttrs::parse(&ast.attributes) else {
        return Ok(None);
    };
    if !struct_attrs_supported(&struct_attrs) {
        return Ok(None);
    }
    let struct_inherited = struct_attrs.merge_inherited(Inherited::default());

    if let Some(layout) = classify_forward(ast, struct_inherited, writer_sizes) {
        return Ok(Some(Layout::Forward(layout)));
    }

    let mut fields = Vec::with_capacity(ast.items.len());
    let mut bit_offset = 0usize;
    let mut pending_hook_len: Option<PendingHookLen> = None;
    let Some(last_index) = ast.items.len().checked_sub(1) else {
        return Ok(None);
    };
    for (index, item) in ast.items.iter().enumerate() {
        let field = match item {
            ast::StructItem::Field(field) => field,
            ast::StructItem::Conditional(_) => return Ok(None),
        };
        let Ok(field_attrs) = ParsedAttrs::parse(&field.attributes) else {
            return Ok(None);
        };
        let kind = match &field.value {
            ast::FieldValue::Type(ast::Type::Primitive(primitive)) => {
                if !field_attrs_supported(&field_attrs) {
                    return Ok(None);
                }
                if !bit_offset.is_multiple_of(8) {
                    return Ok(None);
                }
                let endian = field_attrs.merge_inherited(struct_inherited).endian;
                let (len, _) = crate::match_primitive(primitive);
                let kind = FieldKind::Primitive {
                    primitive: *primitive,
                    endian,
                };
                fields.push(WriterField {
                    name: format_ident!("{}", field.name),
                    kind,
                    bit_offset,
                });
                bit_offset += len.byte * 8;
                continue;
            }
            ast::FieldValue::Type(ast::Type::BitField(width)) => {
                let width = *width as usize;
                if !(1..=7).contains(&width) {
                    return Ok(None);
                }
                if !bitfield_attrs_supported(&field_attrs) {
                    return Ok(None);
                }
                let bit_order = field_attrs.merge_inherited(struct_inherited).bit_order;
                FieldKind::BitField { width, bit_order }
            }
            ast::FieldValue::Type(ast::Type::Array(array)) => {
                if !bit_offset.is_multiple_of(8) {
                    return Ok(None);
                }
                if !matches!(
                    array.elem_ty,
                    ast::ArrayElemType::Primitive(ast::Primitive::U8)
                ) {
                    return Ok(None);
                }
                if array.size.is_none() && field_attrs.hook.is_some() {
                    if pending_hook_len.is_some() {
                        return Ok(None);
                    }
                    pending_hook_len = Some(PendingHookLen {
                        field,
                        prefix_size: bit_offset / 8,
                    });
                    continue;
                }
                if array.size.is_none()
                    && field_attrs.greedy
                    && field_attrs.until.is_none()
                    && field_attrs.hook.is_none()
                {
                    if pending_hook_len.is_some() {
                        return Ok(None);
                    }
                    if index != last_index {
                        return Ok(None);
                    }
                    let tail = DynamicTailOpen {
                        array_field: format_ident!("{}", field.name),
                        prefix_size: bit_offset / 8,
                    };
                    return Ok(Some(Layout::DynamicTailOpen { fields, tail }));
                }
                if !field_attrs_supported(&field_attrs) {
                    return Ok(None);
                }
                let Some(size) = array.size.as_ref() else {
                    return Ok(None);
                };
                if let Some(len) = expr::lower(size, ExprType::Numeric, &[])
                    .ok()
                    .and_then(|lowered| lowered.const_value)
                {
                    fields.push(WriterField {
                        name: format_ident!("{}", field.name),
                        kind: FieldKind::ByteArray { len },
                        bit_offset,
                    });
                    bit_offset += len * 8;
                    continue;
                }
                if index != last_index {
                    return Ok(None);
                }
                if let Some(pending) = &pending_hook_len
                    && size_path_matches(size, pending.field.name)
                {
                    return classify_dynamic_tail_hook(ast.name, field.name, pending, fields);
                }
                let Some(tail) = classify_dynamic_tail(size, field.name, bit_offset, &fields) else {
                    return Ok(None);
                };
                return Ok(Some(Layout::DynamicTail { fields, tail }));
            }
            ast::FieldValue::Type(ast::Type::StructRef(child_name)) => {
                if !field_attrs_supported(&field_attrs) {
                    return Ok(None);
                }
                if !bit_offset.is_multiple_of(8) {
                    return Ok(None);
                }
                let Some(size) = writer_sizes.get(child_name).copied() else {
                    return Ok(None);
                };
                fields.push(WriterField {
                    name: format_ident!("{}", field.name),
                    kind: FieldKind::StructRef {
                        name: format_ident!("{}Writer", child_name),
                        size,
                    },
                    bit_offset,
                });
                bit_offset += size * 8;
                continue;
            }
            ast::FieldValue::Type(ast::Type::Union(union)) => {
                if pending_hook_len.is_some() {
                    return Ok(None);
                }
                if index != last_index {
                    return Ok(None);
                }
                if !field_attrs_supported(&field_attrs) {
                    return Ok(None);
                }
                if !bit_offset.is_multiple_of(8) {
                    return Ok(None);
                }
                return classify_union(
                    union,
                    field.name,
                    fields,
                    bit_offset,
                    struct_inherited,
                    writer_sizes,
                );
            }
            ast::FieldValue::Constraint(ast::Expr::Literal(ast::Literal::Int(lit))) => {
                if !field_attrs_supported(&field_attrs) {
                    return Ok(None);
                }
                let Some((kind, width)) =
                    constant_field_kind(lit, bit_offset, field_attrs.merge_inherited(struct_inherited))
                else {
                    return Ok(None);
                };
                fields.push(WriterField {
                    name: format_ident!("{}", field.name),
                    kind,
                    bit_offset,
                });
                bit_offset += width;
                continue;
            }
            _ => return Ok(None),
        };
        let width = match &kind {
            FieldKind::BitField { width, .. } => *width,
            FieldKind::Primitive { .. }
            | FieldKind::ByteArray { .. }
            | FieldKind::StructRef { .. }
            | FieldKind::Constant { .. } => unreachable!(),
        };
        fields.push(WriterField {
            name: format_ident!("{}", field.name),
            kind,
            bit_offset,
        });
        bit_offset += width;
    }

    if pending_hook_len.is_some() {
        return Ok(None);
    }

    if !bit_offset.is_multiple_of(8) {
        return Ok(None);
    }

    Ok(Some(Layout::Fixed { fields }))
}

fn classify_forward(
    ast: &ast::Struct,
    struct_inherited: Inherited,
    writer_sizes: &HashMap<&str, usize>,
) -> Option<ForwardLayout> {
    let mut prefix_fields = Vec::new();
    let mut bit_offset = 0usize;
    let mut region: Option<(
        syn::Ident,
        String,
        ast::Primitive,
        usize,
        Endian,
        LenAdjust,
        usize,
    )> = None;
    let mut trailer_fields = Vec::new();
    let mut trailer_bit_offset = 0usize;

    for item in &ast.items {
        let ast::StructItem::Field(field) = item else {
            return None;
        };
        let field_attrs = ParsedAttrs::parse(&field.attributes).ok()?;

        if region.is_none() {
            if let ast::FieldValue::Type(ast::Type::Array(array)) = &field.value
                && matches!(
                    array.elem_ty,
                    ast::ArrayElemType::Primitive(ast::Primitive::U8)
                )
                && field_attrs_supported(&field_attrs)
                && bit_offset.is_multiple_of(8)
                && let Some(size) = array.size.as_ref()
                && expr::lower(size, ExprType::Numeric, &[])
                    .ok()
                    .and_then(|lowered| lowered.const_value)
                    .is_none()
                && let Some((len_field_str, len_adjust)) = affine_size_shape(size)
            {
                let len_field = prefix_fields.iter().find(|f: &&WriterField| f.name == len_field_str)?;
                let (len_primitive, len_endian) = match &len_field.kind {
                    FieldKind::Primitive { primitive, endian }
                        if !crate::is_signed(primitive) =>
                    {
                        (*primitive, *endian)
                    }
                    _ => return None,
                };
                region = Some((
                    format_ident!("{}", field.name),
                    len_field_str.to_string(),
                    len_primitive,
                    len_field.bit_offset / 8,
                    len_endian,
                    len_adjust,
                    bit_offset / 8,
                ));
                continue;
            }

            let (writer_field, width) =
                classify_fixed_field(field, bit_offset, struct_inherited, writer_sizes)?;
            prefix_fields.push(writer_field);
            bit_offset += width;
        } else {
            let (writer_field, width) =
                classify_fixed_field(field, trailer_bit_offset, struct_inherited, writer_sizes)?;
            trailer_fields.push(writer_field);
            trailer_bit_offset += width;
        }
    }

    let (
        region_field,
        len_field_str,
        len_primitive,
        len_offset,
        len_endian,
        len_adjust,
        region_offset,
    ) = region?;

    if trailer_fields.is_empty() {
        return None;
    }
    if !bit_offset.is_multiple_of(8) || !trailer_bit_offset.is_multiple_of(8) {
        return None;
    }

    Some(ForwardLayout {
        prefix_fields,
        region_field,
        region_offset,
        len_field_str,
        len_primitive,
        len_offset,
        len_endian,
        len_adjust,
        trailer_fields,
        trailer_prefix_size: trailer_bit_offset / 8,
    })
}

fn classify_fixed_field(
    field: &ast::Field<'_>,
    bit_offset: usize,
    struct_inherited: Inherited,
    writer_sizes: &HashMap<&str, usize>,
) -> Option<(WriterField, usize)> {
    let field_attrs = ParsedAttrs::parse(&field.attributes).ok()?;
    match &field.value {
        ast::FieldValue::Type(ast::Type::Primitive(primitive)) => {
            if !field_attrs_supported(&field_attrs) {
                return None;
            }
            if !bit_offset.is_multiple_of(8) {
                return None;
            }
            let endian = field_attrs.merge_inherited(struct_inherited).endian;
            let (len, _) = crate::match_primitive(primitive);
            let kind = FieldKind::Primitive {
                primitive: *primitive,
                endian,
            };
            Some((
                WriterField {
                    name: format_ident!("{}", field.name),
                    kind,
                    bit_offset,
                },
                len.byte * 8,
            ))
        }
        ast::FieldValue::Type(ast::Type::BitField(width)) => {
            let width = *width as usize;
            if !(1..=7).contains(&width) {
                return None;
            }
            if !bitfield_attrs_supported(&field_attrs) {
                return None;
            }
            let bit_order = field_attrs.merge_inherited(struct_inherited).bit_order;
            Some((
                WriterField {
                    name: format_ident!("{}", field.name),
                    kind: FieldKind::BitField { width, bit_order },
                    bit_offset,
                },
                width,
            ))
        }
        ast::FieldValue::Type(ast::Type::Array(array)) => {
            if !field_attrs_supported(&field_attrs) {
                return None;
            }
            if !bit_offset.is_multiple_of(8) {
                return None;
            }
            if !matches!(
                array.elem_ty,
                ast::ArrayElemType::Primitive(ast::Primitive::U8)
            ) {
                return None;
            }
            let size = array.size.as_ref()?;
            let len = expr::lower(size, ExprType::Numeric, &[])
                .ok()
                .and_then(|lowered| lowered.const_value)?;
            Some((
                WriterField {
                    name: format_ident!("{}", field.name),
                    kind: FieldKind::ByteArray { len },
                    bit_offset,
                },
                len * 8,
            ))
        }
        ast::FieldValue::Type(ast::Type::StructRef(child_name)) => {
            if !field_attrs_supported(&field_attrs) {
                return None;
            }
            if !bit_offset.is_multiple_of(8) {
                return None;
            }
            let size = writer_sizes.get(child_name).copied()?;
            Some((
                WriterField {
                    name: format_ident!("{}", field.name),
                    kind: FieldKind::StructRef {
                        name: format_ident!("{}Writer", child_name),
                        size,
                    },
                    bit_offset,
                },
                size * 8,
            ))
        }
        _ => None,
    }
}

fn classify_fixed_items(
    items: &[ast::StructItem<'_>],
    struct_inherited: Inherited,
    writer_sizes: &HashMap<&str, usize>,
) -> Option<Vec<WriterField>> {
    let mut fields = Vec::with_capacity(items.len());
    let mut bit_offset = 0usize;
    for item in items {
        let ast::StructItem::Field(field) = item else {
            return None;
        };
        let (writer_field, width) =
            classify_fixed_field(field, bit_offset, struct_inherited, writer_sizes)?;
        fields.push(writer_field);
        bit_offset += width;
    }
    if !bit_offset.is_multiple_of(8) {
        return None;
    }
    Some(fields)
}

fn classify_union(
    union: &ast::Union<'_>,
    field_name: &str,
    prefix_fields: Vec<WriterField>,
    bit_offset: usize,
    struct_inherited: Inherited,
    writer_sizes: &HashMap<&str, usize>,
) -> Result<Option<Layout>, Error> {
    let [disc_name] = union.args.as_slice() else {
        return Ok(None);
    };
    let Some(disc) = prefix_fields.iter().find(|f| f.name == disc_name) else {
        return Ok(None);
    };
    let disc = match &disc.kind {
        FieldKind::Primitive { primitive, endian } => WriterField {
            name: disc.name.clone(),
            kind: FieldKind::Primitive {
                primitive: *primitive,
                endian: *endian,
            },
            bit_offset: disc.bit_offset,
        },
        FieldKind::BitField { width, bit_order } => WriterField {
            name: disc.name.clone(),
            kind: FieldKind::BitField {
                width: *width,
                bit_order: *bit_order,
            },
            bit_offset: disc.bit_offset,
        },
        FieldKind::ByteArray { .. } | FieldKind::StructRef { .. } | FieldKind::Constant { .. } => {
            return Ok(None);
        }
    };

    let mut variants = Vec::with_capacity(union.variants.len());
    for variant in &union.variants {
        let [matcher] = variant.matchers.as_slice() else {
            return Ok(None);
        };
        let disc_value = match matcher {
            ast::UnionMatcher::Literal(ast::Literal::Int(int_lit)) => {
                proc_macro2::Literal::u128_unsuffixed(int_lit.value as u128)
            }
            ast::UnionMatcher::Literal(_) | ast::UnionMatcher::Tuple(_) => return Ok(None),
            ast::UnionMatcher::Wildcard => continue,
        };
        let inline = match &variant.body {
            ast::UnionBody::NamedInline(inline) => inline,
            ast::UnionBody::Error(..) => return Ok(None),
        };
        let variant_inherited = match ParsedAttrs::parse(&inline.attributes) {
            Ok(attrs) if struct_attrs_supported(&attrs) => attrs.merge_inherited(struct_inherited),
            _ => return Ok(None),
        };
        let Some(fields) = classify_fixed_items(&inline.items, variant_inherited, writer_sizes)
        else {
            return Ok(None);
        };
        variants.push(UnionVariantInfo {
            name: format_ident!("{}", inline.name),
            reader_variant: format_ident!("{}", inline.name),
            fields,
            disc_value,
        });
    }

    if variants.is_empty() {
        return Ok(None);
    }

    Ok(Some(Layout::Union(UnionLayout {
        field_name: field_name.to_string(),
        prefix_fields,
        disc,
        prefix_size: bit_offset / 8,
        variants,
    })))
}

fn classify_dynamic_tail(
    size: &ast::Expr<'_>,
    array_name: &str,
    array_bit_offset: usize,
    fields: &[WriterField],
) -> Option<DynamicTail> {
    let (len_field_str, len_adjust) = affine_size_shape(size)?;
    let len_field = fields.iter().find(|f| f.name == len_field_str)?;
    let (len_primitive, len_endian) = match &len_field.kind {
        FieldKind::Primitive { primitive, endian } if !crate::is_signed(primitive) => {
            (*primitive, *endian)
        }
        _ => return None,
    };
    Some(DynamicTail {
        array_field: format_ident!("{}", array_name),
        array_offset: array_bit_offset / 8,
        len_field_str: len_field_str.to_string(),
        len_primitive,
        len_offset: len_field.bit_offset / 8,
        len_endian,
        len_adjust,
    })
}

fn affine_size_shape<'a>(size: &'a ast::Expr<'a>) -> Option<(&'a str, LenAdjust)> {
    match size {
        ast::Expr::Path(path) => {
            let [len_field_str] = path.as_slice() else {
                return None;
            };
            Some((len_field_str, LenAdjust::None))
        }
        ast::Expr::Binary(binary) => {
            let op = match binary.op {
                ast::BinaryOp::Numeric(ast::NumericBinaryOp::Add) => LenAdjust::Sub,
                ast::BinaryOp::Numeric(ast::NumericBinaryOp::Sub) => LenAdjust::Add,
                _ => return None,
            };
            let ast::Expr::Path(path) = &binary.lhs else {
                return None;
            };
            let [len_field_str] = path.as_slice() else {
                return None;
            };
            let ast::Expr::Literal(ast::Literal::Int(int_lit)) = &binary.rhs else {
                return None;
            };
            if int_lit.value == 0 {
                return Some((len_field_str, LenAdjust::None));
            }
            Some((len_field_str, op(int_lit.value)))
        }
        _ => None,
    }
}

fn len_value_expr(region_len: &TokenStream, adjust: LenAdjust) -> TokenStream {
    match adjust {
        LenAdjust::None => quote! { #region_len },
        LenAdjust::Add(k) => quote! { (#region_len).saturating_add(#k) },
        LenAdjust::Sub(k) => quote! { (#region_len).saturating_sub(#k) },
    }
}

fn size_path_matches(size: &ast::Expr<'_>, name: &str) -> bool {
    matches!(size, ast::Expr::Path(path) if path.as_slice() == [name])
}

fn constant_field_kind(
    lit: &ast::IntLiteral,
    bit_offset: usize,
    inherited: Inherited,
) -> Option<(FieldKind, usize)> {
    let primitive = match lit.ty {
        ast::IntType::Binary => match lit.width as usize {
            width @ 1..=7 => {
                let kind = FieldKind::Constant {
                    value: lit.value,
                    primitive: None,
                    width,
                    bit_order: inherited.bit_order,
                    endian: inherited.endian,
                };
                return Some((kind, width));
            }
            8 => ast::Primitive::U8,
            _ => return None,
        },
        ast::IntType::Hex => match usize::from(lit.width).div_ceil(2) {
            1 => ast::Primitive::U8,
            2 => ast::Primitive::U16,
            3..=4 => ast::Primitive::U32,
            5..=8 => ast::Primitive::U64,
            9..=16 => ast::Primitive::U128,
            _ => return None,
        },
        ast::IntType::Decimal => {
            if lit.value <= usize::from(u8::MAX) {
                ast::Primitive::U8
            } else if lit.value <= usize::from(u16::MAX) {
                ast::Primitive::U16
            } else if u32::try_from(lit.value).is_ok() {
                ast::Primitive::U32
            } else {
                ast::Primitive::U64
            }
        }
    };
    if !bit_offset.is_multiple_of(8) {
        return None;
    }
    let (len, _) = crate::match_primitive(&primitive);
    let kind = FieldKind::Constant {
        value: lit.value,
        primitive: Some(primitive),
        width: len.byte * 8,
        bit_order: inherited.bit_order,
        endian: inherited.endian,
    };
    Some((kind, len.byte * 8))
}

fn classify_dynamic_tail_hook(
    struct_name: &str,
    array_name: &str,
    pending: &PendingHookLen<'_>,
    fields: Vec<WriterField>,
) -> Result<Option<Layout>, Error> {
    let field_attrs = match ParsedAttrs::parse(&pending.field.attributes) {
        Ok(attrs) => attrs,
        Err(_) => return Ok(None),
    };
    let Some(hook) = field_attrs.hook else {
        return Ok(None);
    };
    let Some((encode_fn, width_fn)) = parse_write_hook(&pending.field.attributes) else {
        return Err(Error::MissingWriteHook {
            struct_name: struct_name.to_string(),
            field: pending.field.name.to_string(),
        });
    };
    let tail = DynamicTailHook {
        array_field: format_ident!("{}", array_name),
        prefix_size: pending.prefix_size,
        encode_fn,
        width_fn,
        return_ty: hook.return_ty,
    };
    Ok(Some(Layout::DynamicTailHook { fields, tail }))
}

fn parse_write_hook(attrs: &[ast::Attribute<'_>]) -> Option<(TokenStream, TokenStream)> {
    let attr = attrs.iter().find(|attr| attr.name == "write_hook")?;
    let [encode, width] = attr.args.as_slice() else {
        return None;
    };
    Some((path_to_tokens(encode)?, path_to_tokens(width)?))
}

fn path_to_tokens(expr: &ast::Expr<'_>) -> Option<TokenStream> {
    match expr {
        ast::Expr::Path(segments) => {
            let idents: Vec<_> = segments.iter().map(|s| format_ident!("{}", s)).collect();
            Some(quote! { #(#idents)::* })
        }
        _ => None,
    }
}

fn struct_attrs_supported(attrs: &ParsedAttrs<'_>) -> bool {
    let ParsedAttrs {
        endian: _,
        bit_order: _,
        hook,
        check: _,
        range: _,
        until,
        greedy,
        max_iter,
        skip,
        pad,
        pad_to,
        align,
        len,
        discriminator: _,
        payload: _,
        cache_len: _,
        cache_value: _,
    } = attrs;
    hook.is_none()
        && until.is_none()
        && !greedy
        && max_iter.is_none()
        && !skip
        && pad.is_none()
        && pad_to.is_none()
        && align.is_none()
        && len.is_none()
}

fn field_attrs_supported(attrs: &ParsedAttrs<'_>) -> bool {
    attrs.bit_order.is_none() && struct_attrs_supported(attrs)
}

fn bitfield_attrs_supported(attrs: &ParsedAttrs<'_>) -> bool {
    struct_attrs_supported(attrs)
}

fn emit_fixed(name: &str, fields: &[WriterField]) -> (TokenStream, Option<usize>) {
    let writer_name = format_ident!("{}Writer", name);
    let content_name = format_ident!("{}Content", name);

    let total_bits = fields.iter().map(field_bit_width).sum::<usize>();
    let size = total_bits / 8;

    let setters = fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(|field| match &field.kind {
            FieldKind::ByteArray { len } => {
                let accessor_name = format_ident!("{}_mut", field.name);
                let offset = field.bit_offset / 8;
                let end = offset + len;
                quote! {
                    pub fn #accessor_name(&mut self) -> &mut [u8] {
                        &mut self.data[#offset..#end]
                    }
                }
            }
            FieldKind::StructRef { name: child_writer, size } => {
                let accessor_name = format_ident!("{}_mut", field.name);
                let offset = field.bit_offset / 8;
                let end = offset + size;
                quote! {
                    pub fn #accessor_name(&mut self) -> #child_writer<'_> {
                        #child_writer { data: &mut self.data[#offset..#end] }
                    }
                }
            }
            FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                let setter_name = format_ident!("set_{}", field.name);
                let ty = field_type(field);
                let body = setter_body(field);
                quote! {
                    pub fn #setter_name(&mut self, value: #ty) -> &mut Self {
                        #body
                        self
                    }
                }
            }
            FieldKind::Constant { .. } => unreachable!(),
        });

    let write_calls = fields.iter().map(|field| {
        let field_name = &field.name;
        match &field.kind {
            FieldKind::ByteArray { .. } => {
                let accessor_name = format_ident!("{}_mut", field.name);
                quote! { w.#accessor_name().copy_from_slice(&content.#field_name); }
            }
            FieldKind::StructRef { name: child_writer, size } => {
                let offset = field.bit_offset / 8;
                let end = offset + size;
                quote! { #child_writer::write_into(&mut w.data[#offset..#end], &content.#field_name)?; }
            }
            FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                let setter_name = format_ident!("set_{}", field.name);
                quote! { w.#setter_name(content.#field_name); }
            }
            FieldKind::Constant { .. } => constant_write_call(field, &quote! { w.data }),
        }
    });

    let content_fields = fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(|field| {
            let field_name = &field.name;
            let ty = field_type(field);
            quote! { pub #field_name: #ty }
        });

    let tokens = quote! {
        pub struct #writer_name<'a> {
            data: &'a mut [u8],
        }

        impl<'a> #writer_name<'a> {
            pub const SIZE: usize = #size;

            pub fn new(data: &'a mut [u8]) -> ::binparse::WriteResult<Self> {
                if data.len() < Self::SIZE {
                    return Err(::binparse::WriteError::NotEnoughSpace {
                        expected: Self::SIZE,
                        got: data.len(),
                    });
                }
                Ok(Self { data })
            }

            #(#setters)*

            pub fn write_into(data: &'a mut [u8], content: &#content_name) -> ::binparse::WriteResult<usize> {
                let mut w = Self::new(data)?;
                #(#write_calls)*
                Ok(Self::SIZE)
            }

            pub fn to_vec(content: &#content_name) -> ::std::vec::Vec<u8> {
                let mut buf = ::std::vec![0u8; Self::SIZE];
                let _ = #writer_name::write_into(&mut buf, content);
                buf
            }
        }

        pub struct #content_name {
            #(#content_fields),*
        }
    };
    (tokens, Some(size))
}

fn emit_dynamic_tail(name: &str, fields: &[WriterField], tail: &DynamicTail) -> TokenStream {
    let writer_name = format_ident!("{}Writer", name);
    let content_name = format_ident!("{}Content", name);
    let lens_name = format_ident!("{}Lens", name);

    let array_field = &tail.array_field;
    let array_mut = format_ident!("{}_mut", tail.array_field);
    let array_offset = tail.array_offset;
    let prefix_size = array_offset;

    let len_ty = crate::match_primitive(&tail.len_primitive).1;
    let len_field_qualified = format!("{}.{}", name, tail.len_field_str);
    let len_value = len_value_expr(&quote! { self.lens.#array_field }, tail.len_adjust);
    let len_value_param = len_value_expr(&quote! { lens.#array_field }, tail.len_adjust);

    let write_len = {
        let len_offset = tail.len_offset;
        let single_byte = crate::single_byte_read(&tail.len_primitive);
        if single_byte.is_some() {
            quote! {
                fn write_len(&mut self) {
                    self.data[#len_offset] = #len_value as #len_ty;
                }
            }
        } else {
            let (len, _) = crate::match_primitive(&tail.len_primitive);
            let end = len_offset + len.byte;
            let to_bytes = match tail.len_endian {
                Endian::Big => quote! { to_be_bytes },
                Endian::Little => quote! { to_le_bytes },
            };
            quote! {
                fn write_len(&mut self) {
                    self.data[#len_offset..#end]
                        .copy_from_slice(&(#len_value as #len_ty).#to_bytes());
                }
            }
        }
    };

    let setters = fields
        .iter()
        .filter(|field| {
            field.name != tail.len_field_str && !matches!(field.kind, FieldKind::Constant { .. })
        })
        .map(|field| match &field.kind {
            FieldKind::ByteArray { len } => {
                let accessor_name = format_ident!("{}_mut", field.name);
                let offset = field.bit_offset / 8;
                let end = offset + len;
                quote! {
                    pub fn #accessor_name(&mut self) -> &mut [u8] {
                        &mut self.data[#offset..#end]
                    }
                }
            }
            FieldKind::StructRef { name: child_writer, size } => {
                let accessor_name = format_ident!("{}_mut", field.name);
                let offset = field.bit_offset / 8;
                let end = offset + size;
                quote! {
                    pub fn #accessor_name(&mut self) -> #child_writer<'_> {
                        #child_writer { data: &mut self.data[#offset..#end] }
                    }
                }
            }
            FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                let setter_name = format_ident!("set_{}", field.name);
                let ty = field_type(field);
                let body = setter_body(field);
                quote! {
                    pub fn #setter_name(&mut self, value: #ty) -> &mut Self {
                        #body
                        self
                    }
                }
            }
            FieldKind::Constant { .. } => unreachable!(),
        });

    let write_calls = fields
        .iter()
        .filter(|field| field.name != tail.len_field_str)
        .map(|field| {
            let field_name = &field.name;
            match &field.kind {
                FieldKind::ByteArray { .. } => {
                    let accessor_name = format_ident!("{}_mut", field.name);
                    quote! { w.#accessor_name().copy_from_slice(&content.#field_name); }
                }
                FieldKind::StructRef { name: child_writer, size } => {
                    let offset = field.bit_offset / 8;
                    let end = offset + size;
                    quote! { #child_writer::write_into(&mut w.data[#offset..#end], &content.#field_name)?; }
                }
                FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                    let setter_name = format_ident!("set_{}", field.name);
                    quote! { w.#setter_name(content.#field_name); }
                }
                FieldKind::Constant { .. } => constant_write_call(field, &quote! { w.data }),
            }
        });

    let content_fields = fields
        .iter()
        .filter(|field| {
            field.name != tail.len_field_str && !matches!(field.kind, FieldKind::Constant { .. })
        })
        .map(|field| {
            let field_name = &field.name;
            let ty = field_type(field);
            quote! { pub #field_name: #ty }
        });

    quote! {
        #[derive(Clone, Copy)]
        pub struct #lens_name {
            pub #array_field: usize,
        }

        pub struct #writer_name<'a> {
            data: &'a mut [u8],
            lens: #lens_name,
        }

        impl<'a> #writer_name<'a> {
            pub fn encoded_len(lens: &#lens_name) -> usize {
                #prefix_size + lens.#array_field
            }

            pub fn new(data: &'a mut [u8], lens: #lens_name) -> ::binparse::WriteResult<Self> {
                let need = Self::encoded_len(&lens);
                if data.len() < need {
                    return Err(::binparse::WriteError::NotEnoughSpace {
                        expected: need,
                        got: data.len(),
                    });
                }
                if #len_value_param > (#len_ty::MAX as usize) {
                    return Err(::binparse::WriteError::ValueTooLarge {
                        field: #len_field_qualified,
                        value: #len_value_param,
                        max: #len_ty::MAX as usize,
                    });
                }
                let mut me = Self { data, lens };
                me.write_len();
                Ok(me)
            }

            #write_len

            #(#setters)*

            pub fn #array_mut(&mut self) -> &mut [u8] {
                let off = #array_offset;
                &mut self.data[off..off + self.lens.#array_field]
            }

            pub fn write_into(data: &'a mut [u8], content: &#content_name) -> ::binparse::WriteResult<usize> {
                let lens = #lens_name { #array_field: content.#array_field.len() };
                let need = Self::encoded_len(&lens);
                let mut w = Self::new(data, lens)?;
                #(#write_calls)*
                w.#array_mut().copy_from_slice(content.#array_field);
                Ok(need)
            }

            pub fn to_vec(content: &#content_name) -> ::std::vec::Vec<u8> {
                let lens = #lens_name { #array_field: content.#array_field.len() };
                let mut buf = ::std::vec![0u8; Self::encoded_len(&lens)];
                let _ = #writer_name::write_into(&mut buf, content);
                buf
            }
        }

        pub struct #content_name<'a> {
            #(#content_fields,)*
            pub #array_field: &'a [u8],
        }
    }
}

fn emit_dynamic_tail_open(
    name: &str,
    fields: &[WriterField],
    tail: &DynamicTailOpen,
) -> TokenStream {
    let writer_name = format_ident!("{}Writer", name);
    let content_name = format_ident!("{}Content", name);
    let lens_name = format_ident!("{}Lens", name);

    let array_field = &tail.array_field;
    let array_mut = format_ident!("{}_mut", tail.array_field);
    let prefix_size = tail.prefix_size;

    let setters = fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(|field| match &field.kind {
            FieldKind::ByteArray { len } => {
                let accessor_name = format_ident!("{}_mut", field.name);
                let offset = field.bit_offset / 8;
                let end = offset + len;
                quote! {
                    pub fn #accessor_name(&mut self) -> &mut [u8] {
                        &mut self.data[#offset..#end]
                    }
                }
            }
            FieldKind::StructRef { name: child_writer, size } => {
                let accessor_name = format_ident!("{}_mut", field.name);
                let offset = field.bit_offset / 8;
                let end = offset + size;
                quote! {
                    pub fn #accessor_name(&mut self) -> #child_writer<'_> {
                        #child_writer { data: &mut self.data[#offset..#end] }
                    }
                }
            }
            FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                let setter_name = format_ident!("set_{}", field.name);
                let ty = field_type(field);
                let body = setter_body(field);
                quote! {
                    pub fn #setter_name(&mut self, value: #ty) -> &mut Self {
                        #body
                        self
                    }
                }
            }
            FieldKind::Constant { .. } => unreachable!(),
        });

    let write_calls = fields.iter().map(|field| {
        let field_name = &field.name;
        match &field.kind {
            FieldKind::ByteArray { .. } => {
                let accessor_name = format_ident!("{}_mut", field.name);
                quote! { w.#accessor_name().copy_from_slice(&content.#field_name); }
            }
            FieldKind::StructRef { name: child_writer, size } => {
                let offset = field.bit_offset / 8;
                let end = offset + size;
                quote! { #child_writer::write_into(&mut w.data[#offset..#end], &content.#field_name)?; }
            }
            FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                let setter_name = format_ident!("set_{}", field.name);
                quote! { w.#setter_name(content.#field_name); }
            }
            FieldKind::Constant { .. } => constant_write_call(field, &quote! { w.data }),
        }
    });

    let content_fields = fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(|field| {
            let field_name = &field.name;
            let ty = field_type(field);
            quote! { pub #field_name: #ty }
        });

    quote! {
        #[derive(Clone, Copy)]
        pub struct #lens_name {
            pub #array_field: usize,
        }

        pub struct #writer_name<'a> {
            data: &'a mut [u8],
            lens: #lens_name,
        }

        impl<'a> #writer_name<'a> {
            pub fn encoded_len(lens: &#lens_name) -> usize {
                #prefix_size + lens.#array_field
            }

            pub fn new(data: &'a mut [u8], lens: #lens_name) -> ::binparse::WriteResult<Self> {
                let need = Self::encoded_len(&lens);
                if data.len() < need {
                    return Err(::binparse::WriteError::NotEnoughSpace {
                        expected: need,
                        got: data.len(),
                    });
                }
                Ok(Self { data, lens })
            }

            #(#setters)*

            pub fn #array_mut(&mut self) -> &mut [u8] {
                let off = #prefix_size;
                &mut self.data[off..off + self.lens.#array_field]
            }

            pub fn write_into(data: &'a mut [u8], content: &#content_name) -> ::binparse::WriteResult<usize> {
                let lens = #lens_name { #array_field: content.#array_field.len() };
                let need = Self::encoded_len(&lens);
                let mut w = Self::new(data, lens)?;
                #(#write_calls)*
                w.#array_mut().copy_from_slice(content.#array_field);
                Ok(need)
            }

            pub fn to_vec(content: &#content_name) -> ::std::vec::Vec<u8> {
                let lens = #lens_name { #array_field: content.#array_field.len() };
                let mut buf = ::std::vec![0u8; Self::encoded_len(&lens)];
                let _ = #writer_name::write_into(&mut buf, content);
                buf
            }
        }

        pub struct #content_name<'a> {
            #(#content_fields,)*
            pub #array_field: &'a [u8],
        }
    }
}

fn emit_dynamic_tail_hook(
    name: &str,
    fields: &[WriterField],
    tail: &DynamicTailHook,
) -> TokenStream {
    let writer_name = format_ident!("{}Writer", name);
    let content_name = format_ident!("{}Content", name);
    let lens_name = format_ident!("{}Lens", name);

    let array_field = &tail.array_field;
    let array_mut = format_ident!("{}_mut", tail.array_field);
    let prefix_size = tail.prefix_size;
    let encode_fn = &tail.encode_fn;
    let width_fn = &tail.width_fn;
    let return_ty = &tail.return_ty;

    let setters = fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(|field| match &field.kind {
            FieldKind::ByteArray { len } => {
                let accessor_name = format_ident!("{}_mut", field.name);
                let offset = field.bit_offset / 8;
                let end = offset + len;
                quote! {
                    pub fn #accessor_name(&mut self) -> &mut [u8] {
                        &mut self.data[#offset..#end]
                    }
                }
            }
            FieldKind::StructRef { name: child_writer, size } => {
                let accessor_name = format_ident!("{}_mut", field.name);
                let offset = field.bit_offset / 8;
                let end = offset + size;
                quote! {
                    pub fn #accessor_name(&mut self) -> #child_writer<'_> {
                        #child_writer { data: &mut self.data[#offset..#end] }
                    }
                }
            }
            FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                let setter_name = format_ident!("set_{}", field.name);
                let ty = field_type(field);
                let body = setter_body(field);
                quote! {
                    pub fn #setter_name(&mut self, value: #ty) -> &mut Self {
                        #body
                        self
                    }
                }
            }
            FieldKind::Constant { .. } => unreachable!(),
        });

    let write_calls = fields.iter().map(|field| {
        let field_name = &field.name;
        match &field.kind {
            FieldKind::ByteArray { .. } => {
                let accessor_name = format_ident!("{}_mut", field.name);
                quote! { w.#accessor_name().copy_from_slice(&content.#field_name); }
            }
            FieldKind::StructRef { name: child_writer, size } => {
                let offset = field.bit_offset / 8;
                let end = offset + size;
                quote! { #child_writer::write_into(&mut w.data[#offset..#end], &content.#field_name)?; }
            }
            FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                let setter_name = format_ident!("set_{}", field.name);
                quote! { w.#setter_name(content.#field_name); }
            }
            FieldKind::Constant { .. } => constant_write_call(field, &quote! { w.data }),
        }
    });

    let content_fields = fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(|field| {
            let field_name = &field.name;
            let ty = field_type(field);
            quote! { pub #field_name: #ty }
        });

    quote! {
        #[derive(Clone, Copy)]
        pub struct #lens_name {
            pub #array_field: usize,
        }

        pub struct #writer_name<'a> {
            data: &'a mut [u8],
            lens: #lens_name,
            len_width: usize,
        }

        impl<'a> #writer_name<'a> {
            pub fn encoded_len(lens: &#lens_name) -> usize {
                #prefix_size + #width_fn(lens.#array_field as #return_ty) + lens.#array_field
            }

            pub fn new(data: &'a mut [u8], lens: #lens_name) -> ::binparse::WriteResult<Self> {
                let need = Self::encoded_len(&lens);
                if data.len() < need {
                    return Err(::binparse::WriteError::NotEnoughSpace {
                        expected: need,
                        got: data.len(),
                    });
                }
                let len_width = #encode_fn(lens.#array_field as #return_ty, &mut data[#prefix_size..])?;
                Ok(Self { data, lens, len_width })
            }

            #(#setters)*

            pub fn #array_mut(&mut self) -> &mut [u8] {
                let off = #prefix_size + self.len_width;
                &mut self.data[off..off + self.lens.#array_field]
            }

            pub fn write_into(data: &'a mut [u8], content: &#content_name) -> ::binparse::WriteResult<usize> {
                let lens = #lens_name { #array_field: content.#array_field.len() };
                let need = Self::encoded_len(&lens);
                let mut w = Self::new(data, lens)?;
                #(#write_calls)*
                w.#array_mut().copy_from_slice(content.#array_field);
                Ok(need)
            }

            pub fn to_vec(content: &#content_name) -> ::std::vec::Vec<u8> {
                let lens = #lens_name { #array_field: content.#array_field.len() };
                let mut buf = ::std::vec![0u8; Self::encoded_len(&lens)];
                let _ = #writer_name::write_into(&mut buf, content);
                buf
            }
        }

        pub struct #content_name<'a> {
            #(#content_fields,)*
            pub #array_field: &'a [u8],
        }
    }
}

fn emit_union(name: &str, layout: &UnionLayout) -> TokenStream {
    let writer_name = format_ident!("{}Writer", name);
    let content_name = format_ident!("{}Content", name);
    let field_pascal = to_pascal_case(&layout.field_name);
    let body_enum = format_ident!("{}{}Content", name, field_pascal);
    let body_field = format_ident!("{}", layout.field_name);

    let prefix_size = layout.prefix_size;
    let disc_name = &layout.disc.name;

    let mut variant_entities = TokenStream::new();
    for variant in &layout.variants {
        let (tokens, _) = emit_fixed(&variant.name.to_string(), &variant.fields);
        variant_entities.extend(tokens);
    }

    let enum_variants = layout.variants.iter().map(|variant| {
        let reader_variant = &variant.reader_variant;
        let variant_content = format_ident!("{}Content", variant.name);
        quote! { #reader_variant(#variant_content) }
    });

    let prefix_setters = layout
        .prefix_fields
        .iter()
        .filter(|field| {
            field.name != *disc_name && !matches!(field.kind, FieldKind::Constant { .. })
        })
        .map(|field| match &field.kind {
            FieldKind::ByteArray { len } => {
                let accessor_name = format_ident!("{}_mut", field.name);
                let offset = field.bit_offset / 8;
                let end = offset + len;
                quote! {
                    fn #accessor_name(&mut self) -> &mut [u8] {
                        &mut self.data[#offset..#end]
                    }
                }
            }
            FieldKind::StructRef { name: child_writer, size } => {
                let accessor_name = format_ident!("{}_mut", field.name);
                let offset = field.bit_offset / 8;
                let end = offset + size;
                quote! {
                    fn #accessor_name(&mut self) -> #child_writer<'_> {
                        #child_writer { data: &mut self.data[#offset..#end] }
                    }
                }
            }
            FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                let setter_name = format_ident!("set_{}", field.name);
                let ty = field_type(field);
                let body = setter_body(field);
                quote! {
                    fn #setter_name(&mut self, value: #ty) -> &mut Self {
                        #body
                        self
                    }
                }
            }
            FieldKind::Constant { .. } => unreachable!(),
        });

    let prefix_write_calls = layout
        .prefix_fields
        .iter()
        .filter(|field| field.name != *disc_name)
        .map(|field| {
            let field_name = &field.name;
            match &field.kind {
                FieldKind::ByteArray { .. } => {
                    let accessor_name = format_ident!("{}_mut", field.name);
                    quote! { w.#accessor_name().copy_from_slice(&content.#field_name); }
                }
                FieldKind::StructRef { name: child_writer, size } => {
                    let offset = field.bit_offset / 8;
                    let end = offset + size;
                    quote! { #child_writer::write_into(&mut w.data[#offset..#end], &content.#field_name)?; }
                }
                FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                    let setter_name = format_ident!("set_{}", field.name);
                    quote! { w.#setter_name(content.#field_name); }
                }
                FieldKind::Constant { .. } => constant_write_call(field, &quote! { w.data }),
            }
        });

    let content_fields = layout
        .prefix_fields
        .iter()
        .filter(|field| {
            field.name != *disc_name && !matches!(field.kind, FieldKind::Constant { .. })
        })
        .map(|field| {
            let field_name = &field.name;
            let ty = field_type(field);
            quote! { pub #field_name: #ty }
        });

    let encoded_len_arms = layout.variants.iter().map(|variant| {
        let reader_variant = &variant.reader_variant;
        let variant_writer = format_ident!("{}Writer", variant.name);
        quote! { #body_enum::#reader_variant(_) => #variant_writer::SIZE }
    });

    let disc_ty = field_type(&layout.disc);
    let disc_target = quote! { w.data };
    let disc_body = setter_body_into(&layout.disc, &disc_target);

    let write_arms = layout.variants.iter().map(|variant| {
        let reader_variant = &variant.reader_variant;
        let variant_writer = format_ident!("{}Writer", variant.name);
        let disc_value = &variant.disc_value;
        quote! {
            #body_enum::#reader_variant(c) => {
                let value: #disc_ty = #disc_value;
                #disc_body
                let end = #prefix_size + #variant_writer::SIZE;
                #variant_writer::write_into(&mut w.data[#prefix_size..end], c)?;
            }
        }
    });

    quote! {
        #variant_entities

        pub enum #body_enum {
            #(#enum_variants),*
        }

        pub struct #writer_name<'a> {
            data: &'a mut [u8],
        }

        impl<'a> #writer_name<'a> {
            pub fn encoded_len(content: &#content_name) -> usize {
                #prefix_size + match &content.#body_field {
                    #(#encoded_len_arms),*
                }
            }

            #(#prefix_setters)*

            pub fn write_into(data: &'a mut [u8], content: &#content_name) -> ::binparse::WriteResult<usize> {
                let need = Self::encoded_len(content);
                if data.len() < need {
                    return Err(::binparse::WriteError::NotEnoughSpace {
                        expected: need,
                        got: data.len(),
                    });
                }
                let mut w = Self { data };
                #(#prefix_write_calls)*
                match &content.#body_field {
                    #(#write_arms)*
                }
                Ok(need)
            }

            pub fn to_vec(content: &#content_name) -> ::std::vec::Vec<u8> {
                let mut buf = ::std::vec![0u8; Self::encoded_len(content)];
                let _ = #writer_name::write_into(&mut buf, content);
                buf
            }
        }

        pub struct #content_name {
            #(#content_fields,)*
            pub #body_field: #body_enum,
        }
    }
}

fn emit_forward(name: &str, layout: &ForwardLayout) -> TokenStream {
    let writer_name = format_ident!("{}Writer", name);
    let content_name = format_ident!("{}Content", name);
    let lens_name = format_ident!("{}Lens", name);

    let region_field = &layout.region_field;
    let region_mut = format_ident!("{}_mut", layout.region_field);
    let region_offset = layout.region_offset;
    let trailer_prefix_size = layout.trailer_prefix_size;

    let len_ty = crate::match_primitive(&layout.len_primitive).1;
    let len_field_qualified = format!("{}.{}", name, layout.len_field_str);
    let len_value = len_value_expr(&quote! { self.lens.#region_field }, layout.len_adjust);
    let len_value_param = len_value_expr(&quote! { lens.#region_field }, layout.len_adjust);

    let write_len = {
        let len_offset = layout.len_offset;
        let single_byte = crate::single_byte_read(&layout.len_primitive);
        if single_byte.is_some() {
            quote! {
                fn write_len(&mut self) {
                    self.data[#len_offset] = #len_value as #len_ty;
                }
            }
        } else {
            let (len, _) = crate::match_primitive(&layout.len_primitive);
            let end = len_offset + len.byte;
            let to_bytes = match layout.len_endian {
                Endian::Big => quote! { to_be_bytes },
                Endian::Little => quote! { to_le_bytes },
            };
            quote! {
                fn write_len(&mut self) {
                    self.data[#len_offset..#end]
                        .copy_from_slice(&(#len_value as #len_ty).#to_bytes());
                }
            }
        }
    };

    let prefix_setters = layout
        .prefix_fields
        .iter()
        .filter(|field| {
            field.name != layout.len_field_str
                && !matches!(field.kind, FieldKind::Constant { .. })
        })
        .map(|field| match &field.kind {
            FieldKind::ByteArray { len } => {
                let accessor_name = format_ident!("{}_mut", field.name);
                let offset = field.bit_offset / 8;
                let end = offset + len;
                quote! {
                    pub fn #accessor_name(&mut self) -> &mut [u8] {
                        &mut self.data[#offset..#end]
                    }
                }
            }
            FieldKind::StructRef { name: child_writer, size } => {
                let accessor_name = format_ident!("{}_mut", field.name);
                let offset = field.bit_offset / 8;
                let end = offset + size;
                quote! {
                    pub fn #accessor_name(&mut self) -> #child_writer<'_> {
                        #child_writer { data: &mut self.data[#offset..#end] }
                    }
                }
            }
            FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                let setter_name = format_ident!("set_{}", field.name);
                let ty = field_type(field);
                let body = setter_body(field);
                quote! {
                    pub fn #setter_name(&mut self, value: #ty) -> &mut Self {
                        #body
                        self
                    }
                }
            }
            FieldKind::Constant { .. } => unreachable!(),
        });

    let trailer_setters = layout
        .trailer_fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(|field| {
            let base = quote! { let base = #region_offset + self.lens.#region_field; };
            match &field.kind {
                FieldKind::ByteArray { len } => {
                    let accessor_name = format_ident!("{}_mut", field.name);
                    let offset = field.bit_offset / 8;
                    let end = offset + len;
                    quote! {
                        pub fn #accessor_name(&mut self) -> &mut [u8] {
                            #base
                            &mut self.data[base + #offset..base + #end]
                        }
                    }
                }
                FieldKind::StructRef { name: child_writer, size } => {
                    let accessor_name = format_ident!("{}_mut", field.name);
                    let offset = field.bit_offset / 8;
                    let end = offset + size;
                    quote! {
                        pub fn #accessor_name(&mut self) -> #child_writer<'_> {
                            #base
                            #child_writer { data: &mut self.data[base + #offset..base + #end] }
                        }
                    }
                }
                FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                    let setter_name = format_ident!("set_{}", field.name);
                    let ty = field_type(field);
                    let body = setter_body_into(field, &quote! { trailer });
                    quote! {
                        pub fn #setter_name(&mut self, value: #ty) -> &mut Self {
                            #base
                            let trailer = &mut self.data[base..];
                            #body
                            self
                        }
                    }
                }
                FieldKind::Constant { .. } => unreachable!(),
            }
        });

    let prefix_write_calls = layout
        .prefix_fields
        .iter()
        .filter(|field| field.name != layout.len_field_str)
        .map(|field| {
            let field_name = &field.name;
            match &field.kind {
                FieldKind::ByteArray { .. } => {
                    let accessor_name = format_ident!("{}_mut", field.name);
                    quote! { w.#accessor_name().copy_from_slice(&content.#field_name); }
                }
                FieldKind::StructRef { name: child_writer, size } => {
                    let offset = field.bit_offset / 8;
                    let end = offset + size;
                    quote! { #child_writer::write_into(&mut w.data[#offset..#end], &content.#field_name)?; }
                }
                FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                    let setter_name = format_ident!("set_{}", field.name);
                    quote! { w.#setter_name(content.#field_name); }
                }
                FieldKind::Constant { .. } => constant_write_call(field, &quote! { w.data }),
            }
        });

    let trailer_write_calls = layout.trailer_fields.iter().map(|field| {
        let field_name = &field.name;
        match &field.kind {
            FieldKind::ByteArray { len } => {
                let offset = field.bit_offset / 8;
                let end = offset + len;
                quote! {
                    w.data[trailer_base + #offset..trailer_base + #end]
                        .copy_from_slice(&content.#field_name);
                }
            }
            FieldKind::StructRef { name: child_writer, size } => {
                let offset = field.bit_offset / 8;
                let end = offset + size;
                quote! {
                    #child_writer::write_into(
                        &mut w.data[trailer_base + #offset..trailer_base + #end],
                        &content.#field_name,
                    )?;
                }
            }
            FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                let ty = field_type(field);
                let body = setter_body_into(field, &quote! { trailer });
                quote! {
                    {
                        let value: #ty = content.#field_name;
                        let trailer = &mut w.data[trailer_base..];
                        #body
                    }
                }
            }
            FieldKind::Constant { .. } => {
                let body = constant_write_call(field, &quote! { trailer });
                quote! {
                    {
                        let trailer = &mut w.data[trailer_base..];
                        #body
                    }
                }
            }
        }
    });

    let prefix_content_fields = layout
        .prefix_fields
        .iter()
        .filter(|field| {
            field.name != layout.len_field_str
                && !matches!(field.kind, FieldKind::Constant { .. })
        })
        .map(|field| {
            let field_name = &field.name;
            let ty = field_type(field);
            quote! { pub #field_name: #ty }
        });

    let trailer_content_fields = layout
        .trailer_fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(|field| {
            let field_name = &field.name;
            let ty = field_type(field);
            quote! { pub #field_name: #ty }
        });

    quote! {
        #[derive(Clone, Copy)]
        pub struct #lens_name {
            pub #region_field: usize,
        }

        pub struct #writer_name<'a> {
            data: &'a mut [u8],
            lens: #lens_name,
        }

        impl<'a> #writer_name<'a> {
            pub fn encoded_len(lens: &#lens_name) -> usize {
                #region_offset + lens.#region_field + #trailer_prefix_size
            }

            pub fn new(data: &'a mut [u8], lens: #lens_name) -> ::binparse::WriteResult<Self> {
                let need = Self::encoded_len(&lens);
                if data.len() < need {
                    return Err(::binparse::WriteError::NotEnoughSpace {
                        expected: need,
                        got: data.len(),
                    });
                }
                if #len_value_param > (#len_ty::MAX as usize) {
                    return Err(::binparse::WriteError::ValueTooLarge {
                        field: #len_field_qualified,
                        value: #len_value_param,
                        max: #len_ty::MAX as usize,
                    });
                }
                let mut me = Self { data, lens };
                me.write_len();
                Ok(me)
            }

            #write_len

            #(#prefix_setters)*

            pub fn #region_mut(&mut self) -> &mut [u8] {
                let off = #region_offset;
                &mut self.data[off..off + self.lens.#region_field]
            }

            #(#trailer_setters)*

            pub fn write_into(data: &'a mut [u8], content: &#content_name) -> ::binparse::WriteResult<usize> {
                let lens = #lens_name { #region_field: content.#region_field.len() };
                let need = Self::encoded_len(&lens);
                let mut w = Self::new(data, lens)?;
                #(#prefix_write_calls)*
                w.#region_mut().copy_from_slice(content.#region_field);
                let trailer_base = #region_offset + w.lens.#region_field;
                #(#trailer_write_calls)*
                Ok(need)
            }

            pub fn to_vec(content: &#content_name) -> ::std::vec::Vec<u8> {
                let lens = #lens_name { #region_field: content.#region_field.len() };
                let mut buf = ::std::vec![0u8; Self::encoded_len(&lens)];
                let _ = #writer_name::write_into(&mut buf, content);
                buf
            }
        }

        pub struct #content_name<'a> {
            #(#prefix_content_fields,)*
            pub #region_field: &'a [u8],
            #(#trailer_content_fields,)*
        }
    }
}

fn to_pascal_case(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut capitalize = true;
    for ch in name.chars() {
        if ch == '_' {
            capitalize = true;
        } else if capitalize {
            result.extend(ch.to_uppercase());
            capitalize = false;
        } else {
            result.push(ch);
        }
    }
    result
}

fn field_type(field: &WriterField) -> TokenStream {
    match &field.kind {
        FieldKind::Primitive { primitive, .. } => crate::match_primitive(primitive).1,
        FieldKind::BitField { .. } => quote! { u8 },
        FieldKind::ByteArray { len } => quote! { [u8; #len] },
        FieldKind::StructRef { name, .. } => {
            let content = content_name_from_writer(name);
            quote! { #content }
        }
        FieldKind::Constant { .. } => unreachable!(),
    }
}

fn field_bit_width(field: &WriterField) -> usize {
    match &field.kind {
        FieldKind::Primitive { primitive, .. } => crate::match_primitive(primitive).0.byte * 8,
        FieldKind::BitField { width, .. } => *width,
        FieldKind::ByteArray { len } => *len * 8,
        FieldKind::StructRef { size, .. } => *size * 8,
        FieldKind::Constant { width, .. } => *width,
    }
}

fn constant_write_call(field: &WriterField, target: &TokenStream) -> TokenStream {
    let FieldKind::Constant {
        value,
        primitive,
        width,
        bit_order,
        endian,
    } = &field.kind
    else {
        unreachable!()
    };
    match primitive {
        Some(primitive) => {
            let ty = crate::match_primitive(primitive).1;
            let value = proc_macro2::Literal::u128_unsuffixed(*value as u128);
            let body = primitive_setter_body(primitive, *endian, field.bit_offset / 8, target);
            quote! {
                {
                    let value: #ty = #value;
                    #body
                }
            }
        }
        None => {
            let value = proc_macro2::Literal::u128_unsuffixed(*value as u128);
            let body = bitfield_setter_body(*width, *bit_order, field.bit_offset, target);
            quote! {
                {
                    let value: u8 = #value;
                    #body
                }
            }
        }
    }
}

fn content_name_from_writer(writer: &syn::Ident) -> syn::Ident {
    let writer = writer.to_string();
    let base = writer.strip_suffix("Writer").unwrap_or(&writer);
    format_ident!("{}Content", base)
}

fn setter_body(field: &WriterField) -> TokenStream {
    let target = quote! { self.data };
    setter_body_into(field, &target)
}

fn setter_body_into(field: &WriterField, target: &TokenStream) -> TokenStream {
    match &field.kind {
        FieldKind::Primitive { primitive, endian } => {
            primitive_setter_body(primitive, *endian, field.bit_offset / 8, target)
        }
        FieldKind::BitField { width, bit_order } => {
            bitfield_setter_body(*width, *bit_order, field.bit_offset, target)
        }
        FieldKind::ByteArray { .. } | FieldKind::StructRef { .. } | FieldKind::Constant { .. } => {
            unreachable!()
        }
    }
}

fn primitive_setter_body(
    primitive: &ast::Primitive,
    endian: Endian,
    offset: usize,
    target: &TokenStream,
) -> TokenStream {
    let single_byte = crate::single_byte_read(primitive);
    if let Some(read) = single_byte {
        if read.is_empty() {
            quote! { #target[#offset] = value; }
        } else {
            quote! { #target[#offset] = value as u8; }
        }
    } else {
        let (len, _) = crate::match_primitive(primitive);
        let end = offset + len.byte;
        let to_bytes = match endian {
            Endian::Big => quote! { to_be_bytes },
            Endian::Little => quote! { to_le_bytes },
        };
        quote! { #target[#offset..#end].copy_from_slice(&value.#to_bytes()); }
    }
}

fn bitfield_setter_body(
    width: usize,
    bit_order: BitOrder,
    bit_offset: usize,
    target: &TokenStream,
) -> TokenStream {
    let byte_idx = bit_offset / 8;
    let bit_idx = bit_offset % 8;
    let mask = (1u8 << width) - 1;

    if bit_idx + width <= 8 {
        match bit_order {
            BitOrder::Msb => {
                let shift = 8 - bit_idx - width;
                let clear = !(mask << shift);
                quote! {
                    #target[#byte_idx] = (#target[#byte_idx] & #clear) | ((value & #mask) << #shift);
                }
            }
            BitOrder::Lsb => {
                let clear = !(mask << bit_idx);
                quote! {
                    #target[#byte_idx] = (#target[#byte_idx] & #clear) | ((value & #mask) << #bit_idx);
                }
            }
        }
    } else {
        let bits_in_first = 8 - bit_idx;
        let bits_in_second = width - bits_in_first;
        let second_byte = byte_idx + 1;

        match bit_order {
            BitOrder::Msb => {
                let first_mask = (1u8 << bits_in_first) - 1;
                let second_shift = 8 - bits_in_second;
                let second_mask = (1u8 << bits_in_second) - 1;
                let clear_first = !first_mask;
                let clear_second = !(second_mask << second_shift);
                quote! {
                    let value = value & #mask;
                    #target[#byte_idx] = (#target[#byte_idx] & #clear_first) | (value >> #bits_in_second);
                    #target[#second_byte] = (#target[#second_byte] & #clear_second) | ((value & #second_mask) << #second_shift);
                }
            }
            BitOrder::Lsb => {
                let first_mask = (1u8 << bits_in_first) - 1;
                let second_mask = (1u8 << bits_in_second) - 1;
                let clear_first = !(first_mask << bit_idx);
                let clear_second = !second_mask;
                quote! {
                    let value = value & #mask;
                    #target[#byte_idx] = (#target[#byte_idx] & #clear_first) | ((value & #first_mask) << #bit_idx);
                    #target[#second_byte] = (#target[#second_byte] & #clear_second) | (value >> #bits_in_first);
                }
            }
        }
    }
}
