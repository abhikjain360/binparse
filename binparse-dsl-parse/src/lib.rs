use winnow::{
    Parser,
    ascii::{digit1, hex_digit1, multispace0},
    combinator::{
        alt, delimited, dispatch, fail, opt, peek, preceded, repeat, separated, seq, terminated,
    },
    token::{any, take_until, take_while},
};

use binparse_dsl as ast;

// --- Whitespace & Comments ---

fn line_comment<'a>(input: &mut &'a str) -> winnow::Result<()> {
    ("//", take_while(0.., |c| c != '\n'), opt('\n'))
        .void()
        .parse_next(input)
}

fn block_comment<'a>(input: &mut &'a str) -> winnow::Result<()> {
    ("/*", take_until(0.., "*/"), "*/").void().parse_next(input)
}

fn ws<'a>(input: &mut &'a str) -> winnow::Result<()> {
    loop {
        let start_len = input.len();
        multispace0.parse_next(input)?;
        if line_comment.parse_next(input).is_ok() {
            continue;
        }
        if block_comment.parse_next(input).is_ok() {
            continue;
        }
        if start_len == input.len() {
            break;
        }
    }
    Ok(())
}

fn padded<'a, O, F>(mut inner: F) -> impl Parser<&'a str, O, winnow::error::ContextError>
where
    F: Parser<&'a str, O, winnow::error::ContextError>,
{
    move |input: &mut &'a str| {
        ws.parse_next(input)?;
        let res = inner.parse_next(input)?;
        ws.parse_next(input)?;
        Ok(res)
    }
}

// --- Identifiers ---

fn ident_raw<'a>(input: &mut &'a str) -> winnow::Result<&'a str> {
    take_while(1.., |c: char| c.is_ascii_alphanumeric() || c == '_')
        .verify(|s: &str| {
            s.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        })
        .parse_next(input)
}

fn identifier<'a>(input: &mut &'a str) -> winnow::Result<&'a str> {
    let start = *input;
    let i = ident_raw(input)?;
    let reserved = ["struct", "union", "concat", "if", "else", "error", "match"];
    if reserved.contains(&i) {
        *input = start;
        fail.parse_next(input)
    } else {
        Ok(i)
    }
}

fn path<'a>(input: &mut &'a str) -> winnow::Result<Vec<&'a str>> {
    separated(1.., identifier, ".").parse_next(input)
}

// --- Literals ---

fn literal<'a>(input: &mut &'a str) -> winnow::Result<ast::Literal<'a>> {
    dispatch! {peek(any);
        '"' => string_literal,
        'x' => hex_literal,
        'b' => binary_literal,
        '0' => alt((hex_literal, binary_literal, decimal_literal)),
        '1'..='9' => decimal_literal,
        _ => fail
    }
    .parse_next(input)
}

#[derive(Debug, thiserror::Error)]
pub enum IntLiteralError {
    #[error("width too large: {0}")]
    WidthTooLarge(#[from] std::num::TryFromIntError),
    #[error("invalid integer: {0}")]
    InvalidInt(#[from] std::num::ParseIntError),
}

fn decimal_literal<'a>(input: &mut &'a str) -> winnow::Result<ast::Literal<'a>> {
    digit1
        .try_map(|s: &str| {
            let width = s.len().try_into()?;

            s.parse::<u128>()
                .map(|v| ast::IntLiteral {
                    value: v,
                    width,
                    ty: ast::IntType::Decimal,
                })
                .map_err(IntLiteralError::InvalidInt)
        })
        .map(ast::Literal::Int)
        .parse_next(input)
}

fn hex_literal<'a>(input: &mut &'a str) -> winnow::Result<ast::Literal<'a>> {
    preceded("x", hex_digit1)
        .try_map(|s: &str| {
            let width = s.len().try_into()?;
            u128::from_str_radix(s, 16)
                .map(|v| ast::IntLiteral {
                    value: v,
                    width,
                    ty: ast::IntType::Hex,
                })
                .map_err(IntLiteralError::InvalidInt)
        })
        .map(ast::Literal::Int)
        .parse_next(input)
}

fn binary_literal<'a>(input: &mut &'a str) -> winnow::Result<ast::Literal<'a>> {
    preceded("b", take_while(1.., |c| c == '0' || c == '1'))
        .try_map(|s: &str| {
            let width = s.len().try_into()?;
            u128::from_str_radix(s, 2)
                .map(|v| ast::IntLiteral {
                    value: v,
                    width,
                    ty: ast::IntType::Binary,
                })
                .map_err(IntLiteralError::InvalidInt)
        })
        .map(ast::Literal::Int)
        .parse_next(input)
}

fn string_literal<'a>(input: &mut &'a str) -> winnow::Result<ast::Literal<'a>> {
    delimited('"', take_while(0.., |c| c != '"'), '"')
        .map(|s: &str| ast::Literal::String(s))
        .parse_next(input)
}

// --- Expressions ---

fn expr<'a>(input: &mut &'a str) -> winnow::Result<ast::Expr<'a>> {
    logic_or(input)
}

fn args<'a>(input: &mut &'a str) -> winnow::Result<Vec<ast::Expr<'a>>> {
    delimited(padded('('), separated(0.., expr, padded(',')), padded(')')).parse_next(input)
}

fn atom<'a>(input: &mut &'a str) -> winnow::Result<ast::Expr<'a>> {
    padded(alt((
        literal.map(ast::Expr::Literal),
        call_or_path,
        tuple_or_group,
    )))
    .parse_next(input)
}

fn call_or_path<'a>(input: &mut &'a str) -> winnow::Result<ast::Expr<'a>> {
    let p = path(input)?;
    if p.len() == 1 {
        let name = p[0];
        let args_opt = opt(args).parse_next(input)?;
        match args_opt {
            Some(a) => Ok(ast::Expr::Call(name, a)),
            None => Ok(ast::Expr::Path(p)),
        }
    } else {
        Ok(ast::Expr::Path(p))
    }
}

fn tuple_or_group<'a>(input: &mut &'a str) -> winnow::Result<ast::Expr<'a>> {
    delimited(padded('('), separated(0.., expr, padded(',')), padded(')'))
        .map(|mut exprs: Vec<ast::Expr<'a>>| {
            if exprs.len() == 1 {
                exprs.pop().unwrap()
            } else {
                ast::Expr::Tuple(exprs)
            }
        })
        .parse_next(input)
}

fn member_access<'a>(input: &mut &'a str) -> winnow::Result<ast::Expr<'a>> {
    // member access is now folded into atom via path
    atom(input)
}

fn product<'a>(input: &mut &'a str) -> winnow::Result<ast::Expr<'a>> {
    let mut lhs = member_access(input)?;
    loop {
        let start = *input;
        let op_res: winnow::Result<ast::BinaryOp> = padded(alt((
            "*".map(|_| ast::BinaryOp::Mul),
            "/".map(|_| ast::BinaryOp::Div),
            "%".map(|_| ast::BinaryOp::Mod),
        )))
        .parse_next(input);

        match op_res {
            Ok(op) => {
                let rhs = member_access(input)?;
                lhs = ast::Expr::Binary(Box::new(ast::BinaryExpr { lhs, op, rhs }));
            }
            Err(_) => {
                *input = start;
                break;
            }
        }
    }
    Ok(lhs)
}

fn sum<'a>(input: &mut &'a str) -> winnow::Result<ast::Expr<'a>> {
    let mut lhs = product(input)?;
    loop {
        let start = *input;
        let op_res: winnow::Result<ast::BinaryOp> = padded(alt((
            "+".map(|_| ast::BinaryOp::Add),
            "-".map(|_| ast::BinaryOp::Sub),
        )))
        .parse_next(input);

        match op_res {
            Ok(op) => {
                let rhs = product(input)?;
                lhs = ast::Expr::Binary(Box::new(ast::BinaryExpr { lhs, op, rhs }));
            }
            Err(_) => {
                *input = start;
                break;
            }
        }
    }
    Ok(lhs)
}

fn bitwise<'a>(input: &mut &'a str) -> winnow::Result<ast::Expr<'a>> {
    let mut lhs = sum(input)?;
    loop {
        let start = *input;
        let op_res: winnow::Result<ast::BinaryOp> = padded(alt((
            "&".verify(|s: &str| !s.starts_with("&&"))
                .map(|_| ast::BinaryOp::BitAnd),
            "^".map(|_| ast::BinaryOp::BitXor),
            "|".verify(|s: &str| !s.starts_with("||"))
                .map(|_| ast::BinaryOp::BitOr),
        )))
        .parse_next(input);

        match op_res {
            Ok(op) => {
                let rhs = sum(input)?;
                lhs = ast::Expr::Binary(Box::new(ast::BinaryExpr { lhs, op, rhs }));
            }
            Err(_) => {
                *input = start;
                break;
            }
        }
    }
    Ok(lhs)
}

fn comparison<'a>(input: &mut &'a str) -> winnow::Result<ast::Expr<'a>> {
    let mut lhs = bitwise(input)?;
    loop {
        let start = *input;
        let op_res: winnow::Result<ast::BinaryOp> = padded(alt((
            "==".map(|_| ast::BinaryOp::Eq),
            "!=".map(|_| ast::BinaryOp::Neq),
            "<=".map(|_| ast::BinaryOp::Le),
            ">=".map(|_| ast::BinaryOp::Ge),
            "<".map(|_| ast::BinaryOp::Lt),
            ">".map(|_| ast::BinaryOp::Gt),
        )))
        .parse_next(input);

        match op_res {
            Ok(op) => {
                let rhs = bitwise(input)?;
                lhs = ast::Expr::Binary(Box::new(ast::BinaryExpr { lhs, op, rhs }));
            }
            Err(_) => {
                *input = start;
                break;
            }
        }
    }
    Ok(lhs)
}

fn logic_and<'a>(input: &mut &'a str) -> winnow::Result<ast::Expr<'a>> {
    let mut lhs = comparison(input)?;
    loop {
        let start = *input;
        let op_res: winnow::Result<ast::BinaryOp> =
            padded("&&".map(|_| ast::BinaryOp::And)).parse_next(input);
        match op_res {
            Ok(op) => {
                let rhs = comparison(input)?;
                lhs = ast::Expr::Binary(Box::new(ast::BinaryExpr { lhs, op, rhs }));
            }
            Err(_) => {
                *input = start;
                break;
            }
        }
    }
    Ok(lhs)
}

fn logic_or<'a>(input: &mut &'a str) -> winnow::Result<ast::Expr<'a>> {
    let mut lhs = logic_and(input)?;
    loop {
        let start = *input;
        let op_res: winnow::Result<ast::BinaryOp> =
            padded("||".map(|_| ast::BinaryOp::Or)).parse_next(input);
        match op_res {
            Ok(op) => {
                let rhs = logic_and(input)?;
                lhs = ast::Expr::Binary(Box::new(ast::BinaryExpr { lhs, op, rhs }));
            }
            Err(_) => {
                *input = start;
                break;
            }
        }
    }
    Ok(lhs)
}

// --- Types & Attributes ---

fn attribute<'a>(input: &mut &'a str) -> winnow::Result<ast::Attribute<'a>> {
    seq! {ast::Attribute {
        _: '@',
        name: identifier,
        args: opt(args).map(|o| o.unwrap_or_default()),
    }}
    .parse_next(input)
}

fn attributes<'a>(input: &mut &'a str) -> winnow::Result<Vec<ast::Attribute<'a>>> {
    repeat(0.., padded(attribute)).parse_next(input)
}

fn primitive<'a>(input: &mut &'a str) -> winnow::Result<ast::Primitive> {
    dispatch! {peek(any);
        'u' => alt((
            "u8".map(|_| ast::Primitive::U8),
            "u16".map(|_| ast::Primitive::U16),
            "u32".map(|_| ast::Primitive::U32),
            "u64".map(|_| ast::Primitive::U64),
            "u128".map(|_| ast::Primitive::U128),
        )),
        'b' => ("b", delimited('<', digit1, '>')).try_map(|(_, w_str): (&str, &str)| {
            w_str.parse::<u8>()
        }).verify(|w| *w <= 128).map(ast::Primitive::BitField),
        _ => fail
    }
    .parse_next(input)
}

fn type_parser<'a>(input: &mut &'a str) -> winnow::Result<ast::Type<'a>> {
    alt((
        array_type,
        concat_type,
        union_type,
        primitive.map(ast::Type::Primitive),
        padded(path).map(ast::Type::StructRef),
    ))
    .parse_next(input)
}

fn array_type<'a>(input: &mut &'a str) -> winnow::Result<ast::Type<'a>> {
    delimited(
        padded('['),
        (type_parser, opt(preceded(padded(';'), expr))),
        padded(']'),
    )
    .map(|(elem_ty, size_expr)| ast::Type::Array(Box::new(ast::ArrayType { elem_ty, size_expr })))
    .parse_next(input)
}

fn field_value<'a>(input: &mut &'a str) -> winnow::Result<ast::FieldValue<'a>> {
    alt((
        preceded(padded(':'), type_parser).map(ast::FieldValue::Type),
        preceded(padded('='), expr).map(ast::FieldValue::Constraint),
    ))
    .parse_next(input)
}

fn concat_type<'a>(input: &mut &'a str) -> winnow::Result<ast::Type<'a>> {
    preceded(
        padded("concat"),
        delimited(padded('('), separated(0.., field, padded(',')), padded(')')),
    )
    .map(ast::Type::Concat)
    .parse_next(input)
}

fn error_body<'a>(input: &mut &'a str) -> winnow::Result<ast::UnionBody<'a>> {
    // @error(ERROR_NAME { field: expr, ... }) or @error(ERROR_NAME)
    let _ = padded("@error").parse_next(input)?;
    let _ = padded('(').parse_next(input)?;
    let name = padded(identifier).parse_next(input)?;
    let fields = opt(delimited(
        padded('{'),
        separated(
            0..,
            seq! { padded(identifier), _: padded(':'), expr },
            padded(','),
        ),
        padded('}'),
    ))
    .parse_next(input)?
    .unwrap_or_default();
    let _ = padded(')').parse_next(input)?;
    Ok(ast::UnionBody::Error(name, fields))
}

fn union_body<'a>(input: &mut &'a str) -> winnow::Result<ast::UnionBody<'a>> {
    alt((
        error_body,
        seq! {
            padded(identifier),
            delimited(padded('{'), struct_items, padded('}'))
        }
        .map(|(n, items)| ast::UnionBody::NamedInline(n, items)),
    ))
    .parse_next(input)
}

fn union_variant<'a>(input: &mut &'a str) -> winnow::Result<ast::UnionVariant<'a>> {
    seq! {ast::UnionVariant {
        matchers: separated(1.., expr, padded('|')),
        _: padded("=>"),
        body: union_body,
    }}
    .parse_next(input)
}

fn union_type<'a>(input: &mut &'a str) -> winnow::Result<ast::Type<'a>> {
    preceded(
        padded("union"),
        seq! {
            delimited(padded('('), separated(0.., padded(identifier), padded(',')), padded(')')),
            delimited(padded('{'), union_variants, padded('}'))
        },
    )
    .map(|(args, variants)| ast::Type::Union(ast::Union { args, variants }))
    .parse_next(input)
}

fn union_variants<'a>(input: &mut &'a str) -> winnow::Result<Vec<ast::UnionVariant<'a>>> {
    let mut variants = Vec::new();
    loop {
        let start = *input;
        match union_variant.parse_next(input) {
            Ok(v) => {
                variants.push(v);
                // try to consume comma
                let _ = padded(',').parse_next(input);
            }
            Err(_) => {
                *input = start;
                break;
            }
        }
    }
    Ok(variants)
}

fn struct_item<'a>(input: &mut &'a str) -> winnow::Result<ast::StructItem<'a>> {
    alt((
        conditional.map(ast::StructItem::Conditional),
        field_with_opt_comma.map(ast::StructItem::Field),
    ))
    .parse_next(input)
}

fn struct_items<'a>(input: &mut &'a str) -> winnow::Result<Vec<ast::StructItem<'a>>> {
    repeat(0.., struct_item).parse_next(input)
}

fn conditional<'a>(input: &mut &'a str) -> winnow::Result<ast::Conditional<'a>> {
    seq! {ast::Conditional {
        _: padded("if"),
        condition: delimited(padded('('), expr, padded(')')),
        then_branch: delimited(padded('{'), struct_items, padded('}')),
        else_branch: opt(preceded(padded("else"), delimited(padded('{'), struct_items, padded('}')))),
    }}.parse_next(input)
}

fn field<'a>(input: &mut &'a str) -> winnow::Result<ast::Field<'a>> {
    seq! {ast::Field {
        attributes: attributes,
        name: padded(identifier),
        value: field_value,
    }}
    .parse_next(input)
}

fn field_with_opt_comma<'a>(input: &mut &'a str) -> winnow::Result<ast::Field<'a>> {
    terminated(field, opt(padded(','))).parse_next(input)

    // let type_info = opt(preceded(padded(':'), (attributes, type_parser))).parse_next(input)?;
    // let constraint = opt(preceded(padded('='), expr)).parse_next(input)?;
    // let _ = opt(padded(',')).parse_next(input)?;

    // let (type_attrs, mut ty) = match type_info {
    //     Some((a, t)) => (a, Some(t)),
    //     None => (Vec::new(), None),
    // };
    // let mut final_attrs = field_attrs;
    // final_attrs.extend(type_attrs);

    // if ty.is_none()
    //     && let Some(ast::Expr::Literal(lit)) = &constraint
    // {
    //     match lit {
    //         ast::Literal::Int(ast::IntLiteral {
    //             width,
    //             ty: ast::IntType::Binary,
    //             value,
    //         }) => {
    //             ty = Some(ast::Type::BitMatch(ast::BitMatchType {
    //                 width: *width as u8,
    //                 value: *value,
    //             }));
    //         }
    //         ast::Literal::Int(ast::IntLiteral {
    //             width,
    //             ty: ast::IntType::Hex,
    //             value,
    //         }) => {
    //             ty = Some(ast::Type::BitMatch(ast::BitMatchType {
    //                 width: *width as u8 * 4,
    //                 value: *value,
    //             }));
    //         }
    //         ast::Literal::Int(ast::IntLiteral {
    //             value,
    //             ty: ast::IntType::Decimal,
    //             ..
    //         }) => {
    //             let t = if *value <= u8::MAX as u128 {
    //                 ast::Primitive::U8
    //             } else if *value <= u16::MAX as u128 {
    //                 ast::Primitive::U16
    //             } else if *value <= u32::MAX as u128 {
    //                 ast::Primitive::U32
    //             } else if *value <= u64::MAX as u128 {
    //                 ast::Primitive::U64
    //             } else {
    //                 ast::Primitive::U128
    //             };
    //             ty = Some(ast::Type::Primitive(t));
    //         }
    //         _ => {}
    //     }
    // }
    // let final_ty = ty.unwrap_or(ast::Type::Primitive(ast::Primitive::U8));
    // Ok(ast::Field {
    //     name,
    //     ty: final_ty,
    //     attributes: final_attrs,
    //     value_constraint: constraint,
    // })
}

fn struct_def<'a>(input: &mut &'a str) -> winnow::Result<ast::Definition<'a>> {
    seq! {ast::Struct {
        attributes: attributes,
        _: padded("struct"),
        name: padded(identifier),
        items: delimited(padded('{'), struct_items, padded('}')),
    }}
    .map(ast::Definition::Struct)
    .parse_next(input)
}

fn error_variant<'a>(input: &mut &'a str) -> winnow::Result<ast::ErrorVariant<'a>> {
    let name = padded(identifier).parse_next(input)?;
    let fields = opt(delimited(
        padded('{'),
        separated(
            0..,
            seq! { padded(identifier), _: padded(':'), padded(primitive) },
            padded(','),
        ),
        padded('}'),
    ))
    .parse_next(input)?
    .unwrap_or_default();
    let _ = opt(padded(',')).parse_next(input)?;
    Ok(ast::ErrorVariant { name, fields })
}

fn error_def<'a>(input: &mut &'a str) -> winnow::Result<ast::Definition<'a>> {
    preceded(
        padded("error"),
        delimited(padded('{'), repeat(0.., padded(error_variant)), padded('}')),
    )
    .map(ast::Definition::Error)
    .parse_next(input)
}

/// Parse a BinParse DSL source string into a list of definitions.
pub fn parse<'a>(input: &mut &'a str) -> winnow::Result<Vec<ast::Definition<'a>>> {
    repeat(0.., padded(alt((struct_def, error_def)))).parse_next(input)
}

/// Convenience function that takes an owned string and returns Result.
pub fn parse_str(input: &str) -> Result<Vec<ast::Definition<'_>>, String> {
    parse.parse(input).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_helper(src: &str) -> Vec<ast::Definition<'_>> {
        match parse.parse(src) {
            Ok(defs) => defs,
            Err(e) => {
                panic!("Parse errors: \n{}", e);
            }
        }
    }

    #[test]
    fn test_simple_struct() {
        let src = r#"
            struct Simple {
                a: u8,
                b: u16,
            }
        "#;
        let defs = parse_helper(src);
        assert_eq!(defs.len(), 1);
    }

    #[test]
    fn test_attributes_pre() {
        let src = r#"
            struct Attr {
                @attr1 field: u8,
            }
        "#;
        parse_helper(src);
    }

    #[test]
    fn test_bitfield() {
        let src = r#"
            struct BF {
                f: b<3>,
            }
        "#;
        parse_helper(src);
    }

    #[test]
    fn test_array_expr() {
        let src = r#"
            struct Arr {
                len: u8,
                data: [u8; len * 2],
                complex: [u8; (len * 2) - 4],
            }
        "#;
        parse_helper(src);
    }

    #[test]
    fn test_union_simple() {
        let src = r#"
            struct U {
                t: u8,
                body: union(t) {
                    1 => A { x: u8 },
                }
            }
        "#;
        parse_helper(src);
    }

    #[test]
    fn test_conditional_if() {
        let src = r#"
            struct Cond {
                x: u8,
                if (x == 1) {
                    y: u8,
                }
            }
        "#;
        parse_helper(src);
    }

    #[test]
    fn test_tcp_flags() {
        let src = r#"
            struct TcpFlags {
                data_offset: b<4>,
                reserved: b<3>,
                window_size: b<16>,
            }
        "#;
        parse_helper(src);
    }

    #[test]
    fn test_expr_member_access() {
        let src = r#"
            struct Member {
                a: u8,
                b: [u8; a.len],
            }
        "#;
        parse_helper(src);
    }

    #[test]
    fn test_expr_call() {
        let src = r#"
            struct Call {
                @transform(fn("dec")) data: [u8; 10],
            }
        "#;
        parse_helper(src);
    }

    #[test]
    fn test_type_inference_bitfield() {
        let src = r#"
            struct Infer {
                res = b011,
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => match &s.items[0] {
                ast::StructItem::Field(f) => {
                    assert_eq!(
                        f.value,
                        ast::FieldValue::Constraint(ast::Expr::Literal(ast::Literal::Int(
                            ast::IntLiteral {
                                value: 3,
                                width: 3,
                                ty: ast::IntType::Binary
                            }
                        )))
                    );
                }
                _ => panic!("Expected field"),
            },
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_type_inference_int_simple() {
        let src = r#"
            struct Infer {
                res = 0,
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => {
                match &s.items[0] {
                    ast::StructItem::Field(f) => {
                        // 0 fits in u8
                        assert_eq!(
                            f.value,
                            ast::FieldValue::Constraint(ast::Expr::Literal(ast::Literal::Int(
                                ast::IntLiteral {
                                    value: 0,
                                    width: 1,
                                    ty: ast::IntType::Decimal
                                }
                            )))
                        );
                    }
                    _ => panic!("Expected field"),
                }
            }
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_binary_parser_only() {
        let src = "struct A { f = b00, }";
        parse_helper(src);
    }

    #[test]
    #[should_panic]
    fn test_bitfield_limit() {
        let src = r#"
            struct Fail {
                f: b<129>,
            }
        "#;
        parse_helper(src);
    }

    #[test]
    fn test_type_inference_int() {
        let src = r#"
            struct InferInt {
                v = 10,
                big = 65536,
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => {
                // v = 10 -> u8
                match &s.items[0] {
                    ast::StructItem::Field(f) => {
                        assert_eq!(
                            f.value,
                            ast::FieldValue::Constraint(ast::Expr::Literal(ast::Literal::Int(
                                ast::IntLiteral {
                                    value: 10,
                                    width: 2,
                                    ty: ast::IntType::Decimal
                                }
                            )))
                        );
                    }
                    _ => panic!("Expected field v"),
                }
                // big = 65536 -> u32 (since > u16::MAX 65535)
                match &s.items[1] {
                    ast::StructItem::Field(f) => {
                        assert_eq!(
                            f.value,
                            ast::FieldValue::Constraint(ast::Expr::Literal(ast::Literal::Int(
                                ast::IntLiteral {
                                    value: 65536,
                                    width: 5,
                                    ty: ast::IntType::Decimal
                                }
                            )))
                        );
                    }
                    _ => panic!("Expected field big"),
                }
            }
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_union_multiple_matchers() {
        let src = r#"
            struct IcmpPacket {
                icmp_type: u8,
                body: union(icmp_type) {
                    0 | 8 => Echo { id: u16, seq: u16 },
                    3 => DestUnreach { unused: u32 },
                    _ => Raw { },
                }
            }
        "#;
        parse_helper(src);
    }

    #[test]
    fn test_tuple_union() {
        let src = r#"
            struct Version {
                major: u8,
                minor: u8,
                data: union(major, minor) {
                    (1, 0) => V1_0 { },
                    (1, 1) => V1_1 { },
                    (2, _) => V2Any { },
                    _ => Unknown { },
                }
            }
        "#;
        parse_helper(src);
    }

    #[test]
    fn test_struct_level_attribute() {
        let src = r#"
            @len(total_len)
            @endian(big)
            struct ScopedPacket {
                total_len: u16,
                payload: [u8],
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => {
                assert_eq!(s.attributes.len(), 2);
                assert_eq!(s.attributes[0].name, "len");
                assert_eq!(s.attributes[0].args.len(), 1);
                assert_eq!(s.attributes[0].args[0], ast::Expr::Path(vec!["total_len"]));
                assert_eq!(s.attributes[1].name, "endian");
                assert_eq!(s.attributes[1].args.len(), 1);
                assert_eq!(s.attributes[1].args[0], ast::Expr::Path(vec!["big"]));
            }
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_error_block() {
        let src = r#"
            error {
                MISSING_FLAG { found: u8, expected: u8 },
                INVALID_VERSION { val: u8 },
                CHECKSUM_MISMATCH,
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Error(variants) => {
                assert_eq!(variants.len(), 3);
                assert_eq!(variants[0].name, "MISSING_FLAG");
                assert_eq!(variants[0].fields.len(), 2);
                assert_eq!(variants[2].name, "CHECKSUM_MISMATCH");
                assert_eq!(variants[2].fields.len(), 0);
            }
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn test_field_level_len() {
        let src = r#"
            struct Container {
                len: u16,
                @len(len) inner: InnerPacket,
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => match &s.items[1] {
                ast::StructItem::Field(f) => {
                    assert_eq!(f.attributes.len(), 1);
                    assert_eq!(f.attributes[0].name, "len");
                    assert_eq!(f.attributes[0].args.len(), 1);
                    assert_eq!(f.attributes[0].args[0], ast::Expr::Path(vec!["len"]));
                }
                _ => panic!("Expected field"),
            },
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_greedy_attribute() {
        let src = r#"
            struct Tlv {
                tag: u8,
                len: u16,
                value: [u8; len],
                @greedy(unsafe_eof) trailer: [u8],
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => match &s.items[3] {
                ast::StructItem::Field(f) => {
                    assert_eq!(f.attributes.len(), 1);
                    assert_eq!(f.attributes[0].name, "greedy");
                    assert_eq!(f.attributes[0].args.len(), 1);
                    assert_eq!(f.attributes[0].args[0], ast::Expr::Path(vec!["unsafe_eof"]));
                }
                _ => panic!("Expected field"),
            },
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_until_attribute() {
        let src = r#"
        struct CString {
                @until(x00) content: [u8],
            }
        "#;

        let defs = parse_helper(src);
        println!("{:?}", defs);
        match &defs[0] {
            ast::Definition::Struct(s) => match &s.items[0] {
                ast::StructItem::Field(f) => {
                    assert_eq!(f.attributes[0].name, "until");
                    assert_eq!(f.attributes[0].args.len(), 1);
                    assert_eq!(
                        f.attributes[0].args[0],
                        ast::Expr::Literal(ast::Literal::Int(ast::IntLiteral {
                            value: 0,
                            width: 2,
                            ty: ast::IntType::Hex
                        }))
                    );
                }
                _ => panic!("Expected field"),
            },
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_concat_type() {
        let src = r#"
            struct Fragmented {
                field: concat(
                    chunk_a: b<4>,
                    @skip reserved: b<4>,
                    chunk_b: b<8>
                ),
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => match &s.items[0] {
                ast::StructItem::Field(f) => match &f.value {
                    ast::FieldValue::Type(ast::Type::Concat(fields)) => {
                        assert_eq!(fields.len(), 3);
                        assert_eq!(fields[1].attributes[0].name, "skip");
                    }
                    _ => panic!("Expected concat type"),
                },
                _ => panic!("Expected field"),
            },
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_opaque_attribute() {
        let src = r#"
            struct Container {
                len: u16,
                @opaque inner: InnerPacket,
            }
        "#;
        parse_helper(src);
    }

    #[test]
    fn test_parse_with_hook() {
        let src = r#"
            struct VarIntPacket {
                @parse_with(fn("crate::varint::parse"), u64) length: [u8],
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => match &s.items[0] {
                ast::StructItem::Field(f) => {
                    assert_eq!(f.attributes[0].name, "parse_with");
                    assert_eq!(f.attributes[0].args.len(), 2);
                }
                _ => panic!("Expected field"),
            },
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_max_iter_attribute() {
        let src = r#"
            struct Table {
                count: u16,
                @max_iter(1024) records: [Record; count],
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => match &s.items[1] {
                ast::StructItem::Field(f) => {
                    assert_eq!(f.attributes[0].name, "max_iter");
                }
                _ => panic!("Expected field"),
            },
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_endian_attributes() {
        let src = r#"
            @endian(big)
            struct EndianExample {
                val_be: u32,
                @endian(little) val_le: u32,
                @bit_order(lsb) lsb_flags: b<8>,
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => {
                assert_eq!(s.attributes[0].name, "endian");
                match &s.items[1] {
                    ast::StructItem::Field(f) => {
                        assert_eq!(f.attributes[0].name, "endian");
                        assert_eq!(f.attributes[0].args.len(), 1);
                        assert_eq!(f.attributes[0].args[0], ast::Expr::Path(vec!["little"]));
                    }
                    _ => panic!("Expected field"),
                }

                match &s.items[2] {
                    ast::StructItem::Field(f) => {
                        assert_eq!(f.attributes[0].name, "bit_order");
                        assert_eq!(f.attributes[0].args.len(), 1);
                        assert_eq!(f.attributes[0].args[0], ast::Expr::Path(vec!["lsb"]));
                    }
                    _ => panic!("Expected field"),
                }
            }
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_check_attribute() {
        let src = r#"
            struct Outer {
                inner_len: u16,
                @check(inner_len == inner.total_length) inner: Inner,
            }
        "#;
        parse_helper(src);
    }

    #[test]
    fn test_comments() {
        let src = r#"
            // This is a line comment
            struct WithComments {
                /* Block comment */ field: u8,
                // Another comment
                other: u16, /* trailing */
            }
        "#;
        parse_helper(src);
    }

    #[test]
    fn test_wildcard_matcher() {
        let src = r#"
            struct WithWildcard {
                t: u8,
                data: union(t) {
                    1 => One { },
                    _ => Other { },
                }
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => {
                match &s.items[1] {
                    ast::StructItem::Field(f) => {
                        match &f.value {
                            ast::FieldValue::Type(ast::Type::Union(u)) => {
                                // The wildcard '_' is parsed as an identifier
                                assert_eq!(u.variants.len(), 2);
                            }
                            _ => panic!("Expected union"),
                        }
                    }
                    _ => panic!("Expected field"),
                }
            }
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_union_error_fallback() {
        let src = r#"
            struct Packet {
                packet_type: b<3>,
                variant: union(packet_type) {
                    b010 => Something { data: u8 },
                    _ => @error(MISSING_FLAG { found: packet_type, expected: b010 })
                }
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => {
                match &s.items[1] {
                    ast::StructItem::Field(f) => {
                        match &f.value {
                            ast::FieldValue::Type(ast::Type::Union(u)) => {
                                assert_eq!(u.variants.len(), 2);
                                // Check the error variant
                                match &u.variants[1].body {
                                    ast::UnionBody::Error(name, fields) => {
                                        assert_eq!(name, &"MISSING_FLAG");
                                        assert_eq!(fields.len(), 2);
                                        assert_eq!(fields[0].0, "found");
                                        assert_eq!(fields[1].0, "expected");
                                    }
                                    _ => panic!("Expected error body"),
                                }
                            }
                            _ => panic!("Expected union"),
                        }
                    }
                    _ => panic!("Expected field"),
                }
            }
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_no_cache_attribute() {
        let src = r#"
            struct Tlv {
                len: u16,
                value: [u8; len],
                @no_cache @greedy(unsafe_eof) trailer: [u8],
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => match &s.items[2] {
                ast::StructItem::Field(f) => {
                    assert_eq!(f.attributes.len(), 2);
                    assert_eq!(f.attributes[0].name, "no_cache");
                    assert_eq!(f.attributes[1].name, "greedy");
                }
                _ => panic!("Expected field"),
            },
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_transform_attribute() {
        let src = r#"
            struct SecureData {
                @transform(fn("crate::aes_decrypt")) iv: [u8; 16],
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => match &s.items[0] {
                ast::StructItem::Field(f) => {
                    assert_eq!(f.attributes[0].name, "transform");
                    assert_eq!(f.attributes[0].args.len(), 1);
                }
                _ => panic!("Expected field"),
            },
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_align_attribute() {
        let src = r#"
            struct AlignExample {
                flags: b<3>,
                @skip pad: b<5>,
                @align(1) aligned_val: u8,
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => {
                assert_eq!(s.items.len(), 3);
                match &s.items[1] {
                    ast::StructItem::Field(f) => {
                        assert_eq!(f.attributes[0].name, "skip");
                    }
                    _ => panic!("Expected field"),
                }
                match &s.items[2] {
                    ast::StructItem::Field(f) => {
                        assert_eq!(f.attributes[0].name, "align");
                    }
                    _ => panic!("Expected field"),
                }
            }
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_path_parsing() {
        let src = r#"
            struct PathExample {
                @path(inner.flags) flags: b<3>,
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => match &s.items[0] {
                ast::StructItem::Field(f) => {
                    assert_eq!(f.attributes[0].name, "path");
                    assert_eq!(f.attributes[0].args.len(), 1);
                    assert_eq!(
                        f.attributes[0].args[0],
                        ast::Expr::Path(vec!["inner", "flags"])
                    );
                }
                _ => panic!("Expected field"),
            },
            _ => panic!("Expected struct"),
        }
    }
}
