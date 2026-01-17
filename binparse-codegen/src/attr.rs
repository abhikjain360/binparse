use binparse_dsl as ast;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Endian {
    #[default]
    Big,
    Little,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("@{attr} requires exactly {expected} argument(s), got {got}")]
    WrongArgCount {
        attr: &'static str,
        expected: usize,
        got: usize,
    },
    #[error("@endian argument must be 'big' or 'little', got '{0}'")]
    InvalidEndianValue(String),
    #[error("@endian cannot be applied to u8 (single byte has no endianness)")]
    EndianOnU8,
    #[error("@endian cannot be applied to bitfields")]
    EndianOnBitfield,
    #[error("@endian cannot be applied to struct ref (struct uses its own definition's endianness)")]
    EndianOnStructRef,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedAttrs {
    pub endian: Option<Endian>,
}

impl ParsedAttrs {
    pub fn parse(attrs: &[ast::Attribute<'_>]) -> Result<Self, Error> {
        let mut result = Self::default();
        for attr in attrs {
            if attr.name == "endian" {
                result.endian = Some(Self::parse_endian(attr)?);
            }
        }
        Ok(result)
    }

    fn parse_endian(attr: &ast::Attribute<'_>) -> Result<Endian, Error> {
        if attr.args.len() != 1 {
            return Err(Error::WrongArgCount {
                attr: "endian",
                expected: 1,
                got: attr.args.len(),
            });
        }
        match &attr.args[0] {
            ast::Expr::Path(path) if path.len() == 1 => match path[0] {
                "big" => Ok(Endian::Big),
                "little" => Ok(Endian::Little),
                other => Err(Error::InvalidEndianValue(other.to_string())),
            },
            _ => Err(Error::InvalidEndianValue("<non-identifier>".to_string())),
        }
    }

    pub fn merge_endian(&self, default: Endian) -> Endian {
        self.endian.unwrap_or(default)
    }
}
