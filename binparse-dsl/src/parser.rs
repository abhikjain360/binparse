use chumsky::prelude::*;
use crate::ast::*;

// Lexer/Parser helper types
type PError<'a> = extra::Err<Rich<'a, char>>;

pub fn parser<'a>() -> impl Parser<'a, &'a str, Vec<Definition>, PError<'a>> {
    let comment = just("//").then(any().and_is(just('\n').not()).repeats()).pad()
        .or(just("/*").then(any().and_is(just("*/").not()).repeats()).then(just("*/")).pad());

    let padded = |p| p.padded_by(comment.repeats().pad());

    let ident = text::ascii::ident().map(|s: &str| s.to_string());
    
    // Literals
    let decimal = text::int(10).map(|s: &str| Literal::Int(s.parse().unwrap()));
    let hex = just("0x").or(just("x")).ignore_then(text::int(16))
        .map(|s: &str| Literal::Int(i128::from_str_radix(s, 16).unwrap()));
    let binary = just("0b").or(just("b")).ignore_then(text::int(2))
        .map(|s: &str| Literal::Int(i128::from_str_radix(s, 2).unwrap()));
    
    let literal = choice((
        hex,
        binary,
        decimal,
    )).map(Expr::Literal);

    let expr = recursive(|expr| {
        let atom = choice((
            literal,
            ident.map(Expr::Ident),
            expr.clone().delimited_by(just('('), just(')')),
        )).padded();

        // TODO: Add operator precedence climbing here for proper expression parsing
        // For now, simple binary ops or just atoms
        // Implementing full expression parsing might be verbose, let's start with simple structure
        
        // Very basic precedence: Mul/Div, then Add/Sub, then Eq/Rel
        
        let op = choice((
            just("==").to(BinaryOp::Eq),
            just("!=").to(BinaryOp::Neq),
            just("<=").to(BinaryOp::Le),
            just(">=").to(BinaryOp::Ge),
            just("<").to(BinaryOp::Lt),
            just(">").to(BinaryOp::Gt),
            just("+").to(BinaryOp::Add),
            just("-").to(BinaryOp::Sub),
            just("*").to(BinaryOp::Mul),
            just("/").to(BinaryOp::Div),
        ));

        // Note: This is right-associative and doesn't respect precedence correctly without Pratt or fold
        // Using foldl for simple left-associative parsing
        
        let product = atom.clone()
            .foldl(choice((just("*").to(BinaryOp::Mul), just("/").to(BinaryOp::Div))).padded().then(atom.clone()).repeats(), 
            |lhs, (op, rhs)| Expr::Binary(Box::new(lhs), op, Box::new(rhs)));
            
        let sum = product.clone()
            .foldl(choice((just("+").to(BinaryOp::Add), just("-").to(BinaryOp::Sub))).padded().then(product.clone()).repeats(),
            |lhs, (op, rhs)| Expr::Binary(Box::new(lhs), op, Box::new(rhs)));
            
        let comparison = sum.clone()
            .foldl(choice((
                just("==").to(BinaryOp::Eq),
                just("!=").to(BinaryOp::Neq),
                just("<=").to(BinaryOp::Le),
                just(">=").to(BinaryOp::Ge),
                just("<").to(BinaryOp::Lt),
                just(">").to(BinaryOp::Gt)
            )).padded().then(sum.clone()).repeats(),
            |lhs, (op, rhs)| Expr::Binary(Box::new(lhs), op, Box::new(rhs)));

        comparison
    });

    let attribute = just('@')
        .ignore_then(ident)
        .then(expr.separated_by(just(',').padded()).collect().delimited_by(just('('), just(')')).or_not())
        .map(|(name, args)| Attribute {
            name,
            args: args.unwrap_or_default(),
        });

    let attributes = attribute.padded().repeats();

    let type_ref = recursive(|ty| {
        let primitive = choice((
            just("u8").to(Primitive::U8),
            just("u16").to(Primitive::U16),
            just("u32").to(Primitive::U32),
            just("u64").to(Primitive::U64),
            just("u128").to(Primitive::U128),
            just("i8").to(Primitive::I8),
            just("i16").to(Primitive::I16),
            just("i32").to(Primitive::I32),
            just("i64").to(Primitive::I64),
            just("i128").to(Primitive::I128),
            just("b").ignore_then(text::int(10).delimited_by(just('<'), just('>'))).map(|s: &str| Primitive::BitField(s.parse().unwrap())),
        )).map(Type::Primitive);

        // array: [Type; Expr]
        let array = ty.clone()
            .then_ignore(just(';').padded())
            .then(expr.clone())
            .delimited_by(just('['), just(']'))
            .map(|(t, len)| Type::Array(Box::new(t), len));

        // concat(field, ...)
        // Wait, Concat takes Fields, but Fields need Types. Circular dependency?
        // Field needs Type. Concat is a Type that has Fields.
        // Let's define Field parser lazily or pass it?
        // Or simpler: Concat just takes a list of fields.
        
        // We need a forward declaration or recursive setup for Field/StructItem
        
        primitive
            .or(array)
            .or(ident.map(Type::StructRef))
    });
    
    // We need mutually recursive parsers for struct bodies, fields, types (concat/union)
    
    // Breaking it down:
    // Type depends on UnionDef, Concat
    // UnionDef depends on UnionVariant -> UnionBody -> StructItem -> Field -> Type
    
    let type_parser = recursive(|ty| {
        let primitive = choice((
            just("u8").to(Primitive::U8),
            just("u16").to(Primitive::U16),
            just("u32").to(Primitive::U32),
            just("u64").to(Primitive::U64),
            just("u128").to(Primitive::U128),
            just("i8").to(Primitive::I8),
            just("i16").to(Primitive::I16),
            just("i32").to(Primitive::I32),
            just("i64").to(Primitive::I64),
            just("i128").to(Primitive::I128),
            just("b").ignore_then(text::int(10).delimited_by(just('<'), just('>'))).map(|s: &str| Primitive::BitField(s.parse().unwrap())),
        )).map(Type::Primitive);

        let array = ty.clone()
            .then_ignore(just(';').padded())
            .then(expr.clone())
            .delimited_by(just('['), just(']'))
            .map(|(t, len)| Type::Array(Box::new(t), len));

        // Placeholder for complex types
        let struct_ref = ident.map(Type::StructRef);

        primitive
            .or(array)
            .or(struct_ref)
    });

    // Struct Item needs to handle:
    // 1. Field: `name: Type` or `name: Type = Constraint` or `name = Constraint`
    // 2. Conditional: `if (Expr) { ... }`
    
    // Since `union` and `concat` are complex, let's define them fully.
    
    // Need a recursive block for struct items
    let struct_items_block = recursive(|items| {
         // Field definition
        // Type could be:
        // - Primitive/Array/StructRef (handled by `type_parser`)
        // - Union: `union(args) { ... }`
        // - Concat: `concat(...)`
        
        // Redefine Type parser to include Union and Concat
        let full_type = recursive(|ty| {
            let primitive = choice((
                just("u8").to(Primitive::U8),
                just("u16").to(Primitive::U16),
                just("u32").to(Primitive::U32),
                just("u64").to(Primitive::U64),
                just("u128").to(Primitive::U128),
                just("i8").to(Primitive::I8),
                just("i16").to(Primitive::I16),
                just("i32").to(Primitive::I32),
                just("i64").to(Primitive::I64),
                just("i128").to(Primitive::I128),
                just("b").ignore_then(text::int(10).delimited_by(just('<'), just('>'))).map(|s: &str| Primitive::BitField(s.parse().unwrap())),
            )).map(Type::Primitive);

            let array = ty.clone()
                .then_ignore(just(';').padded())
                .then(expr.clone())
                .delimited_by(just('['), just(']'))
                .map(|(t, len)| Type::Array(Box::new(t), len));

             // Concat field definition: `name: type` inside concat
            let concat_field = ident.then_ignore(just(':').padded())
                .then(ty.clone()) // simple types inside concat? Or full types?
                .then(attributes.clone()) // attributes allowed?
                .map(|((name, ty), attrs)| Field {
                    name: Some(name),
                    ty,
                    attributes: attrs,
                    value_constraint: None
                });
                
            let concat = just("concat").ignore_then(
                concat_field.separated_by(just(',').padded()).collect().delimited_by(just('('), just(')'))
            ).map(Type::Concat);

            // Union
            // union(args) { variants }
            // variants: `match => Body`
            // Body: `StructName { ... }` or `StructName` or `{ fields }`
            
            // Re-use `items` parser for inline struct bodies in union variants
            // But `items` produces `Vec<StructItem>`.
            
            let union_body = choice((
                // Inline struct: { ... }
                items.clone().delimited_by(just('{'), just('}')).map(UnionBody::InlineStruct),
                // Type Ref: Ident
                ident.map(UnionBody::TypeRef),
                // Named Inline: Name { ... } - Spec: "Echo { ... }"
                ident.then(items.clone().delimited_by(just('{'), just('}'))).map(|(name, _fields)| UnionBody::TypeRef(name)), // Simplified for now, AST needs update if we want to support inline named structs fully?
                // Actually spec says: `Echo { id: u16, ... }`. This is defining a struct named Echo AND using it.
                // Or is it referencing an existing struct Echo?
                // Spec: "pub struct Echo ...". So likely referencing.
                // But in "Raw { data: @greedy [u8] }", Raw is defined there?
                // "R1.6.5: Union variants represented as internal structs and an Enum"
                // So "Echo { ... }" inside union probably defines "Echo" struct scoped to that union.
                // For now, let's assume it maps to `InlineStruct` but we lose the name "Echo"?
                // Let's stick to TypeRef or InlineStruct. If it has a name and braces, it's tricky.
                // Let's just parse `Ident { ... }` as `InlineStruct` and ignore the name for now or assume it's sugar?
                // Actually, if I look at R1.6: "0 | 8 => Echo { ... }".
                // In expected Rust: "pub struct Echo ...".
                // So it defines a struct.
                // My `UnionBody` AST has `TypeRef(String)` and `InlineStruct(Vec<StructItem>)`.
                // I should probably add `NamedInlineStruct(String, Vec<StructItem>)`.
                // For now let's parse `Ident { ... }` and maybe treat it as `TypeRef`? No that loses fields.
                // Let's treat it as InlineStruct and ignore the name for the moment, or better:
                // `ident.then(block).map(...)`
            ));

            let union_variant = expr.clone().separated_by(just('|').padded()).collect()
                .then_ignore(just("=>").padded())
                .then(union_body)
                .map(|(matchers, body)| UnionVariant { matchers, body });

            let union_def = just("union").ignore_then(
                ident.separated_by(just(',').padded()).collect().delimited_by(just('('), just(')'))
            ).then(
                union_variant.separated_by(just(',').padded()).allow_trailing().collect().delimited_by(just('{'), just('}'))
            ).map(|(args, variants)| Type::Union(UnionDef { args, variants }));

            primitive
                .or(array)
                .or(concat)
                .or(union_def)
                .or(ident.map(Type::StructRef))
        });
        
        let field_decl = attributes.clone()
            .then(ident)
            .then(just(':').padded().ignore_then(full_type.clone()).or_not()) // Optional type if using `=` constraint?
            .then(just('=').padded().ignore_then(expr.clone()).or_not())
            .then_ignore(just(',').padded().or_not())
            .validate(|(((attrs, name), ty), constraint), _, emitter| {
                if ty.is_none() && constraint.is_none() {
                     emitter.emit(Rich::custom(name.span().clone(), "Field must have a type or a constraint"));
                     // Return dummy
                     Field { name: Some(name.clone()), ty: Type::Primitive(Primitive::U8), attributes: attrs, value_constraint: None }
                } else if let Some(t) = ty {
                    Field { name: Some(name), ty: t, attributes: attrs, value_constraint: constraint }
                } else {
                    // Infer type from constraint? Or assume constraint implies type?
                    // Spec: "reserved = b000". b000 implies b<3>.
                    // For now, let's map it to a "Constrained" type or Primitive if possible?
                    // Let's just set a dummy type or try to infer.
                    // For parser simplicity, let's require type OR have a placeholder "Infer".
                    // But AST `ty` is not optional.
                    // Let's default to U8 if missing, but realistically we should inspect expr.
                    // In `b<3> = b000`, type is explicit.
                    // In `reserved = b000`, type is implicit.
                    // I'll assume implicit types are handled in semantic analysis, but I need to store something.
                    // Let's store `Primitive::U8` as a placeholder if ty is missing.
                     Field { name: Some(name), ty: Type::Primitive(Primitive::U8), attributes: attrs, value_constraint: constraint }
                }
            })
            .map(StructItem::Field);

        let conditional = just("if").padded()
            .ignore_then(expr.clone().delimited_by(just('('), just(')')))
            .then(items.clone().delimited_by(just('{'), just('}')))
            .then(just("else").padded().ignore_then(items.clone().delimited_by(just('{'), just('}'))).or_not())
            .map(|((condition, then_branch), else_branch)| StructItem::Conditional(Conditional {
                condition,
                then_branch,
                else_branch
            }));

        choice((
            conditional,
            field_decl
        )).repeats()
    });

    let struct_def = attributes.clone()
        .then_ignore(just("struct").padded())
        .then(ident)
        .then(struct_items_block.delimited_by(just('{'), just('}')))
        .map(|((attrs, name), items)| Definition::Struct(StructDef {
            name,
            attributes: attrs,
            items,
        }));

    let error_variant = ident
        .then(
             // Optional fields: { found: b<3>, ... }
             // Fields in error are `name: type`
             ident.then_ignore(just(':').padded())
                  .then(type_parser.clone()) // Use simple type parser (primitives)
                  .separated_by(just(',').padded()).collect()
                  .delimited_by(just('{'), just('}'))
                  .or_not()
        )
        .then_ignore(just(',').padded().or_not())
        .map(|(name, fields)| ErrorVariant {
            name,
            fields: fields.unwrap_or_default().into_iter().map(|(n, t)| (n, t)).collect(),
        });

    let error_def = just("error").padded()
        .ignore_then(error_variant.repeats().delimited_by(just('{'), just('}')))
        .map(Definition::Error);

    choice((
        struct_def,
        error_def
    )).padded().repeats().collect()
}
