use std::{
    collections::{HashMap, HashSet},
    ops::{Add, Mul},
};

use binparse::Len;
use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::quote;

use struct_::*;

mod field;
mod struct_;
mod type_;

// TODO: uncomment this once you are ready to fix tests
//
// #[cfg(test)]
// mod tests;

pub struct CodeGen<'a> {
    todo: HashMap<&'a str, Todo<'a>>,
    done: HashMap<&'a str, GeneratedStruct>,
}

pub struct Todo<'a> {
    origin: &'a ast::Struct<'a>,
    dependencies: HashSet<&'a str>,
    dependants: HashSet<&'a str>,
}

impl<'a> Todo<'a> {
    pub fn new(origin: &'a ast::Struct<'a>) -> Self {
        Self {
            origin,
            dependencies: HashSet::new(),
            dependants: HashSet::new(),
        }
    }
}

#[derive(Debug, Clone)]
enum GeneratedLen {
    Fixed(Len),
    Dynamic(TokenStream),
}

impl Add for GeneratedLen {
    type Output = GeneratedLen;

    fn add(self, other: Self) -> Self::Output {
        match (self, other) {
            (GeneratedLen::Dynamic(a), GeneratedLen::Fixed(Len { byte, bit })) => {
                GeneratedLen::Dynamic(quote! { (#a) + ::binparse::Len { byte: #byte, bit: #bit } })
            }
            (GeneratedLen::Fixed(Len { byte, bit }), GeneratedLen::Dynamic(a)) => {
                GeneratedLen::Dynamic(quote! { ::binparse::Len { byte: #byte, bit: #bit } + (#a) })
            }
            (GeneratedLen::Dynamic(a), GeneratedLen::Dynamic(b)) => {
                GeneratedLen::Dynamic(quote! { (#a) + (#b) })
            }
            (GeneratedLen::Fixed(a), GeneratedLen::Fixed(b)) => GeneratedLen::Fixed(a + b),
        }
    }
}

impl Mul<usize> for GeneratedLen {
    type Output = GeneratedLen;

    fn mul(self, other: usize) -> Self::Output {
        match self {
            GeneratedLen::Dynamic(a) => GeneratedLen::Dynamic(quote! { #a * #other }),
            GeneratedLen::Fixed(a) => GeneratedLen::Fixed(a * other),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("duplicate struct definition '{name}'")]
    DuplicateStruct { name: String },
    #[error("failed to generate struct '{name}': {source}")]
    GenerateStruct {
        name: String,
        source: struct_::Error,
    },
}

impl<'a> CodeGen<'a> {
    pub fn generate(ast: &'a [ast::Definition<'a>]) -> Result<String, Error> {
        let mut structs = HashMap::new();
        let mut reverse_deps = HashMap::<_, HashSet<_>>::new();

        let mut roots = Vec::new();
        #[expect(unused)]
        let mut error_enum = None;

        for def in ast {
            let s = match def {
                ast::Definition::Struct(s) => s,
                #[expect(unused_assignments)]
                ast::Definition::Error(variants) => {
                    error_enum = Some(variants);
                    continue;
                }
            };

            let mut new_s = Todo::new(s);
            find_dependencies(&s.items, &mut new_s.dependencies);

            if new_s.dependencies.is_empty() {
                roots.push(s.name);
            } else {
                for dependency in &new_s.dependencies {
                    reverse_deps.entry(*dependency).or_default().insert(s.name);
                }
            }

            if structs.insert(s.name, new_s).is_some() {
                return Err(Error::DuplicateStruct {
                    name: s.name.to_owned(),
                });
            }
        }

        for (struct_, actual) in reverse_deps {
            let dependants = &mut structs.get_mut(struct_).expect("dependant not found when they should have been added when building dependencies").dependants;
            *dependants = actual;
        }

        let mut me = Self {
            todo: structs,
            done: HashMap::new(),
        };
        let mut next = Vec::new();
        while !roots.is_empty() {
            for root in roots.drain(..) {
                let todo = me.todo.remove(root).expect("root not found in todo");

                let mut accum = StructAccum::new(todo.origin.name, &me.done);
                let generated =
                    struct_::generate(todo.origin, &mut accum)
                        .map_err(|source| Error::GenerateStruct {
                            name: root.to_string(),
                            source,
                        })?;
                me.done.insert(root, generated);

                for dependant in todo.dependants {
                    let dependant_todo = me.todo.get_mut(dependant).expect("dependant not found");
                    dependant_todo.dependencies.remove(root);
                    if dependant_todo.dependencies.is_empty() {
                        next.push(dependant);
                    }
                }
            }

            std::mem::swap(&mut next, &mut roots);
        }

        me.print()
    }

    fn print(self) -> Result<String, Error> {
        let combined: TokenStream = self.done.into_values().map(|s| s.tokens).collect();
        let file: syn::File = syn::parse2(combined).expect("failed to parse generated tokens");
        Ok(prettyplease::unparse(&file))
    }
}

fn find_dependencies<'a>(
    struct_items: &[ast::StructItem<'a>],
    dependencies: &mut HashSet<&'a str>,
) {
    for item in struct_items {
        match item {
            ast::StructItem::Field(ast::Field {
                value: ast::FieldValue::Type(ty),
                ..
            }) => find_type_dependencies(ty, dependencies),
            ast::StructItem::Conditional(conditional) => {
                find_conditional_dependencies(conditional, dependencies)
            }
            _ => {}
        }
    }
}

fn find_type_dependencies<'a>(ty: &ast::Type<'a>, dependencies: &mut HashSet<&'a str>) {
    match ty {
        ast::Type::StructRef(name) => {
            dependencies.insert(name);
        }
        ast::Type::Array(ast::ArrayType {
            elem_ty: ast::ArrayElemType::StructRef(name),
            ..
        }) => {
            dependencies.insert(name);
        }
        ast::Type::Concat(items) => {
            for item in items {
                find_type_dependencies(&item.ty, dependencies);
            }
        }
        ast::Type::Union(union) => {
            for variant in &union.variants {
                if let ast::UnionBody::NamedInline(_, items) = &variant.body {
                    find_dependencies(items, dependencies);
                }
            }
        }
        _ => {}
    }
}

fn find_conditional_dependencies<'a>(
    conditional: &ast::Conditional<'a>,
    dependencies: &mut HashSet<&'a str>,
) {
    find_dependencies(&conditional.then_branch, dependencies);
    if let Some(else_branch) = &conditional.else_branch {
        find_dependencies(else_branch, dependencies);
    }
}

fn match_binop(binop: ast::NumericBinaryOp) -> (TokenStream, Box<dyn Fn(usize, usize) -> usize>) {
    use ast::NumericBinaryOp::*;

    match binop {
        Add => (quote! { + }, Box::new(|a, b| a + b)),
        Sub => (quote! { - }, Box::new(|a, b| a - b)),
        Mul => (quote! { * }, Box::new(|a, b| a * b)),
        Div => (quote! { / }, Box::new(|a, b| a / b)),
        Mod => (quote! { % }, Box::new(|a, b| a % b)),
        BitAnd => (quote! { & }, Box::new(|a, b| a & b)),
        BitOr => (quote! { | }, Box::new(|a, b| a | b)),
        BitXor => (quote! { ^ }, Box::new(|a, b| a ^ b)),
    }
}

fn match_primitive(primitive: &ast::Primitive) -> (Len, TokenStream) {
    match primitive {
        ast::Primitive::U8 => (Len { byte: 1, bit: 0 }, quote! { u8 }),
        ast::Primitive::U16 => (Len { byte: 2, bit: 0 }, quote! { u16 }),
        ast::Primitive::U32 => (Len { byte: 4, bit: 0 }, quote! { u32 }),
        ast::Primitive::U64 => (Len { byte: 8, bit: 0 }, quote! { u64 }),
        ast::Primitive::U128 => (Len { byte: 16, bit: 0 }, quote! { u128 }),
    }
}
