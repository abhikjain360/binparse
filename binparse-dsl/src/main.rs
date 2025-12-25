use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::prelude::*;

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
pub enum Expr {
    Int(i64),
    Hex(i64), // Keep distinction if needed, or just Int
    Ident(String),
    Binary(Box<Expr>, BinaryOp, Box<Expr>),
    Call(String, Vec<Expr>), // For functions like len() or others if needed
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
pub struct UnionDef {
    pub discriminants: Vec<String>, // union(type, code)
    pub variants: Vec<UnionVariant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnionVariant {
    pub matchers: Vec<Expr>, // 0 | 8 => ...
    pub body: UnionBody,     // The struct definition or reference
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnionBody {
    Struct(Vec<Field>), // Inline struct definition
                        // Reference(String), // Could be a reference, but spec shows inline usually
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
pub struct StructDef {
    pub name: String,
    pub attributes: Vec<Attribute>,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ErrorVariant {
    pub name: String,
    pub fields: Vec<(String, Primitive)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Definition {
    Struct(StructDef),
    Error(Vec<ErrorVariant>),
}

// ... parser implementation ...
fn parser<'a>() -> impl Parser<'a, &'a str, Vec<Definition>, extra::Err<Rich<'a, char>>> {
    // Placeholder
    end().map(|_| vec![])
}

fn main() {
    let src = "struct MyPacket {
    magic: u32,
    version: u8, 
}";

    let broken_src = "struct BrokenPacket {
    magic: u32
    version: u8
"; // Missing closing brace, missing comma

    let filename = "input.chk";

    println!("--- Valid Parse ---");
    let (output, errs) = parser().parse(src).into_output_errors();
    if let Some(ast) = output {
        println!("{:#?}", ast);
    }
    for err in errs {
        Report::build(ReportKind::Error, (filename, err.span().into_range()))
            .with_message(err.to_string())
            .with_label(
                Label::new((filename, err.span().into_range()))
                    .with_message(format!("{:?}", err.reason()))
                    .with_color(Color::Red),
            )
            .finish()
            .print((filename, Source::from(src)))
            .unwrap();
    }

    println!("\n--- Broken Parse (Demo) ---");
    let (output, errs) = parser().parse(broken_src).into_output_errors();

    if let Some(ast) = output {
        println!("Partial output: {:#?}", ast);
    } else {
        println!("Parse failed.");
    }

    for err in errs {
        Report::build(ReportKind::Error, (filename, err.span().into_range()))
            .with_message(err.to_string())
            .with_label(
                Label::new((filename, err.span().into_range()))
                    .with_message(err.reason().to_string())
                    .with_color(Color::Red),
            )
            .finish()
            .print((filename, Source::from(broken_src)))
            .unwrap();
    }
}
