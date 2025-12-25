#[derive(Debug, Clone, PartialEq)]
pub enum Primitive {
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
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
pub enum Literal {
    Int(i128),
    Bool(bool),
    // We might want specific types for Hex/Bin if we want to preserve representation,
    // but for AST values i128 is likely sufficient.
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal),
    Ident(String),
    Binary(Box<Expr>, BinaryOp, Box<Expr>),
    Member(Box<Expr>, String), // access field members (e.g. inner.len)
    Call(String, Vec<Expr>), // For functions or macros
    Tuple(Vec<Expr>), // For tuple matching in unions
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: Option<String>,
    pub ty: Type,
    pub attributes: Vec<Attribute>,
    pub value_constraint: Option<Expr>, // field = 0x10
}

#[derive(Debug, Clone, PartialEq)]
pub struct Conditional {
    pub condition: Expr,
    pub then_branch: Vec<StructItem>,
    pub else_branch: Option<Vec<StructItem>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructItem {
    Field(Field),
    Conditional(Conditional),
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnionVariant {
    pub matchers: Vec<Expr>, // 0 | 8 => ...
    pub body: UnionBody,     // The struct definition or reference
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnionBody {
    InlineStruct(Vec<StructItem>), // { id: u16, ... }
    TypeRef(String), // KnownStruct
    // Spec also implies variants can be empty/unit-like or named.
    // e.g. "Echo { ... }" -> Name + InlineStruct
    // e.g. "Unknown" -> Name (Unit)
    // But in `union(type) { ... }` the variants are usually `value => Name { ... }`
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnionDef {
    pub args: Vec<String>, // union(arg1, arg2)
    pub variants: Vec<UnionVariant>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Primitive(Primitive),
    Array(Box<Type>, Expr), // [Type; LengthExpr]
    StructRef(String),      // Reference to another struct
    Concat(Vec<Field>),     // concat(f1: type, ...)
    Union(UnionDef),        // union(...) { ... }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub name: String,
    pub attributes: Vec<Attribute>,
    pub items: Vec<StructItem>, // Changed from fields to items to support conditionals
}

#[derive(Debug, Clone, PartialEq)]
pub struct ErrorVariant {
    pub name: String,
    pub fields: Vec<(String, Type)>, // Spec allows primitives: "MISSING_THIS_FLAG { found: b<3>, ... }"
}

#[derive(Debug, Clone, PartialEq)]
pub enum Definition {
    Struct(StructDef),
    Error(Vec<ErrorVariant>),
}
