use std::collections::{HashMap, HashSet};

use binparse_dsl as ast;

use crate::context;

#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("duplicate struct definition '{name}'")]
    DuplicateStruct { name: String },
    #[error("unknown struct reference '{name}'")]
    UnknownStruct { name: String },
    #[error("cyclic dependency detected: {cycle:?}")]
    CyclicDependency { cycle: Vec<String> },
}

pub enum TryAnalyze<'a, T> {
    Done(T),
    NeedsStruct(&'a str),
}

impl<'a> context::Analysis<'a> {
    pub fn analyze(ast: &'a [ast::Struct<'a>]) -> Result<context::Analysis<'a>, AnalysisError> {
        let mut structs = HashMap::new();
        for struct_ in ast {
            if structs.insert(struct_.name, struct_).is_some() {
                return Err(AnalysisError::DuplicateStruct {
                    name: struct_.name.to_owned(),
                });
            }
        }

        let mut me = Self::default();
        let mut dep_graph = HashMap::<_, HashSet<_>>::new();
        let mut in_degrees = HashMap::new();

        let mut root = Vec::new();

        for struct_ in ast {
            let mut dependencies = HashSet::new();
            find_dependencies(&struct_.items, &mut dependencies);
            if dependencies.is_empty() {
                root.push(struct_.name);
                continue;
            }
            in_degrees.insert(struct_.name, dependencies.len());
            for dependency in dependencies {
                dep_graph
                    .entry(dependency)
                    .or_default()
                    .insert(struct_.name);
            }
        }

        if root.is_empty() {
            let Some(mut current) = dep_graph.keys().next() else {
                return Ok(me);
            };

            let mut cycle = vec![current.to_string()];
            let mut visited = HashSet::new();
            visited.insert(current);
            while let Some(next) = dep_graph[current].iter().next() {
                if visited.contains(next) {
                    cycle.push(next.to_string());
                    break;
                }
                visited.insert(next);
                current = next;
            }

            if cycle.first() != cycle.last() {
                panic!("cycle detected but not found: {cycle:?}");
            }

            return Err(AnalysisError::CyclicDependency { cycle });
        }

        let mut next = Vec::new();
        while !root.is_empty() {
            for struct_ in root.drain(..) {
                me.analyze_struct(structs.get(struct_).expect("struct not found"))?;
                let Some(dependants) = dep_graph.remove(struct_) else {
                    continue;
                };
                for dependant in dependants {
                    let in_degree = in_degrees
                        .get_mut(dependant)
                        .expect("dependant's in-degree not found");
                    *in_degree -= 1;
                    if *in_degree == 0 {
                        next.push(dependant);
                        in_degrees.remove(dependant);
                    }
                }
            }

            std::mem::swap(&mut next, &mut root);
        }

        Ok(me)
    }

    fn analyze_struct(&mut self, struct_: &'a ast::Struct<'a>) -> Result<(), AnalysisError> {
        let items = self.analyze_struct_items(&struct_.items)?;

        let mut len = context::Len::Static(0);
        for item in items {
            match item {
                context::StructItem::Field(field) => {
                    len = len + field.len;
                }
                context::StructItem::Conditional(conditional) => {
                    len = len + conditional.len;
                }
            }
        }

        let mut analysis = context::Struct::new(struct_);

        self.structs.insert(struct_.name, analysis);

        Ok(())
    }

    fn analyze_struct_items(
        &mut self,
        items: &[ast::StructItem<'a>],
    ) -> Result<Vec<context::StructItem<'a>>, AnalysisError> {
        let mut analysis = Vec::with_capacity(items.len());
        for item in items {
            match item {
                ast::StructItem::Field(field) => {
                    analysis.push(context::StructItem::Field(self.analyze_field(field)?));
                }
                ast::StructItem::Conditional(conditional) => {
                    analysis.push(context::StructItem::Conditional(
                        self.analyze_conditional(conditional)?,
                    ));
                }
            }
        }

        Ok(analysis)
    }

    fn analyze_field(
        &mut self,
        field: &ast::Field<'a>,
    ) -> Result<context::Field<'a>, AnalysisError> {
        todo!()
    }

    fn analyze_conditional(
        &mut self,
        conditional: &ast::Conditional<'a>,
    ) -> Result<context::Conditional<'a>, AnalysisError> {
        todo!()
    }
}

fn find_dependencies<'a>(
    struct_items: &[ast::StructItem<'a>],
    dependencies: &mut HashSet<&'a str>,
) {
    for item in struct_items {
        match item {
            ast::StructItem::Field(ast::Field { ty, .. }) => {
                find_type_dependencies(ty, dependencies)
            }
            ast::StructItem::Conditional(conditional) => {
                find_conditional_dependencies(conditional, dependencies)
            }
        }
    }
}

fn find_type_dependencies<'a>(ty: &ast::Type<'a>, dependencies: &mut HashSet<&'a str>) {
    match ty {
        ast::Type::StructRef(path) => {
            dependencies.insert(path.last().unwrap());
        }
        ast::Type::Array(array_ty) => {
            find_type_dependencies(&array_ty.elem_ty, dependencies);
        }
        ast::Type::Concat(fields) => {
            for field in fields {
                find_type_dependencies(&field.ty, dependencies);
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
