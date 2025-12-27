#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Primitive {
    U8,
    U16,
    U32,
    U64,
    U128,
    BitField(u8), // b<N>
}

#[derive(Debug, Clone, Copy, PartialEq)]
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntLiteral {
    pub value: u128,
    pub width: u8,
    pub ty: IntType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntType {
    Decimal,
    Hex,
    Binary,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal<'a> {
    Int(IntLiteral),
    String(&'a str),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr<'a> {
    Literal(Literal<'a>),
    Path(Vec<&'a str>),
    Binary(Box<BinaryExpr<'a>>),
    Call(&'a str, Vec<Expr<'a>>), // macros
    Tuple(Vec<Expr<'a>>),         // tuple matching in unions
}

#[derive(Debug, Clone, PartialEq)]
pub struct BinaryExpr<'a> {
    pub lhs: Expr<'a>,
    pub op: BinaryOp,
    pub rhs: Expr<'a>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute<'a> {
    pub name: &'a str,
    pub args: Vec<Expr<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field<'a> {
    pub name: &'a str,
    pub attributes: Vec<Attribute<'a>>,
    pub value: FieldValue<'a>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue<'a> {
    Type(Type<'a>),
    Constraint(Expr<'a>), // field = 0x10
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
    pub body: UnionBody<'a>,     // struct definition or reference
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnionBody<'a> {
    // spec: "Echo { ... }".
    // effectively defining a struct inline
    NamedInline(&'a str, Vec<StructItem<'a>>),
    // error variant: @error(ERROR_NAME { field: expr, ... })
    Error(&'a str, Vec<(&'a str, Expr<'a>)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Union<'a> {
    pub args: Vec<&'a str>, // union(arg1, arg2)
    pub variants: Vec<UnionVariant<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BitMatchType {
    pub width: u8,
    pub value: u128,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayType<'a> {
    pub elem_ty: Type<'a>,
    pub size_expr: Option<Expr<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type<'a> {
    BitMatch(BitMatchType),
    Primitive(Primitive),
    Array(Box<ArrayType<'a>>),
    StructRef(Vec<&'a str>), // Reference to another struct
    Concat(Vec<Field<'a>>),  // concat(f1: type, ...)
    Union(Union<'a>),        // union(...) { ... }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Struct<'a> {
    pub name: &'a str,
    pub attributes: Vec<Attribute<'a>>,
    pub items: Vec<StructItem<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ErrorVariant<'a> {
    pub name: &'a str,
    pub fields: Vec<(&'a str, Primitive)>, // Spec says primitive fields only for errors
}

#[derive(Debug, Clone, PartialEq)]
pub enum Definition<'a> {
    Struct(Struct<'a>),
    Error(Vec<ErrorVariant<'a>>),
}
