use ariadne::{Color, Label, Report, ReportKind, Source};
use winnow::{
    ModalResult, Parser,
    ascii::{digit1, hex_digit1, multispace0},
    combinator::*,
    error::{ContextError, ErrMode, StrContext},
    stream::LocatingSlice,
    token::{any, take_until, take_while},
};

use binparse_dsl as ast;

type Input<'a> = LocatingSlice<&'a str>;

fn line_comment(input: &mut Input<'_>) -> ModalResult<()> {
    ("//", take_while(0.., |c| c != '\n'), opt('\n'))
        .void()
        .parse_next(input)
}

fn block_comment(input: &mut Input<'_>) -> ModalResult<()> {
    ("/*", take_until(0.., "*/"), "*/").void().parse_next(input)
}

fn ws(input: &mut Input<'_>) -> ModalResult<()> {
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

fn padded<'a, O, F>(inner: F) -> impl Parser<Input<'a>, O, ErrMode<ContextError>>
where
    F: Parser<Input<'a>, O, ErrMode<ContextError>>,
{
    delimited(ws, inner, ws)
}

fn ident_raw<'a>(input: &mut Input<'a>) -> ModalResult<&'a str> {
    take_while(1.., |c: char| c.is_ascii_alphanumeric() || c == '_')
        .verify(|s: &str| {
            s.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        })
        .parse_next(input)
}

fn identifier<'a>(input: &mut Input<'a>) -> ModalResult<&'a str> {
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

fn path<'a>(input: &mut Input<'a>) -> ModalResult<Vec<&'a str>> {
    separated(1.., identifier, ".").parse_next(input)
}

fn literal<'a>(input: &mut Input<'a>) -> ModalResult<ast::Literal<'a>> {
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

fn decimal_literal<'a>(input: &mut Input<'a>) -> ModalResult<ast::Literal<'a>> {
    digit1
        .try_map(|s: &str| {
            let width = s.len().try_into()?;

            s.parse::<usize>()
                .map(|value| ast::IntLiteral {
                    value,
                    width,
                    ty: ast::IntType::Decimal,
                })
                .map_err(IntLiteralError::InvalidInt)
        })
        .map(ast::Literal::Int)
        .parse_next(input)
}

fn hex_literal<'a>(input: &mut Input<'a>) -> ModalResult<ast::Literal<'a>> {
    preceded("x", hex_digit1)
        .try_map(|s: &str| {
            let width = s.len().try_into()?;
            usize::from_str_radix(s, 16)
                .map(|value| ast::IntLiteral {
                    value,
                    width,
                    ty: ast::IntType::Hex,
                })
                .map_err(IntLiteralError::InvalidInt)
        })
        .map(ast::Literal::Int)
        .parse_next(input)
}

fn binary_literal<'a>(input: &mut Input<'a>) -> ModalResult<ast::Literal<'a>> {
    preceded("b", take_while(1.., |c| c == '0' || c == '1'))
        .try_map(|s: &str| {
            let width = s.len().try_into()?;
            usize::from_str_radix(s, 2)
                .map(|value| ast::IntLiteral {
                    value,
                    width,
                    ty: ast::IntType::Binary,
                })
                .map_err(IntLiteralError::InvalidInt)
        })
        .map(ast::Literal::Int)
        .parse_next(input)
}

fn string_literal<'a>(input: &mut Input<'a>) -> ModalResult<ast::Literal<'a>> {
    delimited('"', take_while(0.., |c| c != '"'), '"')
        .map(|s: &str| ast::Literal::String(s))
        .parse_next(input)
}

fn expr<'a>(input: &mut Input<'a>) -> ModalResult<ast::Expr<'a>> {
    logic_or(input)
}

fn args<'a>(input: &mut Input<'a>) -> ModalResult<Vec<ast::Expr<'a>>> {
    delimited(padded('('), separated(0.., expr, padded(',')), padded(')')).parse_next(input)
}

fn atom<'a>(input: &mut Input<'a>) -> ModalResult<ast::Expr<'a>> {
    padded(alt((
        literal.map(ast::Expr::Literal),
        call_or_path,
        tuple_or_group,
    )))
    .parse_next(input)
}

fn call_or_path<'a>(input: &mut Input<'a>) -> ModalResult<ast::Expr<'a>> {
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

fn tuple_or_group<'a>(input: &mut Input<'a>) -> ModalResult<ast::Expr<'a>> {
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

fn member_access<'a>(input: &mut Input<'a>) -> ModalResult<ast::Expr<'a>> {
    atom(input)
}

fn product<'a>(input: &mut Input<'a>) -> ModalResult<ast::Expr<'a>> {
    let mut lhs = member_access(input)?;
    loop {
        let start = *input;
        let op_res: ModalResult<ast::BinaryOp> = padded(alt((
            "*".map(|_| ast::BinaryOp::Numeric(ast::NumericBinaryOp::Mul)),
            "/".map(|_| ast::BinaryOp::Numeric(ast::NumericBinaryOp::Div)),
            "%".map(|_| ast::BinaryOp::Numeric(ast::NumericBinaryOp::Mod)),
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

fn sum<'a>(input: &mut Input<'a>) -> ModalResult<ast::Expr<'a>> {
    let mut lhs = product(input)?;
    loop {
        let start = *input;
        let op_res: ModalResult<ast::BinaryOp> = padded(alt((
            "+".map(|_| ast::BinaryOp::Numeric(ast::NumericBinaryOp::Add)),
            "-".map(|_| ast::BinaryOp::Numeric(ast::NumericBinaryOp::Sub)),
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

fn bitwise<'a>(input: &mut Input<'a>) -> ModalResult<ast::Expr<'a>> {
    let mut lhs = sum(input)?;
    loop {
        let start = *input;
        let op_res: ModalResult<ast::BinaryOp> = padded(alt((
            "&".verify(|s: &str| !s.starts_with("&&"))
                .map(|_| ast::BinaryOp::Numeric(ast::NumericBinaryOp::BitAnd)),
            "^".map(|_| ast::BinaryOp::Numeric(ast::NumericBinaryOp::BitXor)),
            "|".verify(|s: &str| !s.starts_with("||"))
                .map(|_| ast::BinaryOp::Numeric(ast::NumericBinaryOp::BitOr)),
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

fn comparison<'a>(input: &mut Input<'a>) -> ModalResult<ast::Expr<'a>> {
    let mut lhs = bitwise(input)?;
    loop {
        let start = *input;
        let op_res: ModalResult<ast::BinaryOp> = padded(alt((
            "==".map(|_| ast::BinaryOp::Bool(ast::BoolBinaryOp::Eq)),
            "!=".map(|_| ast::BinaryOp::Bool(ast::BoolBinaryOp::Neq)),
            "<=".map(|_| ast::BinaryOp::Bool(ast::BoolBinaryOp::Le)),
            ">=".map(|_| ast::BinaryOp::Bool(ast::BoolBinaryOp::Ge)),
            "<".map(|_| ast::BinaryOp::Bool(ast::BoolBinaryOp::Lt)),
            ">".map(|_| ast::BinaryOp::Bool(ast::BoolBinaryOp::Gt)),
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

fn logic_and<'a>(input: &mut Input<'a>) -> ModalResult<ast::Expr<'a>> {
    let mut lhs = comparison(input)?;
    loop {
        let start = *input;
        let op_res: ModalResult<ast::BinaryOp> =
            padded("&&".map(|_| ast::BinaryOp::Bool(ast::BoolBinaryOp::And))).parse_next(input);
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

fn logic_or<'a>(input: &mut Input<'a>) -> ModalResult<ast::Expr<'a>> {
    let mut lhs = logic_and(input)?;
    loop {
        let start = *input;
        let op_res: ModalResult<ast::BinaryOp> =
            padded("||".map(|_| ast::BinaryOp::Bool(ast::BoolBinaryOp::Or))).parse_next(input);
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

fn attribute<'a>(input: &mut Input<'a>) -> ModalResult<ast::Attribute<'a>> {
    seq! {ast::Attribute {
        _: '@',
        name: identifier,
        args: opt(args).map(|o: Option<Vec<ast::Expr>>| o.unwrap_or_default()),
    }}
    .parse_next(input)
}

fn attributes<'a>(input: &mut Input<'a>) -> ModalResult<Vec<ast::Attribute<'a>>> {
    repeat(0.., padded(attribute)).parse_next(input)
}

fn primitive(input: &mut Input<'_>) -> ModalResult<ast::Primitive> {
    dispatch! {peek(any);
        'u' => alt((
            "u8".map(|_| ast::Primitive::U8),
            "u16".map(|_| ast::Primitive::U16),
            "u32".map(|_| ast::Primitive::U32),
            "u64".map(|_| ast::Primitive::U64),
            "u128".map(|_| ast::Primitive::U128),
        )),
        _ => fail
    }
    .parse_next(input)
}

fn bit_field_type<'a>(input: &mut Input<'a>) -> ModalResult<ast::Type<'a>> {
    ("b", delimited('<', digit1, '>'))
        .try_map(|(_, w_str): (&str, &str)| w_str.parse::<u8>())
        .verify(|w| *w <= 8)
        .map(ast::Type::BitField)
        .parse_next(input)
}

fn type_parser<'a>(input: &mut Input<'a>) -> ModalResult<ast::Type<'a>> {
    alt((
        array_type,
        concat_type,
        union_type,
        primitive.map(ast::Type::Primitive),
        bit_field_type,
        padded(identifier).map(ast::Type::StructRef),
    ))
    .parse_next(input)
}

fn array_elem_type<'a>(input: &mut Input<'a>) -> ModalResult<ast::ArrayElemType<'a>> {
    alt((
        ("b", delimited('<', digit1, '>'))
            .try_map(|(_, w_str): (&str, &str)| w_str.parse::<u8>())
            .verify(|w| *w <= 8)
            .map(ast::ArrayElemType::BitField),
        primitive.map(ast::ArrayElemType::Primitive),
        padded(identifier).map(ast::ArrayElemType::StructRef),
    ))
    .parse_next(input)
}

fn array_size_atom<'a>(input: &mut Input<'a>) -> ModalResult<ast::ArraySize<'a>> {
    alt((
        path.map(ast::ArraySize::Path),
        decimal_literal.map(|lit| match lit {
            ast::Literal::Int(i) => ast::ArraySize::Int(i),
            _ => unreachable!(),
        }),
        hex_literal.map(|lit| match lit {
            ast::Literal::Int(i) => ast::ArraySize::Int(i),
            _ => unreachable!(),
        }),
        delimited(padded('('), array_size, padded(')')),
    ))
    .parse_next(input)
}

fn array_size_product<'a>(input: &mut Input<'a>) -> ModalResult<ast::ArraySize<'a>> {
    let first = array_size_atom.parse_next(input)?;
    repeat(
        0..,
        (
            padded(alt((
                '*'.value(ast::NumericBinaryOp::Mul),
                '/'.value(ast::NumericBinaryOp::Div),
                '%'.value(ast::NumericBinaryOp::Mod),
            ))),
            array_size_atom,
        ),
    )
    .fold(
        move || first.clone(),
        |lhs, (op, rhs)| ast::ArraySize::Binary(Box::new(ast::ArraySizeBinary { lhs, op, rhs })),
    )
    .parse_next(input)
}

fn array_size_sum<'a>(input: &mut Input<'a>) -> ModalResult<ast::ArraySize<'a>> {
    let first = array_size_product.parse_next(input)?;
    repeat(
        0..,
        (
            padded(alt((
                '+'.value(ast::NumericBinaryOp::Add),
                '-'.value(ast::NumericBinaryOp::Sub),
            ))),
            array_size_product,
        ),
    )
    .fold(
        move || first.clone(),
        |lhs, (op, rhs)| ast::ArraySize::Binary(Box::new(ast::ArraySizeBinary { lhs, op, rhs })),
    )
    .parse_next(input)
}

fn array_size_bitwise<'a>(input: &mut Input<'a>) -> ModalResult<ast::ArraySize<'a>> {
    let first = array_size_sum.parse_next(input)?;
    repeat(
        0..,
        (
            padded(alt((
                '&'.value(ast::NumericBinaryOp::BitAnd),
                '|'.value(ast::NumericBinaryOp::BitOr),
                '^'.value(ast::NumericBinaryOp::BitXor),
            ))),
            array_size_sum,
        ),
    )
    .fold(
        move || first.clone(),
        |lhs, (op, rhs)| ast::ArraySize::Binary(Box::new(ast::ArraySizeBinary { lhs, op, rhs })),
    )
    .parse_next(input)
}

fn array_size<'a>(input: &mut Input<'a>) -> ModalResult<ast::ArraySize<'a>> {
    array_size_bitwise.parse_next(input)
}

fn array_type<'a>(input: &mut Input<'a>) -> ModalResult<ast::Type<'a>> {
    delimited(
        padded('['),
        (
            array_elem_type,
            opt(preceded(padded(';'), array_size)).map(|s| s.unwrap_or(ast::ArraySize::Dynamic)),
        ),
        padded(']'),
    )
    .map(|(elem_ty, size)| ast::Type::Array(ast::ArrayType { elem_ty, size }))
    .parse_next(input)
}

fn field_value<'a>(input: &mut Input<'a>) -> ModalResult<ast::FieldValue<'a>> {
    alt((
        preceded(padded(':'), type_parser).map(ast::FieldValue::Type),
        preceded(padded('='), expr).map(ast::FieldValue::Constraint),
    ))
    .parse_next(input)
}

fn concat_item<'a>(input: &mut Input<'a>) -> ModalResult<ast::ConcatItem<'a>> {
    seq! {ast::ConcatItem {
        attributes: attributes,
        ty: type_parser,
    }}
    .parse_next(input)
}

fn concat_type<'a>(input: &mut Input<'a>) -> ModalResult<ast::Type<'a>> {
    let _ = padded("concat").parse_next(input)?;
    delimited(
        padded('('),
        separated(0.., concat_item, padded(',')),
        padded(')').context(StrContext::Label("')' or type")),
    )
    .map(ast::Type::Concat)
    .parse_next(input)
    .map_err(|e| e.cut())
}

fn error_body<'a>(input: &mut Input<'a>) -> ModalResult<ast::UnionBody<'a>> {
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

fn union_body<'a>(input: &mut Input<'a>) -> ModalResult<ast::UnionBody<'a>> {
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

fn union_matcher_simple<'a>(input: &mut Input<'a>) -> ModalResult<ast::UnionMatcher<'a>> {
    padded(alt((
        "_".map(|_| ast::UnionMatcher::Wildcard),
        literal.map(ast::UnionMatcher::Literal),
    )))
    .parse_next(input)
}

fn union_matcher<'a>(input: &mut Input<'a>) -> ModalResult<ast::UnionMatcher<'a>> {
    padded(alt((
        delimited(
            padded('('),
            separated(1.., union_matcher_simple, padded(',')),
            padded(')'),
        )
        .map(ast::UnionMatcher::Tuple),
        union_matcher_simple,
    )))
    .parse_next(input)
}

fn union_variant<'a>(input: &mut Input<'a>) -> ModalResult<ast::UnionVariant<'a>> {
    seq! {ast::UnionVariant {
        matchers: separated(1.., union_matcher, padded('|')),
        _: padded("=>"),
        body: union_body,
    }}
    .parse_next(input)
}

fn union_type<'a>(input: &mut Input<'a>) -> ModalResult<ast::Type<'a>> {
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

fn union_variants<'a>(input: &mut Input<'a>) -> ModalResult<Vec<ast::UnionVariant<'a>>> {
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

fn struct_item<'a>(input: &mut Input<'a>) -> ModalResult<ast::StructItem<'a>> {
    alt((
        conditional.map(ast::StructItem::Conditional),
        field_with_opt_comma.map(ast::StructItem::Field),
    ))
    .parse_next(input)
}

fn struct_items<'a>(input: &mut Input<'a>) -> ModalResult<Vec<ast::StructItem<'a>>> {
    repeat(0.., struct_item).parse_next(input)
}

fn conditional<'a>(input: &mut Input<'a>) -> ModalResult<ast::Conditional<'a>> {
    seq! {ast::Conditional {
        _: padded("if"),
        condition: delimited(padded('('), expr, padded(')')),
        then_branch: delimited(padded('{'), struct_items, padded('}')),
        else_branch: opt(preceded(padded("else"), delimited(padded('{'), struct_items, padded('}')))),
    }}.parse_next(input)
}

fn field<'a>(input: &mut Input<'a>) -> ModalResult<ast::Field<'a>> {
    seq! {ast::Field {
        attributes: attributes,
        name: padded(identifier),
        value: field_value,
    }}
    .parse_next(input)
}

fn field_with_opt_comma<'a>(input: &mut Input<'a>) -> ModalResult<ast::Field<'a>> {
    terminated(field, opt(padded(','))).parse_next(input)
}

fn struct_def<'a>(input: &mut Input<'a>) -> ModalResult<ast::Definition<'a>> {
    let attrs = attributes.parse_next(input)?;
    let _ = padded("struct").parse_next(input)?;

    let (name, items) = (
        padded(identifier).context(StrContext::Label("struct name")),
        delimited(
            padded('{').context(StrContext::Label("'{'")),
            struct_items,
            padded('}').context(StrContext::Label("'}' or field")),
        ),
    )
        .parse_next(input)
        .map_err(|e| e.cut())?;

    Ok(ast::Definition::Struct(ast::Struct {
        attributes: attrs,
        name,
        items,
    }))
}

fn error_variant<'a>(input: &mut Input<'a>) -> ModalResult<ast::ErrorVariant<'a>> {
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

fn error_def<'a>(input: &mut Input<'a>) -> ModalResult<ast::Definition<'a>> {
    preceded(
        padded("error"),
        delimited(padded('{'), repeat(0.., padded(error_variant)), padded('}')),
    )
    .map(ast::Definition::Error)
    .parse_next(input)
}

fn definition<'a>(input: &mut Input<'a>) -> ModalResult<ast::Definition<'a>> {
    alt((struct_def, error_def)).parse_next(input)
}

/// Parse a BinParse DSL source string into a list of definitions.
fn parse<'a>(input: &mut Input<'a>) -> ModalResult<Vec<ast::Definition<'a>>> {
    let (defs, _) = (
        repeat(0.., padded(definition)),
        (ws, winnow::combinator::eof),
    )
        .parse_next(input)?;
    Ok(defs)
}

fn report_error(src: &str, offset: usize, msg: String) -> String {
    let mut output = Vec::new();
    let report = Report::build(ReportKind::Error, ((), offset..offset))
        .with_message("Parse error")
        .with_label(
            Label::new(((), offset..offset))
                .with_message(msg)
                .with_color(Color::Red),
        )
        .finish();
    let _ = report.write(Source::from(src), &mut output);
    String::from_utf8_lossy(&output).into_owned()
}

/// Convenience function that takes an owned string and returns Result.
pub fn parse_str(src: &str) -> Result<Vec<ast::Definition<'_>>, String> {
    let input = LocatingSlice::new(src);
    parse.parse(input).map_err(|e| {
        let offset = e.offset();
        let inner = e.inner();
        let err_msg = if inner.to_string().is_empty() {
            "parse error".to_string()
        } else {
            inner.to_string()
        };
        report_error(src, offset, err_msg)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_helper(src: &str) -> Vec<ast::Definition<'_>> {
        match parse.parse(LocatingSlice::new(src)) {
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
                window_size: u16,
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
                f: b<9>,
            }
        "#;
        parse_helper(src);
    }

    #[test]
    fn test_bitfield_valid_limit() {
        let src = r#"
            struct Fail {
                f: b<8>,
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
                field: concat(b<4>, @skip b<4>, b<8>),
            }
        "#;
        let defs = parse_helper(src);
        match &defs[0] {
            ast::Definition::Struct(s) => match &s.items[0] {
                ast::StructItem::Field(f) => match &f.value {
                    ast::FieldValue::Type(ast::Type::Concat(items)) => {
                        assert_eq!(items.len(), 3);
                        assert_eq!(items[0].ty, ast::Type::BitField(4));
                        assert_eq!(items[0].attributes.len(), 0);
                        assert_eq!(items[1].ty, ast::Type::BitField(4));
                        assert_eq!(items[1].attributes.len(), 1);
                        assert_eq!(items[1].attributes[0].name, "skip");
                        assert_eq!(items[2].ty, ast::Type::BitField(8));
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
