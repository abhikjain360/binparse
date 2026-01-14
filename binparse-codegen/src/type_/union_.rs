use std::collections::HashMap;

use binparse::Len;
use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{
    GeneratedLen,
    field::FieldCtx,
    struct_::{DoneField, GeneratedStruct},
    type_::{self, GeneratedType},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("union has no arguments")]
    NoArguments,
    #[error("union has no variants")]
    NoVariants,
    #[error("matcher has {got} elements but union has {expected} arguments")]
    MatcherCountMismatch { expected: usize, got: usize },
}

pub(crate) struct UnionCtx<'a, 'b> {
    pub(crate) union: &'a ast::Union<'a>,
    pub(crate) field_name: &'b syn::Ident,
    pub(crate) parent_struct_name: &'b syn::Ident,
    pub(crate) start_offset: GeneratedLen,
    #[expect(dead_code)]
    pub(crate) done_fields: &'a [DoneField<'a>],
    pub(crate) done: &'b HashMap<&'a str, GeneratedStruct>,
}

impl UnionCtx<'_, '_> {
    pub(crate) fn generate(self) -> Result<GeneratedType, super::Error> {
        let num_args = self.union.args.len();
        if num_args == 0 {
            return Err(super::Error::Union(Error::NoArguments));
        }
        if self.union.variants.is_empty() {
            return Err(super::Error::Union(Error::NoVariants));
        }

        let start_byte: TokenStream = match &self.start_offset {
            GeneratedLen::Fixed(offset) => {
                if offset.bit != 0 {
                    return Err(type_::Error::InvalidAlignment(*offset));
                }
                let byte = offset.byte;
                quote! { #byte }
            }
            GeneratedLen::Dynamic(tokens) => {
                quote! { (#tokens).byte }
            }
        };

        let discriminants: Vec<_> = self
            .union
            .args
            .iter()
            .map(|arg| format_ident!("{}", arg))
            .collect();
        let match_expr = if discriminants.len() == 1 {
            let d = &discriminants[0];
            quote! { self.#d() }
        } else {
            let getters = discriminants.iter().map(|d| quote! { self.#d() });
            quote! { (#(#getters),*) }
        };
        let enum_name = format_ident!("{}_{}", self.parent_struct_name, self.field_name);

        let mut variant_structs = TokenStream::new();
        let mut enum_variants = TokenStream::new();
        let mut match_arms = TokenStream::new();
        let mut len_match_arms = TokenStream::new();

        for variant in &self.union.variants {
            for matcher in &variant.matchers {
                let arity = Self::matcher_arity(matcher);
                if arity != num_args {
                    return Err(super::Error::Union(Error::MatcherCountMismatch {
                        expected: num_args,
                        got: arity,
                    }));
                }
            }

            let ast::UnionBody::NamedInline(variant_name, items) = &variant.body else {
                todo!("@error union variants");
            };

            let variant_ident = format_ident!("{}", variant_name);
            let struct_name = format_ident!(
                "{}_{}_{}",
                self.parent_struct_name,
                self.field_name,
                variant_name
            );

            let (variant_struct, variant_len) =
                self.generate_variant_struct(&struct_name, items)?;
            variant_structs.extend(variant_struct);

            enum_variants.extend(quote! {
                #variant_ident(#struct_name<'a>),
            });

            let matchers = self.generate_matchers(&variant.matchers);
            let variant_len_byte = match variant_len {
                GeneratedLen::Fixed(len) => {
                    let byte = len.byte;
                    quote! { #byte }
                }
                GeneratedLen::Dynamic(tokens) => tokens,
            };

            match_arms.extend(quote! {
                #matchers => #enum_name::#variant_ident(#struct_name { data: &self.data[#start_byte..] }),
            });

            len_match_arms.extend(quote! {
                #matchers => ::binparse::Len { byte: #variant_len_byte, bit: 0 },
            });
        }

        let helper_entities = quote! {
            #variant_structs

            pub enum #enum_name<'a> {
                #enum_variants
            }
        };

        let field_getter_body = quote! {
            match #match_expr {
                #match_arms
            }
        };

        let len = GeneratedLen::Dynamic(quote! {
            match #match_expr {
                #len_match_arms
            }
        });

        Ok(GeneratedType {
            len,
            definitions: quote! {},
            helper_fns: quote! {},
            helper_entities,
            field_getter_body,
            return_ty: quote! { #enum_name<'_> },
        })
    }

    fn generate_variant_struct(
        &self,
        struct_name: &syn::Ident,
        items: &[ast::StructItem<'_>],
    ) -> Result<(TokenStream, GeneratedLen), super::Error> {
        let mut functions = TokenStream::new();
        let mut offset = GeneratedLen::Fixed(Len { byte: 0, bit: 0 });
        let mut done_fields: Vec<DoneField> = vec![];

        for item in items {
            let ast::StructItem::Field(field) = item else {
                todo!("conditional fields in union variants");
            };

            let field_ctx =
                FieldCtx::new(field, offset.clone(), &done_fields, self.done, struct_name);
            let generated = field_ctx
                .generate()
                .map_err(|e| super::Error::Field(Box::new(e)))?;

            functions.extend(generated.field_getter);
            functions.extend(generated.offset_getter);

            offset = offset + generated.len.clone();

            done_fields.push(DoneField {
                origin: field,
                len: generated.len,
                offset_getter_fn_name: generated.offset_getter_fn_name,
            });
        }

        let variant_struct = quote! {
            #[allow(non_camel_case_types)]
            pub struct #struct_name<'a> {
                data: &'a [u8],
            }

            impl<'a> #struct_name<'a> {
                #functions
            }
        };

        Ok((variant_struct, offset))
    }

    fn generate_matchers(&self, matchers: &[ast::UnionMatcher<'_>]) -> TokenStream {
        let patterns: Vec<TokenStream> = matchers.iter().map(Self::generate_matcher).collect();

        if patterns.len() == 1 {
            patterns.into_iter().next().unwrap()
        } else {
            quote! { #(#patterns)|* }
        }
    }

    fn matcher_arity(matcher: &ast::UnionMatcher<'_>) -> usize {
        match matcher {
            ast::UnionMatcher::Literal(_) | ast::UnionMatcher::Wildcard => 1,
            ast::UnionMatcher::Tuple(elements) => elements.len(),
        }
    }

    fn generate_matcher(matcher: &ast::UnionMatcher<'_>) -> TokenStream {
        match matcher {
            ast::UnionMatcher::Literal(ast::Literal::Int(int_lit)) => {
                let value = proc_macro2::Literal::usize_unsuffixed(int_lit.value);
                quote! { #value }
            }
            ast::UnionMatcher::Literal(other) => {
                todo!("non-integer literal matcher: {:?}", other)
            }
            ast::UnionMatcher::Wildcard => quote! { _ },
            ast::UnionMatcher::Tuple(elements) => {
                let element_patterns: Vec<_> =
                    elements.iter().map(Self::generate_matcher).collect();
                quote! { (#(#element_patterns),*) }
            }
        }
    }
}
