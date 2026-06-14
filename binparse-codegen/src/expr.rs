use binparse_dsl as ast;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::struct_::{DoneField, DoneFieldType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprType {
    Numeric,
    Bool,
}

impl std::fmt::Display for ExprType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExprType::Numeric => write!(f, "number"),
            ExprType::Bool => write!(f, "boolean"),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LoweredExpr {
    pub(crate) tokens: TokenStream,
    pub(crate) const_value: Option<usize>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("expression '{expr}' references field '{field}' which is unknown or not yet parsed")]
    UnknownField { expr: String, field: String },
    #[error("expression '{expr}' references field '{field}' which is not a numeric field")]
    NonNumericField { expr: String, field: String },
    #[error("expression '{expr}' references conditional field '{field}' which may be absent")]
    ConditionalField { expr: String, field: String },
    #[error("expression '{expr}' uses nested path '{path}' which is not supported")]
    NestedPath { expr: String, path: String },
    #[error("expression '{expr}' is a {got} but a {expected} is required")]
    TypeMismatch {
        expr: String,
        expected: ExprType,
        got: String,
    },
    #[error("constant expression '{expr}' overflows")]
    ConstOverflow { expr: String },
    #[error("expression '{expr}' divides by zero")]
    DivisionByZero { expr: String },
    #[error("expression '{expr}' divides by a non-constant value")]
    NonConstDivisor { expr: String },
    #[error("expression '{expr}' uses a macro call which is not supported")]
    MacroCall { expr: String },
}

pub(crate) fn lower(
    expr: &ast::Expr<'_>,
    expected: ExprType,
    done_fields: &[DoneField],
) -> Result<LoweredExpr, Error> {
    lower_inner(expr, expected, done_fields, expr)
}

pub(crate) fn lower_discriminants(
    args: &[&str],
    done_fields: &[DoneField],
) -> Result<TokenStream, Error> {
    let lowered = args
        .iter()
        .map(|arg| {
            let expr = ast::Expr::Path(vec![arg]);
            lower(&expr, ExprType::Numeric, done_fields).map(|lowered| lowered.tokens)
        })
        .collect::<Result<Vec<_>, _>>()?;

    if let [single] = lowered.as_slice() {
        Ok(quote! { #single })
    } else {
        Ok(quote! { (#(#lowered),*) })
    }
}

fn lower_inner(
    expr: &ast::Expr<'_>,
    expected: ExprType,
    done_fields: &[DoneField],
    root: &ast::Expr<'_>,
) -> Result<LoweredExpr, Error> {
    match expr {
        ast::Expr::Literal(ast::Literal::Int(ast::IntLiteral { value, .. })) => {
            require(ExprType::Numeric, expected, root)?;
            let v = *value;
            Ok(LoweredExpr {
                tokens: quote! { #v },
                const_value: Some(v),
            })
        }

        ast::Expr::Literal(ast::Literal::String(_)) => Err(Error::TypeMismatch {
            expr: render(root),
            expected,
            got: "string".to_string(),
        }),

        ast::Expr::Path(path) => {
            require(ExprType::Numeric, expected, root)?;
            let [field_name] = path.as_slice() else {
                return Err(Error::NestedPath {
                    expr: render(root),
                    path: path.join("."),
                });
            };
            let done_field = done_fields
                .iter()
                .find(|f| f.name == *field_name)
                .ok_or_else(|| Error::UnknownField {
                    expr: render(root),
                    field: field_name.to_string(),
                })?;
            if done_field.conditional {
                return Err(Error::ConditionalField {
                    expr: render(root),
                    field: field_name.to_string(),
                });
            }
            match done_field.field_type {
                DoneFieldType::Primitive | DoneFieldType::BitField => {
                    let getter = format_ident!("{}", field_name);
                    Ok(LoweredExpr {
                        tokens: quote! { self.#getter() as usize },
                        const_value: None,
                    })
                }
                DoneFieldType::Hook => {
                    let getter = format_ident!("{}", field_name);
                    Ok(LoweredExpr {
                        tokens: quote! { self.#getter().map(|v| v as usize).unwrap_or(0) },
                        const_value: None,
                    })
                }
                DoneFieldType::Other => Err(Error::NonNumericField {
                    expr: render(root),
                    field: field_name.to_string(),
                }),
            }
        }

        ast::Expr::Binary(binary) => match binary.op {
            ast::BinaryOp::Numeric(op) => {
                require(ExprType::Numeric, expected, root)?;
                let lhs = lower_inner(&binary.lhs, ExprType::Numeric, done_fields, root)?;
                let rhs = lower_inner(&binary.rhs, ExprType::Numeric, done_fields, root)?;
                lower_numeric_binop(op, lhs, rhs, root)
            }
            ast::BinaryOp::Bool(op) => {
                require(ExprType::Bool, expected, root)?;
                let operand_ty = match op {
                    ast::BoolBinaryOp::And | ast::BoolBinaryOp::Or => ExprType::Bool,
                    _ => ExprType::Numeric,
                };
                let lhs = lower_inner(&binary.lhs, operand_ty, done_fields, root)?;
                let rhs = lower_inner(&binary.rhs, operand_ty, done_fields, root)?;
                let lhs_tokens = lhs.tokens;
                let rhs_tokens = rhs.tokens;
                let op_tokens = match op {
                    ast::BoolBinaryOp::Eq => quote! { == },
                    ast::BoolBinaryOp::Neq => quote! { != },
                    ast::BoolBinaryOp::Lt => quote! { < },
                    ast::BoolBinaryOp::Gt => quote! { > },
                    ast::BoolBinaryOp::Le => quote! { <= },
                    ast::BoolBinaryOp::Ge => quote! { >= },
                    ast::BoolBinaryOp::And => quote! { && },
                    ast::BoolBinaryOp::Or => quote! { || },
                };
                Ok(LoweredExpr {
                    tokens: quote! { ((#lhs_tokens) #op_tokens (#rhs_tokens)) },
                    const_value: None,
                })
            }
        },

        ast::Expr::Tuple(_) => Err(Error::TypeMismatch {
            expr: render(root),
            expected,
            got: "tuple".to_string(),
        }),

        ast::Expr::Call(..) => Err(Error::MacroCall { expr: render(root) }),
    }
}

fn lower_numeric_binop(
    op: ast::NumericBinaryOp,
    lhs: LoweredExpr,
    rhs: LoweredExpr,
    root: &ast::Expr<'_>,
) -> Result<LoweredExpr, Error> {
    use ast::NumericBinaryOp::*;

    if let (Some(l), Some(r)) = (lhs.const_value, rhs.const_value) {
        let folded = match op {
            Add => l.checked_add(r),
            Sub => l.checked_sub(r),
            Mul => l.checked_mul(r),
            Div => l.checked_div(r),
            Mod => l.checked_rem(r),
            BitAnd => Some(l & r),
            BitOr => Some(l | r),
            BitXor => Some(l ^ r),
        };
        let value = folded.ok_or_else(|| match (op, r) {
            (Div | Mod, 0) => Error::DivisionByZero { expr: render(root) },
            _ => Error::ConstOverflow { expr: render(root) },
        })?;
        return Ok(LoweredExpr {
            tokens: quote! { #value },
            const_value: Some(value),
        });
    }

    let lhs_tokens = lhs.tokens;
    let rhs_tokens = rhs.tokens;
    let tokens = match op {
        Add => quote! { (#lhs_tokens).saturating_add(#rhs_tokens) },
        Sub => quote! { (#lhs_tokens).saturating_sub(#rhs_tokens) },
        Mul => quote! { (#lhs_tokens).saturating_mul(#rhs_tokens) },
        Div | Mod => {
            let op_tokens = if matches!(op, Div) {
                quote! { / }
            } else {
                quote! { % }
            };
            match rhs.const_value {
                None => return Err(Error::NonConstDivisor { expr: render(root) }),
                Some(0) => return Err(Error::DivisionByZero { expr: render(root) }),
                Some(_) => quote! { ((#lhs_tokens) #op_tokens (#rhs_tokens)) },
            }
        }
        BitAnd => quote! { ((#lhs_tokens) & (#rhs_tokens)) },
        BitOr => quote! { ((#lhs_tokens) | (#rhs_tokens)) },
        BitXor => quote! { ((#lhs_tokens) ^ (#rhs_tokens)) },
    };

    Ok(LoweredExpr {
        tokens,
        const_value: None,
    })
}

fn require(actual: ExprType, expected: ExprType, root: &ast::Expr<'_>) -> Result<(), Error> {
    if actual == expected {
        Ok(())
    } else {
        Err(Error::TypeMismatch {
            expr: render(root),
            expected,
            got: actual.to_string(),
        })
    }
}

fn render(expr: &ast::Expr<'_>) -> String {
    match expr {
        ast::Expr::Literal(ast::Literal::Int(lit)) => match lit.ty {
            ast::IntType::Decimal => format!("{}", lit.value),
            ast::IntType::Hex => format!("x{:x}", lit.value),
            ast::IntType::Binary => format!("b{:b}", lit.value),
        },
        ast::Expr::Literal(ast::Literal::String(s)) => format!("\"{s}\""),
        ast::Expr::Path(path) => path.join("."),
        ast::Expr::Binary(binary) => {
            let op = match binary.op {
                ast::BinaryOp::Numeric(op) => match op {
                    ast::NumericBinaryOp::Add => "+",
                    ast::NumericBinaryOp::Sub => "-",
                    ast::NumericBinaryOp::Mul => "*",
                    ast::NumericBinaryOp::Div => "/",
                    ast::NumericBinaryOp::Mod => "%",
                    ast::NumericBinaryOp::BitAnd => "&",
                    ast::NumericBinaryOp::BitOr => "|",
                    ast::NumericBinaryOp::BitXor => "^",
                },
                ast::BinaryOp::Bool(op) => match op {
                    ast::BoolBinaryOp::Eq => "==",
                    ast::BoolBinaryOp::Neq => "!=",
                    ast::BoolBinaryOp::Lt => "<",
                    ast::BoolBinaryOp::Gt => ">",
                    ast::BoolBinaryOp::Le => "<=",
                    ast::BoolBinaryOp::Ge => ">=",
                    ast::BoolBinaryOp::And => "&&",
                    ast::BoolBinaryOp::Or => "||",
                },
            };
            format!("({} {} {})", render(&binary.lhs), op, render(&binary.rhs))
        }
        ast::Expr::Call(name, args) => {
            let args = args.iter().map(render).collect::<Vec<_>>().join(", ");
            format!("{name}({args})")
        }
        ast::Expr::Tuple(elements) => {
            let elements = elements.iter().map(render).collect::<Vec<_>>().join(", ");
            format!("({elements})")
        }
    }
}
