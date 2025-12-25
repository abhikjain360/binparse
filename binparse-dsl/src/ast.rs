use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq)]
pub enum Primitive {
    U8,
    U16,
    U32,
    U64,
    U128,
    BitField(u8), // b<N>
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal<'a> {
    Int(u128),
    Binary { val: u128, width: u8 },
    String(Cow<'a, str>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr<'a> {
    Literal(Literal<'a>),
    Ident(Cow<'a, str>),
    Binary(Box<Expr<'a>>, BinaryOp, Box<Expr<'a>>),
    Member(Box<Expr<'a>>, Cow<'a, str>), // access field members (e.g. inner.len)
    Call(Cow<'a, str>, Vec<Expr<'a>>), // For functions or macros
    Tuple(Vec<Expr<'a>>), // For tuple matching in unions
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute<'a> {
    pub name: Cow<'a, str>,
    pub args: Vec<Expr<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field<'a> {
    pub name: Option<Cow<'a, str>>,
    pub ty: Type<'a>,
    pub attributes: Vec<Attribute<'a>>,
    pub value_constraint: Option<Expr<'a>>, // field = 0x10
}

#[derive(Debug, Clone, PartialEq)]
pub struct Conditional<'a> {
    pub condition: Expr<'a>,
    pub then_branch: Vec<StructItem<'a>>,
    pub else_branch: Option<Vec<StructItem<'a>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructItem<'a> {
    Field(Field<'a>),
    Conditional(Conditional<'a>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnionVariant<'a> {
    pub matchers: Vec<Expr<'a>>, // 0 | 8 => ...
    pub body: UnionBody<'a>,     // The struct definition or reference
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnionBody<'a> {
    // Spec: "Echo { ... }".
    // This is effectively defining a struct inline.
    NamedInline(Cow<'a, str>, Vec<StructItem<'a>>),
    // Error variant: @error(ERROR_NAME { field: expr, ... })
    Error(Cow<'a, str>, Vec<(Cow<'a, str>, Expr<'a>)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnionDef<'a> {
    pub args: Vec<Cow<'a, str>>, // union(arg1, arg2)
    pub variants: Vec<UnionVariant<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type<'a> {
    Primitive(Primitive),
    Array(Box<Type<'a>>, Option<Expr<'a>>), // [Type; LengthExpr] or [Type]
    StructRef(Cow<'a, str>),               // Reference to another struct
    Concat(Vec<Field<'a>>),              // concat(f1: type, ...)
    Union(UnionDef<'a>),                 // union(...) { ... }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDef<'a> {
    pub name: Cow<'a, str>,
    pub attributes: Vec<Attribute<'a>>,
    pub items: Vec<StructItem<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ErrorVariant<'a> {
    pub name: Cow<'a, str>,
    pub fields: Vec<(Cow<'a, str>, Primitive)>, // Spec says primitive fields only for errors
}

#[derive(Debug, Clone, PartialEq)]
pub enum Definition<'a> {
    Struct(StructDef<'a>),
    Error(Vec<ErrorVariant<'a>>),
}
