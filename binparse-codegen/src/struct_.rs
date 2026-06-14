use std::collections::HashMap;

use binparse::Len;
use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{
    GeneratedLen,
    attr::{Inherited, ParsedAttrs},
    expr, field,
};

#[derive(Clone, Copy)]
pub(crate) enum DoneFieldType {
    Primitive,
    BitField,
    Hook,
    Other,
}

pub(crate) struct DoneField {
    pub(crate) name: String,
    pub(crate) field_type: DoneFieldType,
    pub(crate) conditional: bool,
}

pub(crate) struct StructAccum<'a> {
    pub(crate) name: syn::Ident,
    pub(crate) inherited: Inherited,
    pub(crate) offset: GeneratedLen,
    pub(crate) struct_len: Option<ast::Expr<'a>>,
    pub(crate) fill_to_bound_field: Option<String>,
    pub(crate) done_fields: Vec<DoneField>,
    pub(crate) other_entities: TokenStream,
    pub(crate) field_definitions: TokenStream,
    pub(crate) functions: TokenStream,
    pub(crate) parse_checks: TokenStream,
    pub(crate) tree_stmts: TokenStream,
    pub(crate) dissect_stmts: TokenStream,
    pub(crate) last_offset_getter_fn_name: Option<syn::Ident>,
    pub(crate) condition: Option<syn::Ident>,
    pub(crate) conditional_count: usize,
    pub(crate) discriminators: Vec<syn::Ident>,
    pub(crate) payload: Option<Payload>,
}

pub(crate) struct Payload {
    pub(crate) start_offset_fn: syn::Ident,
    pub(crate) end_offset_fn: syn::Ident,
}

pub(crate) struct GeneratedStruct {
    pub(crate) len: GeneratedLen,
    pub(crate) tokens: TokenStream,
    pub(crate) last_offset_getter: Option<syn::Ident>,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("failed to generate field '{name}': {error}")]
    Field {
        name: String,
        #[source]
        error: crate::field::Error,
    },
    #[error("'field {field}' needs byte-alignment, but previous fields didn't align")]
    Unaligned { field: String },
    #[error("fill-to-bound array field '{field}' must be the last field in the struct")]
    FillToBoundNotLast { field: String },
    #[error(transparent)]
    Attr(#[from] crate::attr::Error),
    #[error(transparent)]
    Expr(#[from] expr::Error),
}

impl<'a> StructAccum<'a> {
    pub(crate) fn new(name: &str, inherited: Inherited) -> Self {
        Self {
            name: format_ident!("{}", name),
            inherited,
            offset: GeneratedLen::Fixed(Len { byte: 0, bit: 0 }),
            struct_len: None,
            fill_to_bound_field: None,
            done_fields: vec![],
            other_entities: TokenStream::new(),
            field_definitions: TokenStream::new(),
            functions: TokenStream::new(),
            parse_checks: TokenStream::new(),
            tree_stmts: TokenStream::new(),
            dissect_stmts: TokenStream::new(),
            last_offset_getter_fn_name: None,
            condition: None,
            conditional_count: 0,
            discriminators: Vec::new(),
            payload: None,
        }
    }
}

pub(crate) fn generate<'a>(
    ast: &'a ast::Struct<'a>,
    done: &mut HashMap<&'a str, GeneratedStruct>,
    errors: &[ast::ErrorVariant<'_>],
) -> Result<(), Error> {
    let attrs = ParsedAttrs::parse(&ast.attributes)?;
    let struct_inherited = attrs.merge_inherited(Inherited::default());
    let generated = generate_struct(
        ast.name,
        &ast.items,
        struct_inherited,
        attrs.len.clone(),
        done,
        errors,
        TokenStream::new(),
    )?;

    done.insert(ast.name, generated);

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn generate_struct<'a>(
    name: &str,
    items: &[ast::StructItem<'a>],
    inherited: Inherited,
    struct_len: Option<ast::Expr<'a>>,
    done: &HashMap<&'a str, GeneratedStruct>,
    errors: &[ast::ErrorVariant<'_>],
    extra_attrs: TokenStream,
) -> Result<GeneratedStruct, Error> {
    let mut accum = StructAccum::new(name, inherited);
    accum.struct_len = struct_len;

    generate_items(items, done, &mut accum, errors)?;

    let struct_len_fn = apply_struct_len(&mut accum)?;

    let StructAccum {
        name,
        inherited: _,
        offset,
        struct_len: _,
        fill_to_bound_field: _,
        done_fields: _,
        other_entities,
        field_definitions,
        functions,
        parse_checks,
        tree_stmts,
        dissect_stmts,
        last_offset_getter_fn_name,
        condition: _,
        conditional_count: _,
        discriminators,
        payload,
    } = accum;

    let last_offset_getter = last_offset_getter_fn_name.clone();

    let struct_name_str = name.to_string();
    let tree_total_bits = match &last_offset_getter {
        Some(fn_name) => quote! { self.#fn_name().bits() },
        None => quote! { 0usize },
    };
    let trailing_push = struct_len_fn.as_ref().map(|fn_name| {
        quote! {
            {
                let bound_end = self.#fn_name().byte.min(self.data.len());
                let consumed = children
                    .last()
                    .map(|child| child.bit_range.end.div_ceil(8))
                    .unwrap_or(0)
                    .min(bound_end);
                if consumed < bound_end {
                    children.push(
                        ::binparse::FieldNode::new(
                                "trailing",
                                "[u8]",
                                consumed.saturating_mul(8)..bound_end.saturating_mul(8),
                                ::binparse::Value::Bytes(&self.data[consumed..bound_end]),
                            )
                            .hide(),
                    );
                }
            }
        }
    });
    let tree_children = if tree_stmts.is_empty() && trailing_push.is_none() {
        quote! { let children = ::std::vec::Vec::new(); }
    } else {
        quote! {
            let mut children = ::std::vec::Vec::new();
            #tree_stmts
            #trailing_push
        }
    };
    let tree_fn = quote! {
        pub fn field_tree(&self) -> ::binparse::FieldNode<'a> {
            #tree_children
            let mut root = ::binparse::FieldNode::new(
                    #struct_name_str,
                    #struct_name_str,
                    0usize..#tree_total_bits,
                    ::binparse::Value::Struct,
                )
                .with_children(children);
            root.set_paths("");
            root
        }
    };

    let dissect_body = if dissect_stmts.is_empty() {
        quote! {
            let _ = Self { data };
            let children: ::std::vec::Vec<::binparse::FieldNode<'a>> = ::std::vec::Vec::new();
            let mut root = ::binparse::FieldNode::new(
                    #struct_name_str,
                    #struct_name_str,
                    0usize..0usize,
                    ::binparse::Value::Struct,
                )
                .with_children(children);
            root.set_paths("");
            root
        }
    } else {
        let dissect_trailing = struct_len_fn.as_ref().map(|fn_name| {
            quote! {
                if fatal.is_none() {
                    let bound = me.#fn_name().byte;
                    if data.len() < bound {
                        fatal = Some(::binparse::ParseError::NotEnoughData {
                            expected: bound,
                            got: data.len(),
                        });
                    } else {
                        let consumed = children
                            .last()
                            .map(|child| child.bit_range.end.div_ceil(8))
                            .unwrap_or(0)
                            .min(bound);
                        if consumed < bound {
                            children.push(
                                ::binparse::FieldNode::new(
                                        "trailing",
                                        "[u8]",
                                        consumed.saturating_mul(8)..bound.saturating_mul(8),
                                        ::binparse::Value::Bytes(&me.data[consumed..bound]),
                                    )
                                    .hide(),
                            );
                        }
                    }
                }
            }
        });
        let root_end = match &struct_len_fn {
            Some(fn_name) => quote! {
                if fatal.is_some() {
                    children.last().map(|child| child.bit_range.end).unwrap_or(0)
                } else {
                    me.#fn_name().bits()
                }
            },
            None => quote! { children.last().map(|child| child.bit_range.end).unwrap_or(0) },
        };
        quote! {
            let me = Self { data };
            let mut children: ::std::vec::Vec<::binparse::FieldNode<'a>> = ::std::vec::Vec::new();
            let mut fatal: Option<::binparse::ParseError> = None;
            #dissect_stmts
            #dissect_trailing
            let root_end = #root_end;
            let mut root = ::binparse::FieldNode::new(
                    #struct_name_str,
                    #struct_name_str,
                    0usize..root_end,
                    ::binparse::Value::Struct,
                )
                .with_children(children);
            if let Some(error) = fatal {
                root = root.with_status(::binparse::Status::Error(error));
            }
            root.set_paths("");
            root
        }
    };
    let dissect_fn = quote! {
        pub fn dissect(data: &'a [u8]) -> ::binparse::FieldNode<'a> {
            #dissect_body
        }
    };

    let handoff_fn = match &payload {
        Some(payload) => {
            let start_fn = &payload.start_offset_fn;
            let end_fn = &payload.end_offset_fn;
            let keys = discriminators
                .iter()
                .map(|field| quote! { self.#field() as u128 });
            quote! {
                pub fn handoff(&self) -> Option<::binparse::Handoff<'a>> {
                    let start = self.#start_fn();
                    let end = self.#end_fn();
                    if !start.is_byte_aligned() || !end.is_byte_aligned() {
                        return None;
                    }
                    let start = start.byte.min(self.data.len());
                    let end = end.byte.clamp(start, self.data.len());
                    Some(::binparse::Handoff {
                        keys: ::std::vec![#(#keys),*],
                        payload: &self.data[start..end],
                        payload_byte_range: start..end,
                    })
                }
            }
        }
        None => quote! {
            pub fn handoff(&self) -> Option<::binparse::Handoff<'a>> {
                None
            }
        },
    };

    let dissect_impl = quote! {
        impl<'a> ::binparse::Dissect<'a> for #name<'a> {
            fn field_tree(&self) -> ::binparse::FieldNode<'a> {
                #name::field_tree(self)
            }

            fn handoff(&self) -> Option<::binparse::Handoff<'a>> {
                #name::handoff(self)
            }
        }
    };

    let parse_fn = if let Some(fn_name) = last_offset_getter_fn_name {
        quote! {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), ::binparse::ParseError> {
                let me = Self { data };
                #parse_checks
                let len = me.#fn_name();
                if len.bit != 0 {
                    return Err(::binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(::binparse::ParseError::NotEnoughData { expected: len.byte, got: data.len() });
                }
                Ok((me, &data[len.byte..]))
            }
        }
    } else {
        quote! {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), ::binparse::ParseError> {
                Ok((Self { data }, data))
            }
        }
    };

    let tokens = quote! {
        #other_entities

        #extra_attrs
        pub struct #name<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
            #field_definitions
        }

        impl<'a> #name<'a> {
            #parse_fn

            #tree_fn

            #dissect_fn

            #handoff_fn

            #functions
        }

        #dissect_impl
    };

    Ok(GeneratedStruct {
        len: offset,
        tokens,
        last_offset_getter,
    })
}

/// Struct-level `@len(expr)` declares the struct's total length to be `expr`
/// bytes, evaluated against the struct's own earlier-declared fields. The bound
/// replaces the running end offset, so the struct's `parse()` requires `expr`
/// bytes (a `NotEnoughData` after the source fields are themselves length
/// checked, since field checks run first) and advances by exactly `expr`
/// regardless of how many bytes the fields consumed. Using the bound as the
/// struct length also makes a bounded struct report `expr` as its size when it
/// appears as a struct-ref field, array element, or union variant. Bytes inside
/// the bound that no field consumed are not an error; they are reachable from
/// the parent's slice but are not exposed via a dedicated accessor.
fn apply_struct_len(accum: &mut StructAccum<'_>) -> Result<Option<syn::Ident>, Error> {
    let Some(len_expr) = &accum.struct_len else {
        return Ok(None);
    };
    let lowered = expr::lower(len_expr, expr::ExprType::Numeric, &accum.done_fields)?;
    let len_tokens = lowered.tokens;
    let struct_len_fn_name = format_ident!("struct_len");
    accum.functions.extend(quote! {
        #[allow(dead_code)]
        fn #struct_len_fn_name(&self) -> binparse::Len {
            binparse::Len { byte: #len_tokens, bit: 0 }
        }
    });
    accum.offset = match lowered.const_value {
        Some(byte) => GeneratedLen::Fixed(Len { byte, bit: 0 }),
        None => GeneratedLen::Dynamic(quote! { self.#struct_len_fn_name() }),
    };
    accum.last_offset_getter_fn_name = Some(struct_len_fn_name.clone());
    Ok(Some(struct_len_fn_name))
}

fn generate_items<'a>(
    items: &[ast::StructItem<'a>],
    done: &HashMap<&'a str, GeneratedStruct>,
    accum: &mut StructAccum<'a>,
    errors: &[ast::ErrorVariant<'_>],
) -> Result<(), Error> {
    for item in items {
        if let Some(field) = &accum.fill_to_bound_field {
            return Err(Error::FillToBoundNotLast {
                field: field.clone(),
            });
        }
        match item {
            ast::StructItem::Field(ast_field) => {
                field::generate(ast_field, done, accum, errors).map_err(|error| Error::Field {
                    name: ast_field.name.to_string(),
                    error,
                })?;
            }
            ast::StructItem::Conditional(conditional) => {
                generate_conditional(conditional, done, accum, errors)?;
            }
        }
    }

    Ok(())
}

fn generate_conditional<'a>(
    conditional: &ast::Conditional<'a>,
    done: &HashMap<&'a str, GeneratedStruct>,
    accum: &mut StructAccum<'a>,
    errors: &[ast::ErrorVariant<'_>],
) -> Result<(), Error> {
    let condition = expr::lower(
        &conditional.condition,
        expr::ExprType::Bool,
        &accum.done_fields,
    )?
    .tokens;

    let index = accum.conditional_count;
    accum.conditional_count += 1;
    let present_fn_name = format_ident!("conditional_{}_present", index);
    let absent_fn_name = format_ident!("conditional_{}_absent", index);
    let end_offset_fn_name = format_ident!("conditional_{}_end_offset", index);

    let start_offset = accum.offset.clone();
    let start_getter_fn_name = accum.last_offset_getter_fn_name.clone();
    let start_expr = match &start_getter_fn_name {
        Some(fn_name) => quote! { self.#fn_name() },
        None => quote! { binparse::Len::ZERO },
    };

    let outer_gate = accum.condition.clone();
    let gated = |branch_condition: TokenStream| match &outer_gate {
        Some(gate) => quote! { self.#gate() && #branch_condition },
        None => branch_condition,
    };

    let then_condition = gated(quote! { #condition });
    accum.functions.extend(quote! {
        #[allow(dead_code, unused_parens)]
        fn #present_fn_name(&self) -> bool {
            #then_condition
        }
    });
    accum.condition = Some(present_fn_name.clone());
    generate_items(&conditional.then_branch, done, accum, errors)?;
    let then_end_expr = match &accum.last_offset_getter_fn_name {
        Some(fn_name) => quote! { self.#fn_name() },
        None => quote! { binparse::Len::ZERO },
    };

    accum.offset = start_offset;
    accum.last_offset_getter_fn_name = start_getter_fn_name;

    let else_end_expr = match &conditional.else_branch {
        Some(else_branch) => {
            let else_condition = gated(quote! { !#condition });
            accum.functions.extend(quote! {
                #[allow(dead_code, unused_parens)]
                fn #absent_fn_name(&self) -> bool {
                    #else_condition
                }
            });
            accum.condition = Some(absent_fn_name);
            generate_items(else_branch, done, accum, errors)?;
            match &accum.last_offset_getter_fn_name {
                Some(fn_name) => quote! { self.#fn_name() },
                None => quote! { binparse::Len::ZERO },
            }
        }
        None => start_expr,
    };

    accum.condition = outer_gate;

    accum.functions.extend(quote! {
        fn #end_offset_fn_name(&self) -> binparse::Len {
            if self.#present_fn_name() {
                #then_end_expr
            } else {
                #else_end_expr
            }
        }
    });

    accum.offset = GeneratedLen::Dynamic(quote! { self.#end_offset_fn_name() });
    accum.last_offset_getter_fn_name = Some(end_offset_fn_name);

    Ok(())
}
