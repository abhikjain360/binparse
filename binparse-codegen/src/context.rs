use binparse_dsl as ast;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Analysis<'a> {
    pub structs: HashMap<&'a str, Struct<'a>>,
}

#[derive(Debug, Clone)]
pub struct Struct<'a> {
    pub origin: StructOrigin<'a>,
    pub items: Vec<StructItem<'a>>,
    pub len: Len<'a>,
}

#[derive(Debug, Clone)]
pub enum StructOrigin<'a> {
    Struct(&'a ast::Struct<'a>),
    Inline(InlineStructOrigin<'a>),
}

#[derive(Debug, Clone)]
pub struct InlineStructOrigin<'a> {
    pub name: &'a str,
    pub items: Vec<ast::StructItem<'a>>,
}

impl<'a> Struct<'a> {
    pub fn new(origin: &'a ast::Struct<'a>) -> Self {
        Self {
            origin: StructOrigin::Struct(origin),
            len: Len::Static(0),
            items: Vec::new(),
        }
    }

    pub fn new_inline(name: &'a str, items: Vec<ast::StructItem<'a>>) -> Self {
        Self {
            origin: StructOrigin::Inline(InlineStructOrigin { name, items }),
            len: Len::Static(0),
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum StructItem<'a> {
    Field(Field<'a>),
    Conditional(Conditional<'a>),
}

#[derive(Debug, Clone)]
pub struct Field<'a> {
    pub origin: &'a ast::Field<'a>,
    pub type_analysis: Type<'a>,
    pub len: Len<'a>,
}

#[derive(Debug, Clone, Copy)]
pub enum Case {
    Then,
    Else,
}

#[derive(Debug, Clone)]
pub enum Len<'a> {
    Static(u128),
    Path(Vec<&'a str>),
    Binary(Box<BinaryExprLen<'a>>),
    /// This should be an error when generating code.
    Unknown,
}

impl<'a> std::ops::Add for Len<'a> {
    type Output = Len<'a>;

    fn add(self, other: Len<'a>) -> Len<'a> {
        match (self, other) {
            (Len::Unknown, _) | (_, Len::Unknown) => Len::Unknown,

            (Len::Binary(expr), other) | (other, Len::Binary(expr)) => {
                Len::Binary(Box::new(BinaryExprLen {
                    lhs: Len::Binary(expr),
                    op: ast::BinaryOp::Add,
                    rhs: other,
                }))
            }

            (Len::Path(lhs), other) | (other, Len::Path(lhs)) => {
                Len::Binary(Box::new(BinaryExprLen {
                    lhs: Len::Path(lhs),
                    op: ast::BinaryOp::Add,
                    rhs: other,
                }))
            }

            (Len::Static(lhs), Len::Static(rhs)) => Len::Static(lhs + rhs),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BinaryExprLen<'a> {
    pub lhs: Len<'a>,
    pub op: ast::BinaryOp,
    pub rhs: Len<'a>,
}

#[derive(Debug, Clone)]
pub struct Conditional<'a> {
    pub origin: &'a ast::Conditional<'a>,
    pub then_fields: Vec<StructItem<'a>>,
    pub else_fields: Vec<StructItem<'a>>,
    pub len: Len<'a>,
}

#[derive(Debug, Clone)]
pub enum Type<'a> {
    Primitive(ast::Primitive),
    Array(Box<Type<'a>>, Len<'a>),
    StructRef(&'a str),
    Union(Union<'a>),
}

#[derive(Debug, Clone)]
pub struct Union<'a> {
    pub args: Vec<&'a str>,
    pub variants: Vec<UnionVariant<'a>>,
}

#[derive(Debug, Clone)]
pub struct UnionVariant<'a> {
    pub matchers: Vec<ast::Expr<'a>>,
    pub body: UnionBody<'a>,
}

#[derive(Debug, Clone)]
pub enum UnionBody<'a> {
    Structure {
        name: &'a str,
        analysis: Box<Struct<'a>>,
    },
    Error {
        name: &'a str,
        fields: Vec<(&'a str, ast::Expr<'a>)>,
    },
}

impl<'a> From<ast::Primitive> for Len<'a> {
    fn from(primitive: ast::Primitive) -> Self {
        match primitive {
            ast::Primitive::U8 => Len::Static(8),
            ast::Primitive::U16 => Len::Static(16),
            ast::Primitive::U32 => Len::Static(32),
            ast::Primitive::U64 => Len::Static(64),
            ast::Primitive::U128 => Len::Static(128),
            ast::Primitive::BitField(width) => Len::Static(width as u128),
        }
    }
}
