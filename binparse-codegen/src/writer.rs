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
    MultiByteArray {
        primitive: ast::Primitive,
        endian: Endian,
        count: usize,
    },
    Concat {
        items: Vec<WriterField>,
        bytes: usize,
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

struct GreedyStructTail {
    fields: Vec<WriterField>,
    array_field: syn::Ident,
    prefix_size: usize,
    child_writer: syn::Ident,
    child_content: syn::Ident,
    size: usize,
}

struct ConditionalLayout {
    prefix_fields: Vec<WriterField>,
    prefix_size: usize,
    condition: TokenStream,
    then_fields: Vec<WriterField>,
    then_size: usize,
    else_fields: Vec<WriterField>,
    else_size: usize,
}

struct UnionVariantInfo {
    name: syn::Ident,
    reader_variant: syn::Ident,
    fields: Vec<WriterField>,
    disc_values: Option<Vec<proc_macro2::Literal>>,
}

struct UnionLayout {
    field_name: String,
    prefix_fields: Vec<WriterField>,
    discs: Vec<WriterField>,
    prefix_size: usize,
    variants: Vec<UnionVariantInfo>,
}

struct LenUnionLayout {
    union: UnionLayout,
    region_offset: usize,
    len_field_str: String,
    len_primitive: ast::Primitive,
    len_offset: usize,
    len_endian: Endian,
    len_adjust: LenAdjust,
}

struct LenUnionHookLayout {
    union: UnionLayout,
    encode_fn: TokenStream,
    width_fn: TokenStream,
    return_ty: TokenStream,
    len_adjust: LenAdjust,
}

enum RegionKind {
    Bytes,
    StructRef {
        child_writer: syn::Ident,
        child_content: syn::Ident,
        size: usize,
    },
    ArrayOfStructs {
        child_writer: syn::Ident,
        child_content: syn::Ident,
        size: usize,
    },
}

struct ForwardLayout {
    prefix_fields: Vec<WriterField>,
    region_field: syn::Ident,
    region_offset: usize,
    region_kind: RegionKind,
    len_field_str: String,
    len_primitive: ast::Primitive,
    len_offset: usize,
    len_endian: Endian,
    len_adjust: LenAdjust,
    trailer_fields: Vec<WriterField>,
    trailer_prefix_size: usize,
}

enum Layout {
    Fixed {
        fields: Vec<WriterField>,
    },
    FixedPadded {
        fields: Vec<PaddedField>,
    },
    DynamicTail {
        fields: Vec<WriterField>,
        tail: DynamicTail,
    },
    DynamicTailHook {
        fields: Vec<WriterField>,
        tail: DynamicTailHook,
    },
    ContentHook {
        fields: Vec<WriterField>,
        hook: ContentHook,
    },
    ContentHookNoWidth {
        fields: Vec<WriterField>,
        hook: ContentHookNoWidth,
    },
    DynamicTailOpen {
        fields: Vec<WriterField>,
        tail: DynamicTailOpen,
    },
    Union(UnionLayout),
    Forward(ForwardLayout),
    LenUnion(LenUnionLayout),
    LenUnionHook(LenUnionHookLayout),
    GreedyStructTail(GreedyStructTail),
    Conditional(ConditionalLayout),
}

pub(crate) fn generate(
    ast: &ast::Struct,
    writer_sizes: &HashMap<&str, usize>,
) -> Result<(TokenStream, Option<usize>), Error> {
    Ok(match classify(ast, writer_sizes)? {
        Some(Layout::Fixed { fields }) => emit_fixed(ast.name, &fields),
        Some(Layout::FixedPadded { fields }) => emit_fixed_padded(ast.name, &fields),
        Some(Layout::DynamicTail { fields, tail }) => {
            (emit_dynamic_tail(ast.name, &fields, &tail), None)
        }
        Some(Layout::DynamicTailHook { fields, tail }) => {
            (emit_dynamic_tail_hook(ast.name, &fields, &tail), None)
        }
        Some(Layout::ContentHook { fields, hook }) => {
            (emit_content_hook(ast.name, &fields, &hook), None)
        }
        Some(Layout::ContentHookNoWidth { fields, hook }) => {
            (emit_content_hook_no_width(ast.name, &fields, &hook), None)
        }
        Some(Layout::DynamicTailOpen { fields, tail }) => {
            (emit_dynamic_tail_open(ast.name, &fields, &tail), None)
        }
        Some(Layout::Union(layout)) => (emit_union(ast.name, &layout), None),
        Some(Layout::Forward(layout)) => (emit_forward(ast.name, &layout), None),
        Some(Layout::LenUnion(layout)) => (emit_len_union(ast.name, &layout), None),
        Some(Layout::LenUnionHook(layout)) => (emit_len_union_hook(ast.name, &layout), None),
        Some(Layout::GreedyStructTail(layout)) => {
            (emit_greedy_struct_tail(ast.name, &layout), None)
        }
        Some(Layout::Conditional(layout)) => (emit_conditional(ast.name, &layout), None),
        None => (TokenStream::new(), None),
    })
}

struct PendingHookLen<'a> {
    field: &'a ast::Field<'a>,
    prefix_size: usize,
    is_last: bool,
}

struct ContentHook {
    field: syn::Ident,
    prefix_size: usize,
    encode_fn: TokenStream,
    width_fn: TokenStream,
    value_ty: TokenStream,
}

struct ContentHookNoWidth {
    field: syn::Ident,
    prefix_size: usize,
    encode_fn: TokenStream,
    value_ty: TokenStream,
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

    if let Some(layout) = classify_greedy_struct_tail(ast, struct_inherited, writer_sizes) {
        return Ok(Some(Layout::GreedyStructTail(layout)));
    }

    if let Some(layout) = classify_conditional(ast, struct_inherited, writer_sizes) {
        return Ok(Some(Layout::Conditional(layout)));
    }

    if let Some(layout) = classify_forward(ast, struct_inherited, writer_sizes) {
        return Ok(Some(Layout::Forward(layout)));
    }

    if let Some(layout) = classify_len_union(ast, struct_inherited, writer_sizes)? {
        return Ok(Some(Layout::LenUnion(layout)));
    }

    if let Some(fields) = classify_padded_fixed(ast, struct_inherited, writer_sizes) {
        return Ok(Some(Layout::FixedPadded { fields }));
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
            ast::FieldValue::Type(ast::Type::Concat(items)) => {
                if !field_attrs_supported(&field_attrs) {
                    return Ok(None);
                }
                if !bit_offset.is_multiple_of(8) {
                    return Ok(None);
                }
                let Some((concat_items, concat_bits)) =
                    classify_concat_items(items, struct_inherited)
                else {
                    return Ok(None);
                };
                fields.push(WriterField {
                    name: format_ident!("{}", field.name),
                    kind: FieldKind::Concat {
                        items: concat_items,
                        bytes: concat_bits / 8,
                    },
                    bit_offset,
                });
                bit_offset += concat_bits;
                continue;
            }
            ast::FieldValue::Type(ast::Type::Array(array)) => {
                if !bit_offset.is_multiple_of(8) {
                    return Ok(None);
                }
                if let ast::ArrayElemType::Primitive(prim) = array.elem_ty
                    && !matches!(prim, ast::Primitive::U8)
                    && field_attrs_supported(&field_attrs)
                    && let Some(size) = array.size.as_ref()
                    && let Some(count) = expr::lower(size, ExprType::Numeric, &[])
                        .ok()
                        .and_then(|lowered| lowered.const_value)
                {
                    let endian = field_attrs.merge_inherited(struct_inherited).endian;
                    let (len, _) = crate::match_primitive(&prim);
                    fields.push(WriterField {
                        name: format_ident!("{}", field.name),
                        kind: FieldKind::MultiByteArray {
                            primitive: prim,
                            endian,
                            count,
                        },
                        bit_offset,
                    });
                    bit_offset += count * len.byte * 8;
                    continue;
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
                        is_last: index == last_index,
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
                    if !fields_all_simple(&fields) {
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
                if !fields_all_simple(&fields) {
                    return Ok(None);
                }
                if let Some(pending) = &pending_hook_len
                    && size_path_matches(size, pending.field.name)
                {
                    return classify_dynamic_tail_hook(ast.name, field.name, pending, fields);
                }
                let Some(tail) = classify_dynamic_tail(size, field.name, bit_offset, &fields)
                else {
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
                if index != last_index {
                    return Ok(None);
                }
                if !bit_offset.is_multiple_of(8) {
                    return Ok(None);
                }
                if !fields_all_simple(&fields) {
                    return Ok(None);
                }
                if let Some(pending) = pending_hook_len.as_ref() {
                    return classify_len_union_hook(
                        ast.name,
                        union,
                        field,
                        pending,
                        fields,
                        bit_offset,
                        struct_inherited,
                        writer_sizes,
                    );
                }
                if !field_attrs_supported(&field_attrs) {
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
                let Some((kind, width)) = constant_field_kind(
                    lit,
                    bit_offset,
                    field_attrs.merge_inherited(struct_inherited),
                ) else {
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
            | FieldKind::MultiByteArray { .. }
            | FieldKind::Concat { .. }
            | FieldKind::Constant { .. } => unreachable!(),
        };
        fields.push(WriterField {
            name: format_ident!("{}", field.name),
            kind,
            bit_offset,
        });
        bit_offset += width;
    }

    if let Some(pending) = pending_hook_len {
        return classify_content_hook(ast.name, &pending, fields);
    }

    if !bit_offset.is_multiple_of(8) {
        return Ok(None);
    }

    Ok(Some(Layout::Fixed { fields }))
}

struct ForwardRegion {
    region_field: syn::Ident,
    region_kind: RegionKind,
    len_field_str: String,
    len_primitive: ast::Primitive,
    len_offset: usize,
    len_endian: Endian,
    len_adjust: LenAdjust,
    region_offset: usize,
}

fn classify_forward(
    ast: &ast::Struct,
    struct_inherited: Inherited,
    writer_sizes: &HashMap<&str, usize>,
) -> Option<ForwardLayout> {
    let mut prefix_fields = Vec::new();
    let mut bit_offset = 0usize;
    let mut region: Option<ForwardRegion> = None;
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
                let len_field = prefix_fields
                    .iter()
                    .find(|f: &&WriterField| f.name == len_field_str)?;
                let (len_primitive, len_endian) = forward_len_field(len_field)?;
                region = Some(ForwardRegion {
                    region_field: format_ident!("{}", field.name),
                    region_kind: RegionKind::Bytes,
                    len_field_str: len_field_str.to_string(),
                    len_primitive,
                    len_offset: len_field.bit_offset / 8,
                    len_endian,
                    len_adjust,
                    region_offset: bit_offset / 8,
                });
                continue;
            }

            if let ast::FieldValue::Type(ast::Type::StructRef(child_name)) = &field.value
                && bit_offset.is_multiple_of(8)
                && let Some(len_expr) = &field_attrs.len
                && len_only_attr_supported(&field_attrs)
                && let Some((len_field_str, len_adjust)) = affine_size_shape(len_expr)
                && let Some(size) = writer_sizes.get(child_name).copied()
            {
                let len_field = prefix_fields
                    .iter()
                    .find(|f: &&WriterField| f.name == len_field_str)?;
                let (len_primitive, len_endian) = forward_len_field(len_field)?;
                region = Some(ForwardRegion {
                    region_field: format_ident!("{}", field.name),
                    region_kind: RegionKind::StructRef {
                        child_writer: format_ident!("{}Writer", child_name),
                        child_content: format_ident!("{}Content", child_name),
                        size,
                    },
                    len_field_str: len_field_str.to_string(),
                    len_primitive,
                    len_offset: len_field.bit_offset / 8,
                    len_endian,
                    len_adjust,
                    region_offset: bit_offset / 8,
                });
                continue;
            }

            if let ast::FieldValue::Type(ast::Type::Array(array)) = &field.value
                && let ast::ArrayElemType::StructRef(child_name) = &array.elem_ty
                && field_attrs_supported(&field_attrs)
                && bit_offset.is_multiple_of(8)
                && let Some(size) = array.size.as_ref()
                && expr::lower(size, ExprType::Numeric, &[])
                    .ok()
                    .and_then(|lowered| lowered.const_value)
                    .is_none()
                && let Some((len_field_str, len_adjust)) = affine_size_shape(size)
                && let Some(child_size) = writer_sizes.get(child_name).copied()
            {
                let len_field = prefix_fields
                    .iter()
                    .find(|f: &&WriterField| f.name == len_field_str)?;
                let (len_primitive, len_endian) = forward_len_field(len_field)?;
                region = Some(ForwardRegion {
                    region_field: format_ident!("{}", field.name),
                    region_kind: RegionKind::ArrayOfStructs {
                        child_writer: format_ident!("{}Writer", child_name),
                        child_content: format_ident!("{}Content", child_name),
                        size: child_size,
                    },
                    len_field_str: len_field_str.to_string(),
                    len_primitive,
                    len_offset: len_field.bit_offset / 8,
                    len_endian,
                    len_adjust,
                    region_offset: bit_offset / 8,
                });
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

    let region = region?;

    if matches!(region.region_kind, RegionKind::Bytes) && trailer_fields.is_empty() {
        return None;
    }
    if !bit_offset.is_multiple_of(8) || !trailer_bit_offset.is_multiple_of(8) {
        return None;
    }

    Some(ForwardLayout {
        prefix_fields,
        region_field: region.region_field,
        region_offset: region.region_offset,
        region_kind: region.region_kind,
        len_field_str: region.len_field_str,
        len_primitive: region.len_primitive,
        len_offset: region.len_offset,
        len_endian: region.len_endian,
        len_adjust: region.len_adjust,
        trailer_fields,
        trailer_prefix_size: trailer_bit_offset / 8,
    })
}

fn forward_len_field(len_field: &WriterField) -> Option<(ast::Primitive, Endian)> {
    match &len_field.kind {
        FieldKind::Primitive { primitive, endian } if !crate::is_signed(primitive) => {
            Some((*primitive, *endian))
        }
        _ => None,
    }
}

fn classify_greedy_struct_tail(
    ast: &ast::Struct,
    struct_inherited: Inherited,
    writer_sizes: &HashMap<&str, usize>,
) -> Option<GreedyStructTail> {
    let last_index = ast.items.len().checked_sub(1)?;
    let mut fields = Vec::new();
    let mut bit_offset = 0usize;

    for (index, item) in ast.items.iter().enumerate() {
        let ast::StructItem::Field(field) = item else {
            return None;
        };
        let field_attrs = ParsedAttrs::parse(&field.attributes).ok()?;

        if index == last_index
            && let ast::FieldValue::Type(ast::Type::Array(array)) = &field.value
            && let ast::ArrayElemType::StructRef(child_name) = &array.elem_ty
            && array.size.is_none()
            && field_attrs.greedy
            && field_attrs.until.is_none()
            && field_attrs.hook.is_none()
            && field_attrs.max_iter.is_none()
            && bit_offset.is_multiple_of(8)
            && let Some(size) = writer_sizes.get(child_name).copied()
        {
            return Some(GreedyStructTail {
                fields,
                array_field: format_ident!("{}", field.name),
                prefix_size: bit_offset / 8,
                child_writer: format_ident!("{}Writer", child_name),
                child_content: format_ident!("{}Content", child_name),
                size,
            });
        }

        let (writer_field, width) =
            classify_fixed_field(field, bit_offset, struct_inherited, writer_sizes)?;
        fields.push(writer_field);
        bit_offset += width;
    }

    None
}

fn classify_conditional(
    ast: &ast::Struct,
    struct_inherited: Inherited,
    writer_sizes: &HashMap<&str, usize>,
) -> Option<ConditionalLayout> {
    let last_index = ast.items.len().checked_sub(1)?;
    let mut prefix_fields = Vec::new();
    let mut bit_offset = 0usize;

    for (index, item) in ast.items.iter().enumerate() {
        match item {
            ast::StructItem::Field(field) => {
                let (writer_field, width) =
                    classify_fixed_field(field, bit_offset, struct_inherited, writer_sizes)?;
                prefix_fields.push(writer_field);
                bit_offset += width;
            }
            ast::StructItem::Conditional(conditional) => {
                if index != last_index {
                    return None;
                }
                if !bit_offset.is_multiple_of(8) {
                    return None;
                }
                let condition = lower_condition(&conditional.condition, &prefix_fields)?;
                let then_fields =
                    classify_branch(&conditional.then_branch, struct_inherited, writer_sizes)?;
                let then_size = then_fields.iter().map(field_bit_width).sum::<usize>() / 8;
                let (else_fields, else_size) = match &conditional.else_branch {
                    Some(else_branch) => {
                        let fields = classify_branch(else_branch, struct_inherited, writer_sizes)?;
                        let size = fields.iter().map(field_bit_width).sum::<usize>() / 8;
                        (fields, size)
                    }
                    None => (Vec::new(), 0),
                };
                return Some(ConditionalLayout {
                    prefix_fields,
                    prefix_size: bit_offset / 8,
                    condition,
                    then_fields,
                    then_size,
                    else_fields,
                    else_size,
                });
            }
        }
    }

    None
}

fn classify_branch(
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

fn lower_condition(expr: &ast::Expr<'_>, prefix_fields: &[WriterField]) -> Option<TokenStream> {
    match expr {
        ast::Expr::Literal(ast::Literal::Int(ast::IntLiteral { value, .. })) => {
            let v = *value;
            Some(quote! { #v })
        }
        ast::Expr::Path(path) => {
            let [field_name] = path.as_slice() else {
                return None;
            };
            let field = prefix_fields.iter().find(|f| f.name == *field_name)?;
            match &field.kind {
                FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                    let ident = &field.name;
                    Some(quote! { (content.#ident as usize) })
                }
                FieldKind::ByteArray { .. }
                | FieldKind::StructRef { .. }
                | FieldKind::MultiByteArray { .. }
                | FieldKind::Concat { .. }
                | FieldKind::Constant { .. } => None,
            }
        }
        ast::Expr::Binary(binary) => {
            let lhs = lower_condition(&binary.lhs, prefix_fields)?;
            let rhs = lower_condition(&binary.rhs, prefix_fields)?;
            let op = match binary.op {
                ast::BinaryOp::Bool(ast::BoolBinaryOp::Eq) => quote! { == },
                ast::BinaryOp::Bool(ast::BoolBinaryOp::Neq) => quote! { != },
                ast::BinaryOp::Bool(ast::BoolBinaryOp::Lt) => quote! { < },
                ast::BinaryOp::Bool(ast::BoolBinaryOp::Gt) => quote! { > },
                ast::BinaryOp::Bool(ast::BoolBinaryOp::Le) => quote! { <= },
                ast::BinaryOp::Bool(ast::BoolBinaryOp::Ge) => quote! { >= },
                ast::BinaryOp::Bool(ast::BoolBinaryOp::And) => quote! { && },
                ast::BinaryOp::Bool(ast::BoolBinaryOp::Or) => quote! { || },
                ast::BinaryOp::Numeric(ast::NumericBinaryOp::Add) => quote! { + },
                ast::BinaryOp::Numeric(ast::NumericBinaryOp::Sub) => quote! { - },
                ast::BinaryOp::Numeric(ast::NumericBinaryOp::Mul) => quote! { * },
                ast::BinaryOp::Numeric(ast::NumericBinaryOp::Div) => quote! { / },
                ast::BinaryOp::Numeric(ast::NumericBinaryOp::Mod) => quote! { % },
                ast::BinaryOp::Numeric(ast::NumericBinaryOp::BitAnd) => quote! { & },
                ast::BinaryOp::Numeric(ast::NumericBinaryOp::BitOr) => quote! { | },
                ast::BinaryOp::Numeric(ast::NumericBinaryOp::BitXor) => quote! { ^ },
            };
            Some(quote! { ((#lhs) #op (#rhs)) })
        }
        ast::Expr::Literal(ast::Literal::String(_))
        | ast::Expr::Call(..)
        | ast::Expr::Tuple(_)
        | ast::Expr::RawType(_) => None,
    }
}

fn len_only_attr_supported(attrs: &ParsedAttrs<'_>) -> bool {
    let ParsedAttrs {
        endian: _,
        bit_order,
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
        len: _,
        discriminator: _,
        payload: _,
        cache_len: _,
        cache_value: _,
    } = attrs;
    bit_order.is_none()
        && hook.is_none()
        && until.is_none()
        && !greedy
        && max_iter.is_none()
        && !skip
        && pad.is_none()
        && pad_to.is_none()
        && align.is_none()
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

struct PaddedField {
    field: WriterField,
    skip: bool,
}

fn classify_field_type(
    field: &ast::Field<'_>,
    field_attrs: &ParsedAttrs<'_>,
    bit_offset: usize,
    struct_inherited: Inherited,
    writer_sizes: &HashMap<&str, usize>,
) -> Option<(FieldKind, usize)> {
    match &field.value {
        ast::FieldValue::Type(ast::Type::Primitive(primitive)) => {
            if !bit_offset.is_multiple_of(8) {
                return None;
            }
            let endian = field_attrs.merge_inherited(struct_inherited).endian;
            let (len, _) = crate::match_primitive(primitive);
            Some((
                FieldKind::Primitive {
                    primitive: *primitive,
                    endian,
                },
                len.byte * 8,
            ))
        }
        ast::FieldValue::Type(ast::Type::BitField(width)) => {
            let width = *width as usize;
            if !(1..=7).contains(&width) {
                return None;
            }
            let bit_order = field_attrs.merge_inherited(struct_inherited).bit_order;
            Some((FieldKind::BitField { width, bit_order }, width))
        }
        ast::FieldValue::Type(ast::Type::Array(array)) => {
            if !bit_offset.is_multiple_of(8) {
                return None;
            }
            let size = array.size.as_ref()?;
            let count = expr::lower(size, ExprType::Numeric, &[])
                .ok()
                .and_then(|lowered| lowered.const_value)?;
            match array.elem_ty {
                ast::ArrayElemType::Primitive(ast::Primitive::U8) => {
                    Some((FieldKind::ByteArray { len: count }, count * 8))
                }
                ast::ArrayElemType::Primitive(prim) => {
                    let endian = field_attrs.merge_inherited(struct_inherited).endian;
                    let (len, _) = crate::match_primitive(&prim);
                    Some((
                        FieldKind::MultiByteArray {
                            primitive: prim,
                            endian,
                            count,
                        },
                        count * len.byte * 8,
                    ))
                }
                ast::ArrayElemType::BitField(_) | ast::ArrayElemType::StructRef(_) => None,
            }
        }
        ast::FieldValue::Type(ast::Type::StructRef(child_name)) => {
            if !bit_offset.is_multiple_of(8) {
                return None;
            }
            let size = writer_sizes.get(child_name).copied()?;
            Some((
                FieldKind::StructRef {
                    name: format_ident!("{}Writer", child_name),
                    size,
                },
                size * 8,
            ))
        }
        ast::FieldValue::Type(ast::Type::Concat(items)) => {
            if !bit_offset.is_multiple_of(8) {
                return None;
            }
            let (concat_items, concat_bits) = classify_concat_items(items, struct_inherited)?;
            Some((
                FieldKind::Concat {
                    items: concat_items,
                    bytes: concat_bits / 8,
                },
                concat_bits,
            ))
        }
        ast::FieldValue::Constraint(ast::Expr::Literal(ast::Literal::Int(lit))) => {
            constant_field_kind(
                lit,
                bit_offset,
                field_attrs.merge_inherited(struct_inherited),
            )
        }
        _ => None,
    }
}

fn padded_attrs_supported(attrs: &ParsedAttrs<'_>) -> bool {
    let ParsedAttrs {
        endian: _,
        bit_order: _,
        hook,
        check: _,
        range: _,
        until,
        greedy,
        max_iter,
        skip: _,
        pad: _,
        pad_to: _,
        align: _,
        len,
        discriminator: _,
        payload: _,
        cache_len: _,
        cache_value: _,
    } = attrs;
    hook.is_none() && until.is_none() && !greedy && max_iter.is_none() && len.is_none()
}

fn classify_padded_fixed(
    ast: &ast::Struct,
    struct_inherited: Inherited,
    writer_sizes: &HashMap<&str, usize>,
) -> Option<Vec<PaddedField>> {
    let mut fields = Vec::with_capacity(ast.items.len());
    let mut bit_offset = 0usize;
    let mut uses_layout = false;

    for item in &ast.items {
        let ast::StructItem::Field(field) = item else {
            return None;
        };
        let field_attrs = ParsedAttrs::parse(&field.attributes).ok()?;
        if !padded_attrs_supported(&field_attrs) {
            return None;
        }

        if let Some(pad) = field_attrs.pad {
            uses_layout = true;
            bit_offset += pad * 8;
        }
        if let Some(pad_to) = field_attrs.pad_to {
            uses_layout = true;
            let len = binparse::Len {
                byte: bit_offset / 8,
                bit: bit_offset % 8,
            }
            .pad_to(pad_to);
            bit_offset = len.bits();
        }
        if let Some(align) = field_attrs.align {
            uses_layout = true;
            if !bit_offset.is_multiple_of(8) || !(bit_offset / 8).is_multiple_of(align) {
                return None;
            }
        }
        if field_attrs.skip {
            uses_layout = true;
        }

        let (kind, width) = classify_field_type(
            field,
            &field_attrs,
            bit_offset,
            struct_inherited,
            writer_sizes,
        )?;
        fields.push(PaddedField {
            field: WriterField {
                name: format_ident!("{}", field.name),
                kind,
                bit_offset,
            },
            skip: field_attrs.skip,
        });
        bit_offset += width;
    }

    if !uses_layout {
        return None;
    }
    if !bit_offset.is_multiple_of(8) {
        return None;
    }
    Some(fields)
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

fn fields_all_simple(fields: &[WriterField]) -> bool {
    fields.iter().all(|f| {
        !matches!(
            f.kind,
            FieldKind::MultiByteArray { .. } | FieldKind::Concat { .. }
        )
    })
}

fn classify_concat_items(
    items: &[ast::ConcatItem<'_>],
    struct_inherited: Inherited,
) -> Option<(Vec<WriterField>, usize)> {
    let mut fields = Vec::with_capacity(items.len());
    let mut bit_offset = 0usize;
    for (i, item) in items.iter().enumerate() {
        let item_attrs = ParsedAttrs::parse(&item.attributes).ok()?;
        let item_inherited = item_attrs.merge_inherited(struct_inherited);
        let name = format_ident!("item{}", i);
        match &item.ty {
            ast::Type::Primitive(primitive) => {
                if !field_attrs_supported(&item_attrs) {
                    return None;
                }
                if !bit_offset.is_multiple_of(8) {
                    return None;
                }
                let (len, _) = crate::match_primitive(primitive);
                fields.push(WriterField {
                    name,
                    kind: FieldKind::Primitive {
                        primitive: *primitive,
                        endian: item_inherited.endian,
                    },
                    bit_offset,
                });
                bit_offset += len.byte * 8;
            }
            ast::Type::BitField(width) => {
                let width = *width as usize;
                if !(1..=7).contains(&width) {
                    return None;
                }
                if !bitfield_attrs_supported(&item_attrs) {
                    return None;
                }
                fields.push(WriterField {
                    name,
                    kind: FieldKind::BitField {
                        width,
                        bit_order: item_inherited.bit_order,
                    },
                    bit_offset,
                });
                bit_offset += width;
            }
            ast::Type::Array(array) => {
                if !field_attrs_supported(&item_attrs) {
                    return None;
                }
                if !bit_offset.is_multiple_of(8) {
                    return None;
                }
                let size = array.size.as_ref()?;
                let count = expr::lower(size, ExprType::Numeric, &[])
                    .ok()
                    .and_then(|lowered| lowered.const_value)?;
                match array.elem_ty {
                    ast::ArrayElemType::Primitive(ast::Primitive::U8) => {
                        fields.push(WriterField {
                            name,
                            kind: FieldKind::ByteArray { len: count },
                            bit_offset,
                        });
                        bit_offset += count * 8;
                    }
                    ast::ArrayElemType::Primitive(prim) => {
                        let (len, _) = crate::match_primitive(&prim);
                        fields.push(WriterField {
                            name,
                            kind: FieldKind::MultiByteArray {
                                primitive: prim,
                                endian: item_inherited.endian,
                                count,
                            },
                            bit_offset,
                        });
                        bit_offset += count * len.byte * 8;
                    }
                    ast::ArrayElemType::BitField(_) | ast::ArrayElemType::StructRef(_) => {
                        return None;
                    }
                }
            }
            ast::Type::StructRef(_) | ast::Type::Concat(_) | ast::Type::Union(_) => return None,
        }
    }
    if !bit_offset.is_multiple_of(8) {
        return None;
    }
    Some((fields, bit_offset))
}

fn union_disc_field(prefix_fields: &[WriterField], disc_name: &str) -> Option<WriterField> {
    let disc = prefix_fields.iter().find(|f| f.name == disc_name)?;
    match &disc.kind {
        FieldKind::Primitive { primitive, endian } => Some(WriterField {
            name: disc.name.clone(),
            kind: FieldKind::Primitive {
                primitive: *primitive,
                endian: *endian,
            },
            bit_offset: disc.bit_offset,
        }),
        FieldKind::BitField { width, bit_order } => Some(WriterField {
            name: disc.name.clone(),
            kind: FieldKind::BitField {
                width: *width,
                bit_order: *bit_order,
            },
            bit_offset: disc.bit_offset,
        }),
        FieldKind::ByteArray { .. }
        | FieldKind::StructRef { .. }
        | FieldKind::MultiByteArray { .. }
        | FieldKind::Concat { .. }
        | FieldKind::Constant { .. } => None,
    }
}

fn matcher_disc_values(
    matcher: &ast::UnionMatcher<'_>,
    num_args: usize,
) -> Option<Option<Vec<proc_macro2::Literal>>> {
    match matcher {
        ast::UnionMatcher::Literal(ast::Literal::Int(int_lit)) => {
            if num_args != 1 {
                return None;
            }
            Some(Some(vec![proc_macro2::Literal::u128_unsuffixed(
                int_lit.value as u128,
            )]))
        }
        ast::UnionMatcher::Literal(_) => None,
        ast::UnionMatcher::Wildcard => Some(None),
        ast::UnionMatcher::Tuple(elements) => {
            if elements.len() != num_args {
                return None;
            }
            let mut values = Vec::with_capacity(elements.len());
            for element in elements {
                match element {
                    ast::UnionMatcher::Literal(ast::Literal::Int(int_lit)) => {
                        values.push(proc_macro2::Literal::u128_unsuffixed(int_lit.value as u128))
                    }
                    _ => return None,
                }
            }
            Some(Some(values))
        }
    }
}

fn build_union_layout(
    union: &ast::Union<'_>,
    field_name: &str,
    prefix_fields: Vec<WriterField>,
    bit_offset: usize,
    struct_inherited: Inherited,
    writer_sizes: &HashMap<&str, usize>,
    lenient: bool,
) -> Option<UnionLayout> {
    let num_args = union.args.len();
    if num_args == 0 {
        return None;
    }
    let mut discs = Vec::with_capacity(num_args);
    for disc_name in &union.args {
        discs.push(union_disc_field(&prefix_fields, disc_name)?);
    }
    let wildcard_writable =
        discs.len() == 1 && matches!(discs[0].kind, FieldKind::Primitive { .. });

    let mut variants = Vec::with_capacity(union.variants.len());
    for variant in &union.variants {
        let inline = match &variant.body {
            ast::UnionBody::NamedInline(inline) => inline,
            ast::UnionBody::Error(..) => continue,
        };
        let [matcher] = variant.matchers.as_slice() else {
            let disc_values = first_matcher_disc_values(&variant.matchers, num_args)?;
            push_union_variant(
                &mut variants,
                inline,
                disc_values,
                wildcard_writable,
                struct_inherited,
                writer_sizes,
                lenient,
            )?;
            continue;
        };
        let disc_values = matcher_disc_values(matcher, num_args)?;
        push_union_variant(
            &mut variants,
            inline,
            disc_values,
            wildcard_writable,
            struct_inherited,
            writer_sizes,
            lenient,
        )?;
    }

    if variants.is_empty() {
        return None;
    }

    Some(UnionLayout {
        field_name: field_name.to_string(),
        prefix_fields,
        discs,
        prefix_size: bit_offset / 8,
        variants,
    })
}

fn is_disc_field(discs: &[WriterField], name: &syn::Ident) -> bool {
    discs.iter().any(|disc| disc.name == *name)
}

fn variant_disc_writes(
    variant: &UnionVariantInfo,
    discs: &[WriterField],
    target: &TokenStream,
) -> TokenStream {
    match &variant.disc_values {
        None => TokenStream::new(),
        Some(values) => {
            let writes = discs.iter().zip(values).map(|(disc, value)| {
                let disc_ty = field_type(disc);
                let disc_body = setter_body_into(disc, target);
                quote! {
                    let value: #disc_ty = #value;
                    #disc_body
                }
            });
            quote! { #(#writes)* }
        }
    }
}

fn union_variant_decl(variant: &UnionVariantInfo, discs: &[WriterField]) -> TokenStream {
    let reader_variant = &variant.reader_variant;
    let variant_content = format_ident!("{}Content", variant.name);
    if variant.disc_values.is_none() {
        let disc_ty = field_type(&discs[0]);
        quote! { #reader_variant { discriminant: #disc_ty, content: #variant_content } }
    } else {
        quote! { #reader_variant(#variant_content) }
    }
}

fn union_variant_size_pat(body_enum: &syn::Ident, variant: &UnionVariantInfo) -> TokenStream {
    let reader_variant = &variant.reader_variant;
    if variant.disc_values.is_none() {
        quote! { #body_enum::#reader_variant { .. } }
    } else {
        quote! { #body_enum::#reader_variant(_) }
    }
}

fn union_variant_write_pat(
    body_enum: &syn::Ident,
    variant: &UnionVariantInfo,
    discs: &[WriterField],
    target: &TokenStream,
) -> (TokenStream, TokenStream, TokenStream) {
    let reader_variant = &variant.reader_variant;
    if variant.disc_values.is_none() {
        let disc = &discs[0];
        let disc_ty = field_type(disc);
        let disc_body = setter_body_into(disc, target);
        let pat = quote! { #body_enum::#reader_variant { discriminant, content } };
        let disc_write = quote! { let value: #disc_ty = *discriminant; #disc_body };
        (pat, quote! { content }, disc_write)
    } else {
        let disc_writes = variant_disc_writes(variant, discs, target);
        (
            quote! { #body_enum::#reader_variant(c) },
            quote! { c },
            disc_writes,
        )
    }
}

fn first_matcher_disc_values(
    matchers: &[ast::UnionMatcher<'_>],
    num_args: usize,
) -> Option<Option<Vec<proc_macro2::Literal>>> {
    let mut chosen: Option<Vec<proc_macro2::Literal>> = None;
    for matcher in matchers {
        let values = matcher_disc_values(matcher, num_args)?;
        if chosen.is_none()
            && let Some(values) = values
        {
            chosen = Some(values);
        }
    }
    Some(chosen)
}

fn push_union_variant(
    variants: &mut Vec<UnionVariantInfo>,
    inline: &ast::NamedInlineStruct<'_>,
    disc_values: Option<Vec<proc_macro2::Literal>>,
    wildcard_writable: bool,
    struct_inherited: Inherited,
    writer_sizes: &HashMap<&str, usize>,
    lenient: bool,
) -> Option<()> {
    let is_wildcard = disc_values.is_none();
    let variant_inherited = match ParsedAttrs::parse(&inline.attributes) {
        Ok(attrs) if struct_attrs_supported(&attrs) => attrs.merge_inherited(struct_inherited),
        _ => return if is_wildcard || lenient { Some(()) } else { None },
    };
    let Some(fields) = classify_fixed_items(&inline.items, variant_inherited, writer_sizes) else {
        return if is_wildcard || lenient { Some(()) } else { None };
    };
    if is_wildcard && (fields.is_empty() || !wildcard_writable) {
        return Some(());
    }
    variants.push(UnionVariantInfo {
        name: format_ident!("{}", inline.name),
        reader_variant: format_ident!("{}", inline.name),
        fields,
        disc_values,
    });
    Some(())
}

fn classify_union(
    union: &ast::Union<'_>,
    field_name: &str,
    prefix_fields: Vec<WriterField>,
    bit_offset: usize,
    struct_inherited: Inherited,
    writer_sizes: &HashMap<&str, usize>,
) -> Result<Option<Layout>, Error> {
    Ok(build_union_layout(
        union,
        field_name,
        prefix_fields,
        bit_offset,
        struct_inherited,
        writer_sizes,
        false,
    )
    .map(Layout::Union))
}

fn classify_len_union(
    ast: &ast::Struct,
    struct_inherited: Inherited,
    writer_sizes: &HashMap<&str, usize>,
) -> Result<Option<LenUnionLayout>, Error> {
    let mut prefix_fields = Vec::new();
    let mut bit_offset = 0usize;
    let Some(last_index) = ast.items.len().checked_sub(1) else {
        return Ok(None);
    };

    for (index, item) in ast.items.iter().enumerate() {
        let ast::StructItem::Field(field) = item else {
            return Ok(None);
        };
        let Ok(field_attrs) = ParsedAttrs::parse(&field.attributes) else {
            return Ok(None);
        };

        if let ast::FieldValue::Type(ast::Type::Union(union)) = &field.value {
            if index != last_index {
                return Ok(None);
            }
            if !bit_offset.is_multiple_of(8) {
                return Ok(None);
            }
            let Some(len_expr) = &field_attrs.len else {
                return Ok(None);
            };
            if !len_only_attr_supported(&field_attrs) {
                return Ok(None);
            }
            let Some((len_field_str, len_adjust)) = affine_size_shape(len_expr) else {
                return Ok(None);
            };
            let Some(len_field) = prefix_fields
                .iter()
                .find(|f: &&WriterField| f.name == len_field_str)
            else {
                return Ok(None);
            };
            let Some((len_primitive, len_endian)) = forward_len_field(len_field) else {
                return Ok(None);
            };
            let len_offset = len_field.bit_offset / 8;
            let region_offset = bit_offset / 8;
            let Some(union_layout) = build_union_layout(
                union,
                field.name,
                prefix_fields,
                bit_offset,
                struct_inherited,
                writer_sizes,
                false,
            ) else {
                return Ok(None);
            };
            return Ok(Some(LenUnionLayout {
                union: union_layout,
                region_offset,
                len_field_str: len_field_str.to_string(),
                len_primitive,
                len_offset,
                len_endian,
                len_adjust,
            }));
        }

        let Some((writer_field, width)) =
            classify_fixed_field(field, bit_offset, struct_inherited, writer_sizes)
        else {
            return Ok(None);
        };
        prefix_fields.push(writer_field);
        bit_offset += width;
    }

    Ok(None)
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

#[allow(clippy::too_many_arguments)]
fn classify_len_union_hook(
    struct_name: &str,
    union: &ast::Union<'_>,
    body_field: &ast::Field<'_>,
    pending: &PendingHookLen<'_>,
    prefix_fields: Vec<WriterField>,
    bit_offset: usize,
    struct_inherited: Inherited,
    writer_sizes: &HashMap<&str, usize>,
) -> Result<Option<Layout>, Error> {
    let Ok(body_attrs) = ParsedAttrs::parse(&body_field.attributes) else {
        return Ok(None);
    };
    let Some(len_expr) = &body_attrs.len else {
        return Ok(None);
    };
    if !len_only_attr_supported(&body_attrs) {
        return Ok(None);
    }
    let Some((len_field_str, len_adjust)) = affine_size_shape(len_expr) else {
        return Ok(None);
    };
    if len_field_str != pending.field.name {
        return Ok(None);
    }
    let Ok(pending_attrs) = ParsedAttrs::parse(&pending.field.attributes) else {
        return Ok(None);
    };
    let Some(hook) = pending_attrs.hook else {
        return Ok(None);
    };
    let Some((encode_fn, width_fn)) = parse_write_hook(&pending.field.attributes) else {
        return Err(Error::MissingWriteHook {
            struct_name: struct_name.to_string(),
            field: pending.field.name.to_string(),
        });
    };
    let Some(union_layout) = build_union_layout(
        union,
        body_field.name,
        prefix_fields,
        bit_offset,
        struct_inherited,
        writer_sizes,
        true,
    ) else {
        return Ok(None);
    };
    Ok(Some(Layout::LenUnionHook(LenUnionHookLayout {
        union: union_layout,
        encode_fn,
        width_fn,
        return_ty: hook.return_ty,
        len_adjust,
    })))
}

fn parse_write_hook(attrs: &[ast::Attribute<'_>]) -> Option<(TokenStream, TokenStream)> {
    let attr = attrs.iter().find(|attr| attr.name == "write_hook")?;
    let [encode, width] = attr.args.as_slice() else {
        return None;
    };
    Some((path_to_tokens(encode)?, path_to_tokens(width)?))
}

fn classify_content_hook(
    struct_name: &str,
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
    if !parse_write_hook_present(&pending.field.attributes) {
        return Err(Error::MissingWriteHook {
            struct_name: struct_name.to_string(),
            field: pending.field.name.to_string(),
        });
    }
    if !pending.is_last || !fields_all_simple(&fields) {
        return Ok(None);
    }
    if let Some((encode_fn, width_fn)) = parse_write_hook(&pending.field.attributes) {
        let hook = ContentHook {
            field: format_ident!("{}", pending.field.name),
            prefix_size: pending.prefix_size,
            encode_fn,
            width_fn,
            value_ty: hook.return_ty,
        };
        return Ok(Some(Layout::ContentHook { fields, hook }));
    }
    let Some(encode_fn) = parse_write_hook_encode_only(&pending.field.attributes) else {
        return Ok(None);
    };
    let hook = ContentHookNoWidth {
        field: format_ident!("{}", pending.field.name),
        prefix_size: pending.prefix_size,
        encode_fn,
        value_ty: hook.return_ty,
    };
    Ok(Some(Layout::ContentHookNoWidth { fields, hook }))
}

fn parse_write_hook_encode_only(attrs: &[ast::Attribute<'_>]) -> Option<TokenStream> {
    let attr = attrs.iter().find(|attr| attr.name == "write_hook")?;
    let [encode] = attr.args.as_slice() else {
        return None;
    };
    path_to_tokens(encode)
}

fn parse_write_hook_present(attrs: &[ast::Attribute<'_>]) -> bool {
    attrs.iter().any(|attr| attr.name == "write_hook")
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

fn writer_over_fixed() -> TokenStream {
    quote! {
        pub fn writer_over(data: &'a mut [u8]) -> ::binparse::WriteResult<Self> {
            if data.len() < Self::SIZE {
                return Err(::binparse::WriteError::NotEnoughSpace {
                    expected: Self::SIZE,
                    got: data.len(),
                });
            }
            Ok(Self { data })
        }
    }
}

fn writer_over_dynamic(
    name: &str,
    region_field: &syn::Ident,
    lens_init: TokenStream,
) -> TokenStream {
    let reader_name = format_ident!("{}", name);
    let start_off = format_ident!("{}_start_offset", region_field);
    let end_off = format_ident!("{}_end_offset", region_field);
    quote! {
        pub fn writer_over(data: &'a mut [u8]) -> ::binparse::WriteResult<Self> {
            let lens = {
                let (mut view, _) = #reader_name::parse(data)
                    .map_err(|_| ::binparse::WriteError::InvalidContent)?;
                let region_bytes = view.#end_off().byte - view.#start_off().byte;
                #lens_init
            };
            let need = Self::encoded_len(&lens);
            if data.len() < need {
                return Err(::binparse::WriteError::NotEnoughSpace {
                    expected: need,
                    got: data.len(),
                });
            }
            Ok(Self { data, lens })
        }
    }
}

fn emit_fixed(name: &str, fields: &[WriterField]) -> (TokenStream, Option<usize>) {
    let writer_name = format_ident!("{}Writer", name);
    let content_name = format_ident!("{}Content", name);

    let total_bits = fields.iter().map(field_bit_width).sum::<usize>();
    let size = total_bits / 8;

    let setters = fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(fixed_field_setter);

    let write_calls = fields.iter().map(fixed_field_write_call);

    let writer_over = writer_over_fixed();

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

            #writer_over

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

fn fixed_field_setter(field: &WriterField) -> TokenStream {
    match &field.kind {
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
        FieldKind::StructRef {
            name: child_writer,
            size,
        } => {
            let accessor_name = format_ident!("{}_mut", field.name);
            let offset = field.bit_offset / 8;
            let end = offset + size;
            quote! {
                pub fn #accessor_name(&mut self) -> #child_writer<'_> {
                    #child_writer { data: &mut self.data[#offset..#end] }
                }
            }
        }
        FieldKind::MultiByteArray {
            primitive,
            endian,
            count,
        } => {
            let setter_name = format_ident!("set_{}", field.name);
            let ty = field_type(field);
            let body = multibyte_setter_body(
                primitive,
                *endian,
                *count,
                field.bit_offset / 8,
                &quote! { value },
                &quote! { self.data },
            );
            quote! {
                pub fn #setter_name(&mut self, value: #ty) -> &mut Self {
                    #body
                    self
                }
            }
        }
        FieldKind::Concat { items, .. } => {
            let setter_name = format_ident!("set_{}", field.name);
            let ty = field_type(field);
            let body = concat_setter_body(
                items,
                field.bit_offset / 8,
                &quote! { value },
                &quote! { self.data },
            );
            quote! {
                pub fn #setter_name(&mut self, value: #ty) -> &mut Self {
                    #body
                    self
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
    }
}

fn fixed_field_write_call(field: &WriterField) -> TokenStream {
    let field_name = &field.name;
    match &field.kind {
        FieldKind::ByteArray { .. } => {
            let accessor_name = format_ident!("{}_mut", field.name);
            quote! { w.#accessor_name().copy_from_slice(&content.#field_name); }
        }
        FieldKind::StructRef {
            name: child_writer,
            size,
        } => {
            let offset = field.bit_offset / 8;
            let end = offset + size;
            quote! { #child_writer::write_into(&mut w.data[#offset..#end], &content.#field_name)?; }
        }
        FieldKind::Primitive { .. }
        | FieldKind::BitField { .. }
        | FieldKind::MultiByteArray { .. }
        | FieldKind::Concat { .. } => {
            let setter_name = format_ident!("set_{}", field.name);
            quote! { w.#setter_name(content.#field_name); }
        }
        FieldKind::Constant { .. } => constant_write_call(field, &quote! { w.data }),
    }
}

fn emit_fixed_padded(name: &str, fields: &[PaddedField]) -> (TokenStream, Option<usize>) {
    let writer_name = format_ident!("{}Writer", name);
    let content_name = format_ident!("{}Content", name);

    let size = fields
        .iter()
        .map(|pf| (pf.field.bit_offset + field_bit_width(&pf.field)) / 8)
        .max()
        .unwrap_or(0);

    let setters = fields
        .iter()
        .filter(|pf| !pf.skip && !matches!(pf.field.kind, FieldKind::Constant { .. }))
        .map(|pf| fixed_field_setter(&pf.field));

    let write_calls = fields.iter().filter(|pf| !pf.skip).map(|pf| {
        if matches!(pf.field.kind, FieldKind::Constant { .. }) {
            constant_write_call(&pf.field, &quote! { w.data })
        } else {
            fixed_field_write_call(&pf.field)
        }
    });

    let writer_over = writer_over_fixed();

    let content_fields = fields
        .iter()
        .filter(|pf| !pf.skip && !matches!(pf.field.kind, FieldKind::Constant { .. }))
        .map(|pf| {
            let field_name = &pf.field.name;
            let ty = field_type(&pf.field);
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

            #writer_over

            #(#setters)*

            pub fn write_into(data: &'a mut [u8], content: &#content_name) -> ::binparse::WriteResult<usize> {
                let mut w = Self::new(data)?;
                w.data[..Self::SIZE].fill(0);
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
            FieldKind::StructRef {
                name: child_writer,
                size,
            } => {
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
            FieldKind::MultiByteArray { .. }
            | FieldKind::Concat { .. }
            | FieldKind::Constant { .. } => unreachable!(),
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
                FieldKind::MultiByteArray { .. } | FieldKind::Concat { .. } => unreachable!(),
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

    let writer_over = writer_over_dynamic(
        name,
        array_field,
        quote! { #lens_name { #array_field: region_bytes } },
    );

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

            #writer_over

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
            FieldKind::StructRef {
                name: child_writer,
                size,
            } => {
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
            FieldKind::MultiByteArray { .. }
            | FieldKind::Concat { .. }
            | FieldKind::Constant { .. } => unreachable!(),
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
            FieldKind::MultiByteArray { .. } | FieldKind::Concat { .. } => unreachable!(),
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

    let writer_over = writer_over_dynamic(
        name,
        array_field,
        quote! { #lens_name { #array_field: region_bytes } },
    );

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

            #writer_over

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

fn emit_greedy_struct_tail(name: &str, tail: &GreedyStructTail) -> TokenStream {
    let writer_name = format_ident!("{}Writer", name);
    let content_name = format_ident!("{}Content", name);
    let lens_name = format_ident!("{}Lens", name);

    let fields = &tail.fields;
    let array_field = &tail.array_field;
    let prefix_size = tail.prefix_size;
    let child_writer = &tail.child_writer;
    let child_content = &tail.child_content;
    let size = tail.size;

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
            FieldKind::StructRef {
                name: child_writer,
                size,
            } => {
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
            FieldKind::MultiByteArray { .. }
            | FieldKind::Concat { .. }
            | FieldKind::Constant { .. } => unreachable!(),
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
            FieldKind::MultiByteArray { .. } | FieldKind::Concat { .. } => unreachable!(),
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

    let writer_over = writer_over_dynamic(
        name,
        array_field,
        quote! { #lens_name { #array_field: region_bytes / #size } },
    );

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
                #prefix_size + lens.#array_field * #size
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

            #writer_over

            #(#setters)*

            pub fn write_into(data: &'a mut [u8], content: &#content_name) -> ::binparse::WriteResult<usize> {
                let lens = #lens_name { #array_field: content.#array_field.len() };
                let need = Self::encoded_len(&lens);
                let mut w = Self::new(data, lens)?;
                #(#write_calls)*
                let base = #prefix_size;
                for (i, c) in content.#array_field.iter().enumerate() {
                    let start = base + i * #size;
                    #child_writer::write_into(&mut w.data[start..start + #size], c)?;
                }
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
            pub #array_field: &'a [#child_content],
        }
    }
}

fn branch_content_fields<'a>(fields: &'a [WriterField]) -> impl Iterator<Item = TokenStream> + 'a {
    fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(|field| {
            let field_name = &field.name;
            let ty = field_type(field);
            quote! { pub #field_name: ::core::option::Option<#ty> }
        })
}

fn branch_write_calls<'a>(
    fields: &'a [WriterField],
    base: &'a TokenStream,
) -> impl Iterator<Item = TokenStream> + 'a {
    fields.iter().map(move |field| {
        let field_name = &field.name;
        match &field.kind {
            FieldKind::ByteArray { len } => {
                let offset = field.bit_offset / 8;
                let end = offset + len;
                quote! {
                    if let ::core::option::Option::Some(value) = &content.#field_name {
                        w.data[#base + #offset..#base + #end].copy_from_slice(value);
                    }
                }
            }
            FieldKind::StructRef {
                name: child_writer,
                size,
            } => {
                let offset = field.bit_offset / 8;
                let end = offset + size;
                quote! {
                    if let ::core::option::Option::Some(value) = &content.#field_name {
                        #child_writer::write_into(
                            &mut w.data[#base + #offset..#base + #end],
                            value,
                        )?;
                    }
                }
            }
            FieldKind::Primitive { .. } | FieldKind::BitField { .. } => {
                let ty = field_type(field);
                let body = setter_body_into(field, &quote! { branch });
                quote! {
                    if let ::core::option::Option::Some(value) = content.#field_name {
                        let value: #ty = value;
                        let branch = &mut w.data[#base..];
                        #body
                    }
                }
            }
            FieldKind::MultiByteArray { .. } | FieldKind::Concat { .. } => unreachable!(),
            FieldKind::Constant { .. } => {
                let body = constant_write_call(field, &quote! { branch });
                quote! {
                    {
                        let branch = &mut w.data[#base..];
                        #body
                    }
                }
            }
        }
    })
}

fn emit_conditional(name: &str, layout: &ConditionalLayout) -> TokenStream {
    let writer_name = format_ident!("{}Writer", name);
    let content_name = format_ident!("{}Content", name);

    let prefix_size = layout.prefix_size;
    let then_size = layout.then_size;
    let else_size = layout.else_size;
    let condition = &layout.condition;

    let prefix_setters = layout
        .prefix_fields
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
            FieldKind::StructRef {
                name: child_writer,
                size,
            } => {
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
            FieldKind::MultiByteArray { .. }
            | FieldKind::Concat { .. }
            | FieldKind::Constant { .. } => unreachable!(),
        });

    let prefix_write_calls = layout.prefix_fields.iter().map(|field| {
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
            FieldKind::MultiByteArray { .. } | FieldKind::Concat { .. } => unreachable!(),
            FieldKind::Constant { .. } => constant_write_call(field, &quote! { w.data }),
        }
    });

    let prefix_content_fields = layout
        .prefix_fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(|field| {
            let field_name = &field.name;
            let ty = field_type(field);
            quote! { pub #field_name: #ty }
        });

    let then_content_fields = branch_content_fields(&layout.then_fields);
    let else_content_fields = branch_content_fields(&layout.else_fields);

    let base = quote! { #prefix_size };
    let then_write_calls = branch_write_calls(&layout.then_fields, &base);
    let else_write_calls = branch_write_calls(&layout.else_fields, &base);

    let branch_write = if layout.else_fields.is_empty() {
        quote! {
            if #condition {
                #(#then_write_calls)*
            }
        }
    } else {
        quote! {
            if #condition {
                #(#then_write_calls)*
            } else {
                #(#else_write_calls)*
            }
        }
    };

    quote! {
        pub struct #writer_name<'a> {
            data: &'a mut [u8],
        }

        impl<'a> #writer_name<'a> {
            #[allow(unused_parens)]
            pub fn encoded_len(content: &#content_name) -> usize {
                #prefix_size + if #condition { #then_size } else { #else_size }
            }

            #(#prefix_setters)*

            #[allow(unused_parens)]
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
                #branch_write
                Ok(need)
            }

            pub fn to_vec(content: &#content_name) -> ::std::vec::Vec<u8> {
                let mut buf = ::std::vec![0u8; Self::encoded_len(content)];
                let _ = #writer_name::write_into(&mut buf, content);
                buf
            }
        }

        pub struct #content_name {
            #(#prefix_content_fields,)*
            #(#then_content_fields,)*
            #(#else_content_fields,)*
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
            FieldKind::StructRef {
                name: child_writer,
                size,
            } => {
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
            FieldKind::MultiByteArray { .. }
            | FieldKind::Concat { .. }
            | FieldKind::Constant { .. } => unreachable!(),
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
            FieldKind::MultiByteArray { .. } | FieldKind::Concat { .. } => unreachable!(),
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

fn emit_content_hook(name: &str, fields: &[WriterField], hook: &ContentHook) -> TokenStream {
    let writer_name = format_ident!("{}Writer", name);
    let content_name = format_ident!("{}Content", name);

    let field = &hook.field;
    let prefix_size = hook.prefix_size;
    let encode_fn = &hook.encode_fn;
    let width_fn = &hook.width_fn;
    let value_ty = &hook.value_ty;

    let setters = fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(fixed_field_setter);

    let prefix_write_calls = fields.iter().map(fixed_field_write_call);

    let prefix_content_fields = fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(|field| {
            let field_name = &field.name;
            let ty = field_type(field);
            quote! { pub #field_name: #ty }
        });

    let content_lifetime = if value_ty.to_string().contains('\'') {
        quote! { <'a> }
    } else {
        quote! {}
    };

    quote! {
        pub struct #writer_name<'a> {
            data: &'a mut [u8],
        }

        impl<'a> #writer_name<'a> {
            pub fn encoded_len(content: &#content_name) -> usize {
                #prefix_size + #width_fn(content.#field)
            }

            #(#setters)*

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
                let (head, tail) = w.data.split_at_mut(#prefix_size);
                let written = #encode_fn(
                    content.#field,
                    tail,
                    ::binparse::WriteHookContext {
                        offset: #prefix_size,
                        written: &*head,
                    },
                )?;
                Ok(#prefix_size + written)
            }

            pub fn to_vec(content: &#content_name) -> ::std::vec::Vec<u8> {
                let mut buf = ::std::vec![0u8; Self::encoded_len(content)];
                let written = #writer_name::write_into(&mut buf, content).unwrap_or(0);
                buf.truncate(written);
                buf
            }
        }

        pub struct #content_name #content_lifetime {
            #(#prefix_content_fields,)*
            pub #field: #value_ty,
        }
    }
}

fn emit_content_hook_no_width(
    name: &str,
    fields: &[WriterField],
    hook: &ContentHookNoWidth,
) -> TokenStream {
    let writer_name = format_ident!("{}Writer", name);
    let content_name = format_ident!("{}Content", name);

    let field = &hook.field;
    let prefix_size = hook.prefix_size;
    let encode_fn = &hook.encode_fn;
    let value_ty = &hook.value_ty;

    let setters = fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(fixed_field_setter);

    let prefix_write_calls = fields.iter().map(fixed_field_write_call);

    let prefix_content_fields = fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(|field| {
            let field_name = &field.name;
            let ty = field_type(field);
            quote! { pub #field_name: #ty }
        });

    let content_lifetime = if value_ty.to_string().contains('\'') {
        quote! { <'a> }
    } else {
        quote! {}
    };

    quote! {
        pub struct #writer_name<'a> {
            data: &'a mut [u8],
        }

        impl<'a> #writer_name<'a> {
            #(#setters)*

            pub fn write_into(data: &'a mut [u8], content: &#content_name) -> ::binparse::WriteResult<usize> {
                if data.len() < #prefix_size {
                    return Err(::binparse::WriteError::NotEnoughSpace {
                        expected: #prefix_size,
                        got: data.len(),
                    });
                }
                let mut w = Self { data };
                #(#prefix_write_calls)*
                let mut cursor = #prefix_size;
                let (head, tail) = w.data.split_at_mut(cursor);
                let written = #encode_fn(
                    content.#field,
                    tail,
                    ::binparse::WriteHookContext {
                        offset: cursor,
                        written: &*head,
                    },
                )?;
                cursor += written;
                Ok(cursor)
            }

            pub fn to_vec(content: &#content_name) -> ::std::vec::Vec<u8> {
                let mut cap = 64usize;
                loop {
                    let mut buf = ::std::vec![0u8; cap];
                    match #writer_name::write_into(&mut buf, content) {
                        Ok(written) => {
                            buf.truncate(written);
                            return buf;
                        }
                        Err(_) if cap < (1usize << 24) => {
                            cap *= 2;
                        }
                        Err(_) => return ::std::vec::Vec::new(),
                    }
                }
            }
        }

        pub struct #content_name #content_lifetime {
            #(#prefix_content_fields,)*
            pub #field: #value_ty,
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
    let discs = &layout.discs;

    let mut variant_entities = TokenStream::new();
    for variant in &layout.variants {
        let (tokens, _) = emit_fixed(&variant.name.to_string(), &variant.fields);
        variant_entities.extend(tokens);
    }

    let enum_variants = layout
        .variants
        .iter()
        .map(|variant| union_variant_decl(variant, discs));

    let prefix_setters = layout
        .prefix_fields
        .iter()
        .filter(|field| {
            !is_disc_field(discs, &field.name) && !matches!(field.kind, FieldKind::Constant { .. })
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
            FieldKind::StructRef {
                name: child_writer,
                size,
            } => {
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
            FieldKind::MultiByteArray { .. }
            | FieldKind::Concat { .. }
            | FieldKind::Constant { .. } => unreachable!(),
        });

    let prefix_write_calls = layout
        .prefix_fields
        .iter()
        .filter(|field| !is_disc_field(discs, &field.name))
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
                FieldKind::MultiByteArray { .. } | FieldKind::Concat { .. } => unreachable!(),
                FieldKind::Constant { .. } => constant_write_call(field, &quote! { w.data }),
            }
        });

    let content_fields = layout
        .prefix_fields
        .iter()
        .filter(|field| {
            !is_disc_field(discs, &field.name) && !matches!(field.kind, FieldKind::Constant { .. })
        })
        .map(|field| {
            let field_name = &field.name;
            let ty = field_type(field);
            quote! { pub #field_name: #ty }
        });

    let encoded_len_arms = layout.variants.iter().map(|variant| {
        let variant_writer = format_ident!("{}Writer", variant.name);
        let pat = union_variant_size_pat(&body_enum, variant);
        quote! { #pat => #variant_writer::SIZE }
    });

    let disc_target = quote! { w.data };

    let write_arms = layout.variants.iter().map(|variant| {
        let variant_writer = format_ident!("{}Writer", variant.name);
        let (pat, c, disc_writes) =
            union_variant_write_pat(&body_enum, variant, discs, &disc_target);
        quote! {
            #pat => {
                #disc_writes
                let end = #prefix_size + #variant_writer::SIZE;
                #variant_writer::write_into(&mut w.data[#prefix_size..end], #c)?;
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

fn emit_len_union(name: &str, layout: &LenUnionLayout) -> TokenStream {
    let union = &layout.union;
    let writer_name = format_ident!("{}Writer", name);
    let content_name = format_ident!("{}Content", name);
    let field_pascal = to_pascal_case(&union.field_name);
    let body_enum = format_ident!("{}{}Content", name, field_pascal);
    let body_field = format_ident!("{}", union.field_name);

    let prefix_size = union.prefix_size;
    let region_offset = layout.region_offset;
    let discs = &union.discs;
    let len_field_str = &layout.len_field_str;
    let len_ty = crate::match_primitive(&layout.len_primitive).1;
    let len_field_qualified = format!("{}.{}", name, len_field_str);

    let mut variant_entities = TokenStream::new();
    for variant in &union.variants {
        let (tokens, _) = emit_fixed(&variant.name.to_string(), &variant.fields);
        variant_entities.extend(tokens);
    }

    let enum_variants = union
        .variants
        .iter()
        .map(|variant| union_variant_decl(variant, discs));

    let prefix_setters = union
        .prefix_fields
        .iter()
        .filter(|field| {
            !is_disc_field(discs, &field.name)
                && field.name != *len_field_str
                && !matches!(field.kind, FieldKind::Constant { .. })
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
            FieldKind::StructRef {
                name: child_writer,
                size,
            } => {
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
            FieldKind::MultiByteArray { .. }
            | FieldKind::Concat { .. }
            | FieldKind::Constant { .. } => unreachable!(),
        });

    let prefix_write_calls = union
        .prefix_fields
        .iter()
        .filter(|field| !is_disc_field(discs, &field.name) && field.name != *len_field_str)
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
                FieldKind::MultiByteArray { .. } | FieldKind::Concat { .. } => unreachable!(),
                FieldKind::Constant { .. } => constant_write_call(field, &quote! { w.data }),
            }
        });

    let content_fields = union
        .prefix_fields
        .iter()
        .filter(|field| {
            !is_disc_field(discs, &field.name)
                && field.name != *len_field_str
                && !matches!(field.kind, FieldKind::Constant { .. })
        })
        .map(|field| {
            let field_name = &field.name;
            let ty = field_type(field);
            quote! { pub #field_name: #ty }
        });

    let encoded_len_arms = union.variants.iter().map(|variant| {
        let variant_writer = format_ident!("{}Writer", variant.name);
        let pat = union_variant_size_pat(&body_enum, variant);
        quote! { #pat => #variant_writer::SIZE }
    });

    let disc_target = quote! { w.data };

    let write_len = {
        let len_offset = layout.len_offset;
        let len_value = len_value_expr(&quote! { region_len }, layout.len_adjust);
        let check = quote! {
            let len_field_value = #len_value;
            if len_field_value > (#len_ty::MAX as usize) {
                return Err(::binparse::WriteError::ValueTooLarge {
                    field: #len_field_qualified,
                    value: len_field_value,
                    max: #len_ty::MAX as usize,
                });
            }
        };
        let single_byte = crate::single_byte_read(&layout.len_primitive);
        if single_byte.is_some() {
            quote! {
                #check
                w.data[#len_offset] = len_field_value as #len_ty;
            }
        } else {
            let (len, _) = crate::match_primitive(&layout.len_primitive);
            let end = len_offset + len.byte;
            let to_bytes = match layout.len_endian {
                Endian::Big => quote! { to_be_bytes },
                Endian::Little => quote! { to_le_bytes },
            };
            quote! {
                #check
                w.data[#len_offset..#end]
                    .copy_from_slice(&(len_field_value as #len_ty).#to_bytes());
            }
        }
    };

    let write_arms = union.variants.iter().map(|variant| {
        let variant_writer = format_ident!("{}Writer", variant.name);
        let (pat, c, disc_writes) =
            union_variant_write_pat(&body_enum, variant, discs, &disc_target);
        quote! {
            #pat => {
                #disc_writes
                let region_len = #variant_writer::SIZE;
                #write_len
                let end = #region_offset + region_len;
                #variant_writer::write_into(&mut w.data[#region_offset..end], #c)?;
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

fn emit_len_union_hook(name: &str, layout: &LenUnionHookLayout) -> TokenStream {
    let union = &layout.union;
    let writer_name = format_ident!("{}Writer", name);
    let content_name = format_ident!("{}Content", name);
    let field_pascal = to_pascal_case(&union.field_name);
    let body_enum = format_ident!("{}{}Content", name, field_pascal);
    let body_field = format_ident!("{}", union.field_name);

    let prefix_size = union.prefix_size;
    let discs = &union.discs;
    let encode_fn = &layout.encode_fn;
    let width_fn = &layout.width_fn;
    let return_ty = &layout.return_ty;

    let mut variant_entities = TokenStream::new();
    for variant in &union.variants {
        let (tokens, _) = emit_fixed(&variant.name.to_string(), &variant.fields);
        variant_entities.extend(tokens);
    }

    let enum_variants = union
        .variants
        .iter()
        .map(|variant| union_variant_decl(variant, discs));

    let prefix_setters = union
        .prefix_fields
        .iter()
        .filter(|field| {
            !is_disc_field(discs, &field.name) && !matches!(field.kind, FieldKind::Constant { .. })
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
            FieldKind::StructRef {
                name: child_writer,
                size,
            } => {
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
            FieldKind::MultiByteArray { .. }
            | FieldKind::Concat { .. }
            | FieldKind::Constant { .. } => unreachable!(),
        });

    let prefix_write_calls = union
        .prefix_fields
        .iter()
        .filter(|field| !is_disc_field(discs, &field.name))
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
                FieldKind::MultiByteArray { .. } | FieldKind::Concat { .. } => unreachable!(),
                FieldKind::Constant { .. } => constant_write_call(field, &quote! { w.data }),
            }
        });

    let content_fields = union
        .prefix_fields
        .iter()
        .filter(|field| {
            !is_disc_field(discs, &field.name) && !matches!(field.kind, FieldKind::Constant { .. })
        })
        .map(|field| {
            let field_name = &field.name;
            let ty = field_type(field);
            quote! { pub #field_name: #ty }
        });

    let encoded_len_arms = union.variants.iter().map(|variant| {
        let variant_writer = format_ident!("{}Writer", variant.name);
        let pat = union_variant_size_pat(&body_enum, variant);
        quote! { #pat => #variant_writer::SIZE }
    });

    let disc_target = quote! { w.data };
    let len_value = len_value_expr(&quote! { region_len }, layout.len_adjust);

    let write_arms = union.variants.iter().map(|variant| {
        let variant_writer = format_ident!("{}Writer", variant.name);
        let (pat, c, disc_writes) =
            union_variant_write_pat(&body_enum, variant, discs, &disc_target);
        quote! {
            #pat => {
                #disc_writes
                let region_len = #variant_writer::SIZE;
                let len_value = #len_value;
                let len_width = #encode_fn(len_value as #return_ty, &mut w.data[#prefix_size..])?;
                let region_off = #prefix_size + len_width;
                let end = region_off + region_len;
                #variant_writer::write_into(&mut w.data[region_off..end], #c)?;
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
                let region_len = match &content.#body_field {
                    #(#encoded_len_arms),*
                };
                let len_value = #len_value;
                #prefix_size + #width_fn(len_value as #return_ty) + region_len
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

    let region_bytes = |lens_field: TokenStream| match &layout.region_kind {
        RegionKind::Bytes | RegionKind::StructRef { .. } => lens_field,
        RegionKind::ArrayOfStructs { size, .. } => quote! { (#lens_field * #size) },
    };
    let self_region_bytes = region_bytes(quote! { self.lens.#region_field });
    let w_region_bytes = region_bytes(quote! { w.lens.#region_field });
    let lens_region_bytes = region_bytes(quote! { lens.#region_field });

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
            field.name != layout.len_field_str && !matches!(field.kind, FieldKind::Constant { .. })
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
            FieldKind::StructRef {
                name: child_writer,
                size,
            } => {
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
            FieldKind::MultiByteArray { .. }
            | FieldKind::Concat { .. }
            | FieldKind::Constant { .. } => unreachable!(),
        });

    let trailer_setters = layout
        .trailer_fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Constant { .. }))
        .map(|field| {
            let base = quote! { let base = #region_offset + #self_region_bytes; };
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
                FieldKind::StructRef {
                    name: child_writer,
                    size,
                } => {
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
                FieldKind::MultiByteArray { .. }
                | FieldKind::Concat { .. }
                | FieldKind::Constant { .. } => unreachable!(),
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
                FieldKind::MultiByteArray { .. } | FieldKind::Concat { .. } => unreachable!(),
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
            FieldKind::StructRef {
                name: child_writer,
                size,
            } => {
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
            FieldKind::MultiByteArray { .. } | FieldKind::Concat { .. } => unreachable!(),
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

    let trailer_write_block = if layout.trailer_fields.is_empty() {
        TokenStream::new()
    } else {
        quote! {
            let trailer_base = #region_offset + #w_region_bytes;
            #(#trailer_write_calls)*
        }
    };

    let encoded_len_body = if trailer_prefix_size == 0 {
        quote! { #region_offset + #lens_region_bytes }
    } else {
        quote! { #region_offset + #lens_region_bytes + #trailer_prefix_size }
    };

    let prefix_content_fields = layout
        .prefix_fields
        .iter()
        .filter(|field| {
            field.name != layout.len_field_str && !matches!(field.kind, FieldKind::Constant { .. })
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

    let (
        region_accessor,
        region_lens_init,
        region_write_call,
        region_content_field,
        content_lifetime,
    ) = match &layout.region_kind {
        RegionKind::Bytes => (
            quote! {
                pub fn #region_mut(&mut self) -> &mut [u8] {
                    let off = #region_offset;
                    &mut self.data[off..off + self.lens.#region_field]
                }
            },
            quote! { #lens_name { #region_field: content.#region_field.len() } },
            quote! { w.#region_mut().copy_from_slice(content.#region_field); },
            quote! { pub #region_field: &'a [u8] },
            quote! { <'a> },
        ),
        RegionKind::StructRef {
            child_writer,
            child_content,
            size,
        } => (
            quote! {
                pub fn #region_mut(&mut self) -> #child_writer<'_> {
                    let off = #region_offset;
                    #child_writer { data: &mut self.data[off..off + #size] }
                }
            },
            quote! { #lens_name { #region_field: #child_writer::SIZE } },
            quote! {
                #child_writer::write_into(
                    &mut w.data[#region_offset..#region_offset + #size],
                    &content.#region_field,
                )?;
            },
            quote! { pub #region_field: #child_content },
            quote! {},
        ),
        RegionKind::ArrayOfStructs {
            child_writer,
            child_content,
            size,
        } => (
            TokenStream::new(),
            quote! { #lens_name { #region_field: content.#region_field.len() } },
            quote! {
                let base = #region_offset;
                for (i, c) in content.#region_field.iter().enumerate() {
                    let start = base + i * #size;
                    #child_writer::write_into(&mut w.data[start..start + #size], c)?;
                }
            },
            quote! { pub #region_field: &'a [#child_content] },
            quote! { <'a> },
        ),
    };

    let writer_over_lens_init = match &layout.region_kind {
        RegionKind::Bytes | RegionKind::StructRef { .. } => {
            quote! { #lens_name { #region_field: region_bytes } }
        }
        RegionKind::ArrayOfStructs { size, .. } => {
            quote! { #lens_name { #region_field: region_bytes / #size } }
        }
    };
    let writer_over = writer_over_dynamic(name, region_field, writer_over_lens_init);

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
                #encoded_len_body
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

            #writer_over

            #write_len

            #(#prefix_setters)*

            #region_accessor

            #(#trailer_setters)*

            pub fn write_into(data: &'a mut [u8], content: &#content_name) -> ::binparse::WriteResult<usize> {
                let lens = #region_lens_init;
                let need = Self::encoded_len(&lens);
                let mut w = Self::new(data, lens)?;
                #(#prefix_write_calls)*
                #region_write_call
                #trailer_write_block
                Ok(need)
            }

            pub fn to_vec(content: &#content_name) -> ::std::vec::Vec<u8> {
                let lens = #region_lens_init;
                let mut buf = ::std::vec![0u8; Self::encoded_len(&lens)];
                let _ = #writer_name::write_into(&mut buf, content);
                buf
            }
        }

        pub struct #content_name #content_lifetime {
            #(#prefix_content_fields,)*
            #region_content_field,
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
        FieldKind::MultiByteArray {
            primitive, count, ..
        } => {
            let ty = crate::match_primitive(primitive).1;
            quote! { [#ty; #count] }
        }
        FieldKind::Concat { items, .. } => {
            let tys = items.iter().map(field_type);
            quote! { ( #(#tys,)* ) }
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
        FieldKind::MultiByteArray {
            primitive, count, ..
        } => crate::match_primitive(primitive).0.byte * count * 8,
        FieldKind::Concat { bytes, .. } => *bytes * 8,
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
        FieldKind::ByteArray { .. }
        | FieldKind::StructRef { .. }
        | FieldKind::MultiByteArray { .. }
        | FieldKind::Concat { .. }
        | FieldKind::Constant { .. } => {
            unreachable!()
        }
    }
}

fn multibyte_setter_body(
    primitive: &ast::Primitive,
    endian: Endian,
    count: usize,
    base: usize,
    value: &TokenStream,
    target: &TokenStream,
) -> TokenStream {
    let (len, _) = crate::match_primitive(primitive);
    let elem = len.byte;
    let base_prefix = if base == 0 {
        quote! {}
    } else {
        quote! { #base + }
    };
    let single_byte = crate::single_byte_read(primitive);
    if let Some(read) = single_byte {
        let cast = if read.is_empty() {
            quote! {}
        } else {
            quote! { as u8 }
        };
        quote! {
            for (__i, __v) in #value.iter().enumerate() {
                #target[#base_prefix __i] = (*__v) #cast;
            }
        }
    } else {
        let to_bytes = match endian {
            Endian::Big => quote! { to_be_bytes },
            Endian::Little => quote! { to_le_bytes },
        };
        quote! {
            for __i in 0..#count {
                let __start = #base_prefix __i * #elem;
                #target[__start..__start + #elem]
                    .copy_from_slice(&#value[__i].#to_bytes());
            }
        }
    }
}

fn concat_setter_body(
    items: &[WriterField],
    base_bytes: usize,
    value: &TokenStream,
    target: &TokenStream,
) -> TokenStream {
    let base_bits = base_bytes * 8;
    let writes = items.iter().enumerate().map(|(i, item)| {
        let index = proc_macro2::Literal::usize_unsuffixed(i);
        let elem = quote! { #value.#index };
        let abs_bits = base_bits + item.bit_offset;
        match &item.kind {
            FieldKind::Primitive { primitive, endian } => {
                let body = primitive_setter_body(primitive, *endian, abs_bits / 8, target);
                quote! { { let value = #elem; #body } }
            }
            FieldKind::BitField { width, bit_order } => {
                let body = bitfield_setter_body(*width, *bit_order, abs_bits, target);
                quote! { { let value = #elem; #body } }
            }
            FieldKind::ByteArray { len } => {
                let start = abs_bits / 8;
                let end = start + len;
                quote! { #target[#start..#end].copy_from_slice(&#elem); }
            }
            FieldKind::MultiByteArray {
                primitive,
                endian,
                count,
            } => multibyte_setter_body(primitive, *endian, *count, abs_bits / 8, &elem, target),
            FieldKind::StructRef { .. } | FieldKind::Concat { .. } | FieldKind::Constant { .. } => {
                unreachable!()
            }
        }
    });
    quote! { #(#writes)* }
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
