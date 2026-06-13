use crate::{CodeGen, Error};

fn generate(dsl: &str) -> String {
    let ast = binparse_dsl_parse::parse_str(dsl).expect("failed to parse DSL");
    CodeGen::generate(&ast).expect("failed to generate code")
}

fn generate_err(dsl: &str) -> Error {
    let ast = binparse_dsl_parse::parse_str(dsl).expect("failed to parse DSL");
    CodeGen::generate(&ast).expect_err("expected codegen to fail")
}

fn normalized_items(code: &str) -> Vec<String> {
    let file: syn::File = syn::parse_str(code).expect("failed to parse code as Rust");
    let mut items = file
        .items
        .into_iter()
        .map(|item| {
            prettyplease::unparse(&syn::File {
                shebang: None,
                attrs: vec![],
                items: vec![item],
            })
        })
        .collect::<Vec<_>>();
    items.sort();
    items
}

fn assert_generated_eq(dsl: &str, expected: &str) {
    let actual = normalized_items(&generate(dsl));
    let expected = normalized_items(expected);
    if actual != expected {
        panic!(
            "generated code does not match expected\n\n--- expected ---\n{}\n--- actual ---\n{}",
            expected.join("\n"),
            actual.join("\n")
        );
    }
}

#[test]
fn golden_empty_struct() {
    assert_generated_eq(
        "struct Empty {}",
        r#"
        pub struct Empty<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Empty<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                Ok((Self { data }, data))
            }
        }
        "#,
    );
}

#[test]
fn golden_simple_primitive() {
    assert_generated_eq(
        "struct Simple { value: u32 }",
        r#"
        pub struct Simple<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Simple<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.value_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.value_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn value(&self) -> u32 {
                u32::from_be_bytes(self.data[0usize..4usize].try_into().unwrap())
            }
            pub fn value_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 4usize,
                    bit: 0usize,
                }
            }
            pub fn value_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn value_bit_range(&self) -> ::core::ops::Range<usize> {
                self.value_start_offset().bits()..self.value_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_bitfields() {
    assert_generated_eq(
        "struct IpFlags { version: b<4>, ihl: b<4>, dscp: b<6>, ecn: b<2> }",
        r#"
        pub struct IpFlags<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> IpFlags<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.version_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.ihl_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.dscp_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.ecn_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.ecn_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn version(&self) -> u8 {
                (self.data[0usize] >> 4usize) & 15u8
            }
            pub fn version_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 0usize,
                    bit: 4usize,
                }
            }
            pub fn version_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn version_bit_range(&self) -> ::core::ops::Range<usize> {
                self.version_start_offset().bits()..self.version_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn ihl(&self) -> u8 {
                (self.data[0usize] >> 0usize) & 15u8
            }
            pub fn ihl_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn ihl_start_offset(&self) -> binparse::Len {
                self.version_end_offset()
            }
            pub fn ihl_bit_range(&self) -> ::core::ops::Range<usize> {
                self.ihl_start_offset().bits()..self.ihl_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn dscp(&self) -> u8 {
                (self.data[1usize] >> 2usize) & 63u8
            }
            pub fn dscp_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 6usize,
                }
            }
            pub fn dscp_start_offset(&self) -> binparse::Len {
                self.ihl_end_offset()
            }
            pub fn dscp_bit_range(&self) -> ::core::ops::Range<usize> {
                self.dscp_start_offset().bits()..self.dscp_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn ecn(&self) -> u8 {
                (self.data[1usize] >> 0usize) & 3u8
            }
            pub fn ecn_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
            }
            pub fn ecn_start_offset(&self) -> binparse::Len {
                self.dscp_end_offset()
            }
            pub fn ecn_bit_range(&self) -> ::core::ops::Range<usize> {
                self.ecn_start_offset().bits()..self.ecn_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_cross_byte_bitfield() {
    assert_generated_eq(
        "struct Cross { a: b<5>, b: b<6>, c: b<5> }",
        r#"
        pub struct Cross<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Cross<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.a_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.b_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.c_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.c_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn a(&self) -> u8 {
                (self.data[0usize] >> 3usize) & 31u8
            }
            pub fn a_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 0usize,
                    bit: 5usize,
                }
            }
            pub fn a_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn a_bit_range(&self) -> ::core::ops::Range<usize> {
                self.a_start_offset().bits()..self.a_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn b(&self) -> u8 {
                {
                    let first_part = self.data[0usize] & 7u8;
                    let second_part = self.data[1usize] >> 5usize;
                    (first_part << 3usize) | second_part
                }
            }
            pub fn b_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 3usize,
                }
            }
            pub fn b_start_offset(&self) -> binparse::Len {
                self.a_end_offset()
            }
            pub fn b_bit_range(&self) -> ::core::ops::Range<usize> {
                self.b_start_offset().bits()..self.b_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn c(&self) -> u8 {
                (self.data[1usize] >> 0usize) & 31u8
            }
            pub fn c_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
            }
            pub fn c_start_offset(&self) -> binparse::Len {
                self.b_end_offset()
            }
            pub fn c_bit_range(&self) -> ::core::ops::Range<usize> {
                self.c_start_offset().bits()..self.c_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_nested_structs() {
    assert_generated_eq(
        "struct Inner { a: u8, b: u8 } struct Outer { prefix: u16, inner: Inner, suffix: u16 }",
        r#"
        pub struct Inner<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Inner<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.a_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.b_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.b_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn a(&self) -> u8 {
                self.data[0usize]
            }
            pub fn a_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn a_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn a_bit_range(&self) -> ::core::ops::Range<usize> {
                self.a_start_offset().bits()..self.a_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn b(&self) -> u8 {
                self.data[1usize]
            }
            pub fn b_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
            }
            pub fn b_start_offset(&self) -> binparse::Len {
                self.a_end_offset()
            }
            pub fn b_bit_range(&self) -> ::core::ops::Range<usize> {
                self.b_start_offset().bits()..self.b_end_offset().bits()
            }
        }
        pub struct Outer<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Outer<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.prefix_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.inner_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.suffix_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.suffix_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn prefix(&self) -> u16 {
                u16::from_be_bytes(self.data[0usize..2usize].try_into().unwrap())
            }
            pub fn prefix_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
            }
            pub fn prefix_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn prefix_bit_range(&self) -> ::core::ops::Range<usize> {
                self.prefix_start_offset().bits()..self.prefix_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn inner(&self) -> ::binparse::ParseResult<Inner<'_>> {
                Inner::parse(&self.data[2usize..]).map(|(value, _)| value)
            }
            pub fn inner_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 4usize,
                    bit: 0usize,
                }
            }
            pub fn inner_start_offset(&self) -> binparse::Len {
                self.prefix_end_offset()
            }
            pub fn inner_bit_range(&self) -> ::core::ops::Range<usize> {
                self.inner_start_offset().bits()..self.inner_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn suffix(&self) -> u16 {
                u16::from_be_bytes(self.data[4usize..6usize].try_into().unwrap())
            }
            pub fn suffix_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 6usize,
                    bit: 0usize,
                }
            }
            pub fn suffix_start_offset(&self) -> binparse::Len {
                self.inner_end_offset()
            }
            pub fn suffix_bit_range(&self) -> ::core::ops::Range<usize> {
                self.suffix_start_offset().bits()..self.suffix_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_fixed_array() {
    assert_generated_eq(
        "struct WithArray { count: u8, data: [u32; 4] }",
        r#"
        #[allow(non_camel_case_types)]
        pub struct WithArray_data_Iterator<'a> {
            idx: usize,
            count: usize,
            data: &'a [u8],
        }
        impl<'a> ::std::iter::Iterator for WithArray_data_Iterator<'a> {
            type Item = ::binparse::ParseResult<u32>;
            fn next(&mut self) -> std::option::Option<Self::Item> {
                if self.idx == self.count {
                    return None;
                }
                self.idx += 1;
                let value = u32::from_be_bytes(self.data[..4usize].try_into().unwrap());
                self.data = &self.data[4usize..];
                Some(Ok(value))
            }
        }
        pub struct WithArray<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> WithArray<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.count_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.data_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.data_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn count(&self) -> u8 {
                self.data[0usize]
            }
            pub fn count_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn count_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn count_bit_range(&self) -> ::core::ops::Range<usize> {
                self.count_start_offset().bits()..self.count_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn data(&self) -> ::binparse::ParseResult<WithArray_data_Iterator<'_>> {
                Ok(WithArray_data_Iterator {
                    idx: 0,
                    count: 4usize,
                    data: &self.data[1usize..],
                })
            }
            pub fn data_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 17usize,
                    bit: 0usize,
                }
            }
            pub fn data_start_offset(&self) -> binparse::Len {
                self.count_end_offset()
            }
            pub fn data_bit_range(&self) -> ::core::ops::Range<usize> {
                self.data_start_offset().bits()..self.data_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_expression_sized_array() {
    assert_generated_eq(
        "struct ExprArray { n: u8, items: [u16; n * 2] }",
        r#"
        #[allow(non_camel_case_types)]
        pub struct ExprArray_items_Iterator<'a> {
            idx: usize,
            count: usize,
            data: &'a [u8],
        }
        impl<'a> ::std::iter::Iterator for ExprArray_items_Iterator<'a> {
            type Item = ::binparse::ParseResult<u16>;
            fn next(&mut self) -> std::option::Option<Self::Item> {
                if self.idx == self.count {
                    return None;
                }
                self.idx += 1;
                let value = u16::from_be_bytes(self.data[..2usize].try_into().unwrap());
                self.data = &self.data[2usize..];
                Some(Ok(value))
            }
        }
        pub struct ExprArray<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> ExprArray<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.n_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.items_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.items_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn n(&self) -> u8 {
                self.data[0usize]
            }
            pub fn n_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn n_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn n_bit_range(&self) -> ::core::ops::Range<usize> {
                self.n_start_offset().bits()..self.n_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn items(&self) -> ::binparse::ParseResult<ExprArray_items_Iterator<'_>> {
                Ok(ExprArray_items_Iterator {
                    idx: 0,
                    count: (self.n() as usize).saturating_mul(2usize),
                    data: &self.data[1usize..],
                })
            }
            pub fn items_end_offset(&self) -> binparse::Len {
                ::binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
                    + ({
                        ::binparse::Len {
                            byte: 2usize,
                            bit: 0usize,
                        } * ((self.n() as usize).saturating_mul(2usize))
                    })
            }
            pub fn items_start_offset(&self) -> binparse::Len {
                self.n_end_offset()
            }
            pub fn items_bit_range(&self) -> ::core::ops::Range<usize> {
                self.items_start_offset().bits()..self.items_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_greedy_rest_array() {
    assert_generated_eq(
        "struct Rest { n: u8, @greedy(unsafe_eof) tail: [u8] }",
        r#"
        #[allow(non_camel_case_types)]
        pub struct Rest_tail_Iterator<'a> {
            idx: usize,
            count: usize,
            data: &'a [u8],
        }
        impl<'a> ::std::iter::Iterator for Rest_tail_Iterator<'a> {
            type Item = ::binparse::ParseResult<u8>;
            fn next(&mut self) -> std::option::Option<Self::Item> {
                if self.idx == self.count {
                    return None;
                }
                self.idx += 1;
                let value = self.data[0];
                self.data = &self.data[1..];
                Some(Ok(value))
            }
        }
        pub struct Rest<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Rest<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.n_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.tail_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.tail_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn n(&self) -> u8 {
                self.data[0usize]
            }
            pub fn n_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn n_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn n_bit_range(&self) -> ::core::ops::Range<usize> {
                self.n_start_offset().bits()..self.n_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn tail(&self) -> ::binparse::ParseResult<Rest_tail_Iterator<'_>> {
                Ok(Rest_tail_Iterator {
                    idx: 0,
                    count: self.data.len().saturating_sub(1usize),
                    data: &self.data[1usize..],
                })
            }
            pub fn tail_end_offset(&self) -> binparse::Len {
                ::binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
                    + ({
                        {
                            let start = 1usize;
                            ::binparse::Len {
                                byte: self.data.len().saturating_sub(start),
                                bit: 0,
                            }
                        }
                    })
            }
            pub fn tail_start_offset(&self) -> binparse::Len {
                self.n_end_offset()
            }
            pub fn tail_bit_range(&self) -> ::core::ops::Range<usize> {
                self.tail_start_offset().bits()..self.tail_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_until_array() {
    assert_generated_eq(
        "struct CStr { @until(x00) name: [u8], after: u8 }",
        r#"
        #[allow(non_camel_case_types)]
        pub struct CStr_name_Iterator<'a> {
            idx: usize,
            count: usize,
            data: &'a [u8],
        }
        impl<'a> ::std::iter::Iterator for CStr_name_Iterator<'a> {
            type Item = ::binparse::ParseResult<u8>;
            fn next(&mut self) -> std::option::Option<Self::Item> {
                if self.idx == self.count {
                    return None;
                }
                self.idx += 1;
                let value = self.data[0];
                self.data = &self.data[1..];
                Some(Ok(value))
            }
        }
        pub struct CStr<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> CStr<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.name_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.after_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.after_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn name(&self) -> ::binparse::ParseResult<CStr_name_Iterator<'_>> {
                Ok(CStr_name_Iterator {
                    idx: 0,
                    count: self.data[0usize..].iter().position(|&b| b == 0u8).unwrap_or(0),
                    data: &self.data[0usize..],
                })
            }
            pub fn name_end_offset(&self) -> binparse::Len {
                ::binparse::Len {
                    byte: 0usize,
                    bit: 0usize,
                }
                    + ({
                        {
                            let start = 0usize;
                            let byte = match self
                                .data
                                .get(start..)
                                .and_then(|rest| rest.iter().position(|&b| b == 0u8))
                            {
                                Some(pos) => pos.saturating_add(1),
                                None => self.data.len().saturating_add(1).saturating_sub(start),
                            };
                            ::binparse::Len { byte, bit: 0 }
                        }
                    })
            }
            pub fn name_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn name_bit_range(&self) -> ::core::ops::Range<usize> {
                self.name_start_offset().bits()..self.name_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn after(&self) -> u8 {
                {
                    let offset = ::binparse::Len {
                        byte: 0usize,
                        bit: 0usize,
                    }
                        + ({
                            {
                                let start = 0usize;
                                let byte = match self
                                    .data
                                    .get(start..)
                                    .and_then(|rest| rest.iter().position(|&b| b == 0u8))
                                {
                                    Some(pos) => pos.saturating_add(1),
                                    None => {
                                        self.data.len().saturating_add(1).saturating_sub(start)
                                    }
                                };
                                ::binparse::Len { byte, bit: 0 }
                            }
                        });
                    debug_assert!(offset.bit == 0, "primitive requires byte alignment");
                    self.data[offset.byte]
                }
            }
            pub fn after_end_offset(&self) -> binparse::Len {
                ({
                    ::binparse::Len {
                        byte: 0usize,
                        bit: 0usize,
                    }
                        + ({
                            {
                                let start = 0usize;
                                let byte = match self
                                    .data
                                    .get(start..)
                                    .and_then(|rest| rest.iter().position(|&b| b == 0u8))
                                {
                                    Some(pos) => pos.saturating_add(1),
                                    None => {
                                        self.data.len().saturating_add(1).saturating_sub(start)
                                    }
                                };
                                ::binparse::Len { byte, bit: 0 }
                            }
                        })
                })
                    + ::binparse::Len {
                        byte: 1usize,
                        bit: 0usize,
                    }
            }
            pub fn after_start_offset(&self) -> binparse::Len {
                self.name_end_offset()
            }
            pub fn after_bit_range(&self) -> ::core::ops::Range<usize> {
                self.after_start_offset().bits()..self.after_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_struct_ref_array() {
    assert_generated_eq(
        "struct Inner { a: u8 } struct StructArray { count: u8, items: [Inner; count] }",
        r#"
        pub struct Inner<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Inner<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.a_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.a_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn a(&self) -> u8 {
                self.data[0usize]
            }
            pub fn a_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn a_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn a_bit_range(&self) -> ::core::ops::Range<usize> {
                self.a_start_offset().bits()..self.a_end_offset().bits()
            }
        }
        #[allow(non_camel_case_types)]
        pub struct StructArray_items_Iterator<'a> {
            idx: usize,
            count: usize,
            data: &'a [u8],
        }
        impl<'a> ::std::iter::Iterator for StructArray_items_Iterator<'a> {
            type Item = ::binparse::ParseResult<Inner<'a>>;
            fn next(&mut self) -> std::option::Option<Self::Item> {
                if self.idx == self.count {
                    return None;
                }
                self.idx += 1;
                match Inner::parse(self.data) {
                    Ok((value, rem)) => {
                        self.data = rem;
                        Some(Ok(value))
                    }
                    Err(error) => Some(Err(error)),
                }
            }
        }
        pub struct StructArray<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> StructArray<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.count_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.items_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.items_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn count(&self) -> u8 {
                self.data[0usize]
            }
            pub fn count_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn count_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn count_bit_range(&self) -> ::core::ops::Range<usize> {
                self.count_start_offset().bits()..self.count_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn items(&self) -> ::binparse::ParseResult<StructArray_items_Iterator<'_>> {
                Ok(StructArray_items_Iterator {
                    idx: 0,
                    count: self.count() as usize,
                    data: &self.data[1usize..],
                })
            }
            pub fn items_end_offset(&self) -> binparse::Len {
                ::binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
                    + ({
                        ::binparse::Len {
                            byte: 1usize,
                            bit: 0usize,
                        } * (self.count() as usize)
                    })
            }
            pub fn items_start_offset(&self) -> binparse::Len {
                self.count_end_offset()
            }
            pub fn items_bit_range(&self) -> ::core::ops::Range<usize> {
                self.items_start_offset().bits()..self.items_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_bitfield_array() {
    assert_generated_eq(
        "struct BitArray { nibbles: [b<4>; 4] }",
        r#"
        #[allow(non_camel_case_types)]
        pub struct BitArray_nibbles_Iterator<'a> {
            idx: usize,
            count: usize,
            data: &'a [u8],
            bit_offset: usize,
        }
        impl<'a> ::std::iter::Iterator for BitArray_nibbles_Iterator<'a> {
            type Item = ::binparse::ParseResult<u8>;
            fn next(&mut self) -> std::option::Option<Self::Item> {
                if self.idx == self.count {
                    return None;
                }
                self.idx += 1;
                let byte_idx = self.bit_offset / 8;
                let bit_idx = self.bit_offset % 8;
                let value = if bit_idx + 4usize <= 8 {
                    let mask = (1u8 << 4usize) - 1;
                    (self.data[byte_idx] >> (8 - bit_idx - 4usize)) & mask
                } else {
                    let bits_in_first = 8 - bit_idx;
                    let bits_in_second = 4usize - bits_in_first;
                    let first_mask = (1u8 << bits_in_first) - 1;
                    let first_part = self.data[byte_idx] & first_mask;
                    let second_part = self.data[byte_idx + 1] >> (8 - bits_in_second);
                    (first_part << bits_in_second) | second_part
                };
                self.bit_offset += 4usize;
                Some(Ok(value))
            }
        }
        pub struct BitArray<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> BitArray<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.nibbles_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.nibbles_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn nibbles(&self) -> ::binparse::ParseResult<BitArray_nibbles_Iterator<'_>> {
                Ok(BitArray_nibbles_Iterator {
                    idx: 0,
                    count: 4usize,
                    data: &self.data[0usize..],
                    bit_offset: 0,
                })
            }
            pub fn nibbles_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
            }
            pub fn nibbles_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn nibbles_bit_range(&self) -> ::core::ops::Range<usize> {
                self.nibbles_start_offset().bits()..self.nibbles_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_concat() {
    assert_generated_eq(
        "struct WithConcat { flags: b<3>, fragment_offset: concat(b<5>, u8) }",
        r#"
        pub struct WithConcat<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> WithConcat<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.flags_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.fragment_offset_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.fragment_offset_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn flags(&self) -> u8 {
                (self.data[0usize] >> 5usize) & 7u8
            }
            pub fn flags_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 0usize,
                    bit: 3usize,
                }
            }
            pub fn flags_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn flags_bit_range(&self) -> ::core::ops::Range<usize> {
                self.flags_start_offset().bits()..self.flags_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn fragment_offset_0(&self) -> u8 {
                (self.data[0usize] >> 0usize) & 31u8
            }
            #[allow(clippy::identity_op)]
            pub fn fragment_offset_1(&self) -> u8 {
                self.data[1usize]
            }
            #[allow(clippy::identity_op)]
            pub fn fragment_offset(&self) -> (u8, u8) {
                (self.fragment_offset_0(), self.fragment_offset_1())
            }
            pub fn fragment_offset_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
            }
            pub fn fragment_offset_start_offset(&self) -> binparse::Len {
                self.flags_end_offset()
            }
            pub fn fragment_offset_bit_range(&self) -> ::core::ops::Range<usize> {
                self
                    .fragment_offset_start_offset()
                    .bits()..self.fragment_offset_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_union_single_discriminant() {
    assert_generated_eq(
        r#"struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { id: u16 },
                _ => Unknown { },
            },
        }"#,
        r#"
        #[allow(non_camel_case_types)]
        pub struct Packet_payload_Echo<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Packet_payload_Echo<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.id_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.id_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn id(&self) -> u16 {
                u16::from_be_bytes(self.data[0usize..2usize].try_into().unwrap())
            }
            pub fn id_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
            }
            pub fn id_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn id_bit_range(&self) -> ::core::ops::Range<usize> {
                self.id_start_offset().bits()..self.id_end_offset().bits()
            }
        }
        #[allow(non_camel_case_types)]
        pub struct Packet_payload_Unknown<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Packet_payload_Unknown<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                Ok((Self { data }, data))
            }
        }
        #[allow(non_camel_case_types)]
        pub enum Packet_payload<'a> {
            Echo(Packet_payload_Echo<'a>),
            Unknown(Packet_payload_Unknown<'a>),
        }
        pub struct Packet<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Packet<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.ty_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                me.payload_union_check()?;
                {
                    let len = me.payload_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.payload_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn ty(&self) -> u8 {
                self.data[0usize]
            }
            pub fn ty_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn ty_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn ty_bit_range(&self) -> ::core::ops::Range<usize> {
                self.ty_start_offset().bits()..self.ty_end_offset().bits()
            }
            fn payload_union_check(&self) -> Result<(), binparse::ParseError> {
                match self.ty() as usize {
                    1 => {
                        Packet_payload_Echo::parse(&self.data[(1usize).min(self.data.len())..])?;
                    }
                    _ => {
                        Packet_payload_Unknown::parse(
                            &self.data[(1usize).min(self.data.len())..],
                        )?;
                    }
                }
                Ok(())
            }
            #[allow(clippy::identity_op)]
            pub fn payload(&self) -> ::binparse::ParseResult<Packet_payload<'_>> {
                match self.ty() as usize {
                    1 => {
                        Packet_payload_Echo::parse(&self.data[1usize..])
                            .map(|(value, _)| Packet_payload::Echo(value))
                    }
                    _ => {
                        Packet_payload_Unknown::parse(&self.data[1usize..])
                            .map(|(value, _)| Packet_payload::Unknown(value))
                    }
                }
            }
            pub fn payload_end_offset(&self) -> binparse::Len {
                ::binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
                    + ({
                        match self.ty() as usize {
                            1 => {
                                ::binparse::Len {
                                    byte: 2usize,
                                    bit: 0usize,
                                }
                            }
                            _ => {
                                ::binparse::Len {
                                    byte: 0usize,
                                    bit: 0usize,
                                }
                            }
                        }
                    })
            }
            pub fn payload_start_offset(&self) -> binparse::Len {
                self.ty_end_offset()
            }
            pub fn payload_bit_range(&self) -> ::core::ops::Range<usize> {
                self.payload_start_offset().bits()..self.payload_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_union_tuple_discriminant_with_multiple_matchers() {
    assert_generated_eq(
        r#"struct Packet {
            ty: u8,
            code: u8,
            payload: union(ty, code) {
                (0, 0) | (0, 8) => Echo { id: u16 },
                _ => Unknown { },
            },
        }"#,
        r#"
        #[allow(non_camel_case_types)]
        pub struct Packet_payload_Echo<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Packet_payload_Echo<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.id_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.id_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn id(&self) -> u16 {
                u16::from_be_bytes(self.data[0usize..2usize].try_into().unwrap())
            }
            pub fn id_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
            }
            pub fn id_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn id_bit_range(&self) -> ::core::ops::Range<usize> {
                self.id_start_offset().bits()..self.id_end_offset().bits()
            }
        }
        #[allow(non_camel_case_types)]
        pub struct Packet_payload_Unknown<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Packet_payload_Unknown<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                Ok((Self { data }, data))
            }
        }
        #[allow(non_camel_case_types)]
        pub enum Packet_payload<'a> {
            Echo(Packet_payload_Echo<'a>),
            Unknown(Packet_payload_Unknown<'a>),
        }
        pub struct Packet<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Packet<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.ty_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.code_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                me.payload_union_check()?;
                {
                    let len = me.payload_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.payload_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn ty(&self) -> u8 {
                self.data[0usize]
            }
            pub fn ty_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn ty_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn ty_bit_range(&self) -> ::core::ops::Range<usize> {
                self.ty_start_offset().bits()..self.ty_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn code(&self) -> u8 {
                self.data[1usize]
            }
            pub fn code_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
            }
            pub fn code_start_offset(&self) -> binparse::Len {
                self.ty_end_offset()
            }
            pub fn code_bit_range(&self) -> ::core::ops::Range<usize> {
                self.code_start_offset().bits()..self.code_end_offset().bits()
            }
            fn payload_union_check(&self) -> Result<(), binparse::ParseError> {
                match (self.ty() as usize, self.code() as usize) {
                    (0, 0) | (0, 8) => {
                        Packet_payload_Echo::parse(&self.data[(2usize).min(self.data.len())..])?;
                    }
                    _ => {
                        Packet_payload_Unknown::parse(
                            &self.data[(2usize).min(self.data.len())..],
                        )?;
                    }
                }
                Ok(())
            }
            #[allow(clippy::identity_op)]
            pub fn payload(&self) -> ::binparse::ParseResult<Packet_payload<'_>> {
                match (self.ty() as usize, self.code() as usize) {
                    (0, 0) | (0, 8) => {
                        Packet_payload_Echo::parse(&self.data[2usize..])
                            .map(|(value, _)| Packet_payload::Echo(value))
                    }
                    _ => {
                        Packet_payload_Unknown::parse(&self.data[2usize..])
                            .map(|(value, _)| Packet_payload::Unknown(value))
                    }
                }
            }
            pub fn payload_end_offset(&self) -> binparse::Len {
                ::binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
                    + ({
                        match (self.ty() as usize, self.code() as usize) {
                            (0, 0) | (0, 8) => {
                                ::binparse::Len {
                                    byte: 2usize,
                                    bit: 0usize,
                                }
                            }
                            _ => {
                                ::binparse::Len {
                                    byte: 0usize,
                                    bit: 0usize,
                                }
                            }
                        }
                    })
            }
            pub fn payload_start_offset(&self) -> binparse::Len {
                self.code_end_offset()
            }
            pub fn payload_bit_range(&self) -> ::core::ops::Range<usize> {
                self.payload_start_offset().bits()..self.payload_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_union_error_variant() {
    assert_generated_eq(
        r#"error {
            UNKNOWN_TYPE { ty: u8 },
        }

        struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { id: u16 },
                _ => @error(UNKNOWN_TYPE { ty: ty }),
            },
        }"#,
        r#"
        #[allow(non_camel_case_types)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Error {
            Parse(::binparse::ParseError),
            UNKNOWN_TYPE { ty: u8 },
        }
        #[allow(non_camel_case_types)]
        pub struct Packet_payload_Echo<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Packet_payload_Echo<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.id_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.id_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn id(&self) -> u16 {
                u16::from_be_bytes(self.data[0usize..2usize].try_into().unwrap())
            }
            pub fn id_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
            }
            pub fn id_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn id_bit_range(&self) -> ::core::ops::Range<usize> {
                self.id_start_offset().bits()..self.id_end_offset().bits()
            }
        }
        #[allow(non_camel_case_types)]
        pub enum Packet_payload<'a> {
            Echo(Packet_payload_Echo<'a>),
        }
        pub struct Packet<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Packet<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.ty_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                me.payload_union_check()?;
                {
                    let len = me.payload_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.payload_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn ty(&self) -> u8 {
                self.data[0usize]
            }
            pub fn ty_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn ty_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn ty_bit_range(&self) -> ::core::ops::Range<usize> {
                self.ty_start_offset().bits()..self.ty_end_offset().bits()
            }
            fn payload_union_check(&self) -> Result<(), binparse::ParseError> {
                match self.ty() as usize {
                    1 => {
                        Packet_payload_Echo::parse(&self.data[(1usize).min(self.data.len())..])?;
                    }
                    _ => {}
                }
                Ok(())
            }
            #[allow(clippy::identity_op)]
            pub fn payload(&self) -> Result<Packet_payload<'_>, Error> {
                match self.ty() as usize {
                    1 => {
                        Packet_payload_Echo::parse(&self.data[1usize..])
                            .map(|(value, _)| Packet_payload::Echo(value))
                            .map_err(Error::Parse)
                    }
                    _ => {
                        Err(Error::UNKNOWN_TYPE {
                            ty: (self.ty() as usize) as u8,
                        })
                    }
                }
            }
            pub fn payload_end_offset(&self) -> binparse::Len {
                ::binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
                    + ({
                        match self.ty() as usize {
                            1 => {
                                ::binparse::Len {
                                    byte: 2usize,
                                    bit: 0usize,
                                }
                            }
                            _ => ::binparse::Len::ZERO,
                        }
                    })
            }
            pub fn payload_start_offset(&self) -> binparse::Len {
                self.ty_end_offset()
            }
            pub fn payload_bit_range(&self) -> ::core::ops::Range<usize> {
                self.payload_start_offset().bits()..self.payload_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_endian_attributes() {
    assert_generated_eq(
        r#"@endian(little)
        struct LittlePacket {
            header: u32,
            @endian(big) mixed: u16,
            data: u8,
        }"#,
        r#"
        pub struct LittlePacket<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> LittlePacket<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.header_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.mixed_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.data_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.data_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn header(&self) -> u32 {
                u32::from_le_bytes(self.data[0usize..4usize].try_into().unwrap())
            }
            pub fn header_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 4usize,
                    bit: 0usize,
                }
            }
            pub fn header_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn header_bit_range(&self) -> ::core::ops::Range<usize> {
                self.header_start_offset().bits()..self.header_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn mixed(&self) -> u16 {
                u16::from_be_bytes(self.data[4usize..6usize].try_into().unwrap())
            }
            pub fn mixed_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 6usize,
                    bit: 0usize,
                }
            }
            pub fn mixed_start_offset(&self) -> binparse::Len {
                self.header_end_offset()
            }
            pub fn mixed_bit_range(&self) -> ::core::ops::Range<usize> {
                self.mixed_start_offset().bits()..self.mixed_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn data(&self) -> u8 {
                self.data[6usize]
            }
            pub fn data_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 7usize,
                    bit: 0usize,
                }
            }
            pub fn data_start_offset(&self) -> binparse::Len {
                self.mixed_end_offset()
            }
            pub fn data_bit_range(&self) -> ::core::ops::Range<usize> {
                self.data_start_offset().bits()..self.data_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_signed_primitives() {
    assert_generated_eq(
        "@endian(little) struct SignedPrim { a: i8, b: i16, @endian(big) c: i32 }",
        r#"
        pub struct SignedPrim<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> SignedPrim<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.a_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.b_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.c_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.c_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn a(&self) -> i8 {
                self.data[0usize] as i8
            }
            pub fn a_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn a_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn a_bit_range(&self) -> ::core::ops::Range<usize> {
                self.a_start_offset().bits()..self.a_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn b(&self) -> i16 {
                i16::from_le_bytes(self.data[1usize..3usize].try_into().unwrap())
            }
            pub fn b_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 3usize,
                    bit: 0usize,
                }
            }
            pub fn b_start_offset(&self) -> binparse::Len {
                self.a_end_offset()
            }
            pub fn b_bit_range(&self) -> ::core::ops::Range<usize> {
                self.b_start_offset().bits()..self.b_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn c(&self) -> i32 {
                i32::from_be_bytes(self.data[3usize..7usize].try_into().unwrap())
            }
            pub fn c_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 7usize,
                    bit: 0usize,
                }
            }
            pub fn c_start_offset(&self) -> binparse::Len {
                self.b_end_offset()
            }
            pub fn c_bit_range(&self) -> ::core::ops::Range<usize> {
                self.c_start_offset().bits()..self.c_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_lsb_bit_order() {
    assert_generated_eq(
        "@bit_order(lsb) struct LsbBits { low: b<3>, high: b<5> }",
        r#"
        pub struct LsbBits<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> LsbBits<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.low_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.high_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.high_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn low(&self) -> u8 {
                (self.data[0usize] >> 0usize) & 7u8
            }
            pub fn low_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 0usize,
                    bit: 3usize,
                }
            }
            pub fn low_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn low_bit_range(&self) -> ::core::ops::Range<usize> {
                self.low_start_offset().bits()..self.low_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn high(&self) -> u8 {
                (self.data[0usize] >> 3usize) & 31u8
            }
            pub fn high_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn high_start_offset(&self) -> binparse::Len {
                self.low_end_offset()
            }
            pub fn high_bit_range(&self) -> ::core::ops::Range<usize> {
                self.high_start_offset().bits()..self.high_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_fixed_hook() {
    assert_generated_eq(
        r#"struct WithFixedHook {
            prefix: u8,
            @hook(double_it, u32)
            value: u16,
            suffix: u8,
        }"#,
        r#"
        pub struct WithFixedHook<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> WithFixedHook<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.prefix_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.value_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.suffix_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.suffix_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn prefix(&self) -> u8 {
                self.data[0usize]
            }
            pub fn prefix_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn prefix_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn prefix_bit_range(&self) -> ::core::ops::Range<usize> {
                self.prefix_start_offset().bits()..self.prefix_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn value(&self) -> u32 {
                double_it(u16::from_be_bytes(self.data[1usize..3usize].try_into().unwrap()))
            }
            pub fn value_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 3usize,
                    bit: 0usize,
                }
            }
            pub fn value_start_offset(&self) -> binparse::Len {
                self.prefix_end_offset()
            }
            pub fn value_bit_range(&self) -> ::core::ops::Range<usize> {
                self.value_start_offset().bits()..self.value_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn suffix(&self) -> u8 {
                self.data[3usize]
            }
            pub fn suffix_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 4usize,
                    bit: 0usize,
                }
            }
            pub fn suffix_start_offset(&self) -> binparse::Len {
                self.value_end_offset()
            }
            pub fn suffix_bit_range(&self) -> ::core::ops::Range<usize> {
                self.suffix_start_offset().bits()..self.suffix_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_vla_hook() {
    assert_generated_eq(
        r#"struct WithVlaHook {
            len: u8,
            @hook(parse_cstring, String)
            name: [u8],
        }"#,
        r#"
        pub struct WithVlaHook<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> WithVlaHook<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.len_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                me.name_raw()?;
                {
                    let len = me.name_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.name_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn len(&self) -> u8 {
                self.data[0usize]
            }
            pub fn len_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn len_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn len_bit_range(&self) -> ::core::ops::Range<usize> {
                self.len_start_offset().bits()..self.len_end_offset().bits()
            }
            fn name_raw(&self) -> ::binparse::ParseResult<(String, usize)> {
                let start = 1usize;
                let (value, consumed) = parse_cstring(
                    &self.data[start..],
                    ::binparse::HookContext {
                        field: "WithVlaHook.name",
                        offset: start,
                        enclosing: self.data,
                    },
                )?;
                let end = start.saturating_add(consumed);
                if end > self.data.len() {
                    return Err(::binparse::ParseError::NotEnoughData {
                        expected: end,
                        got: self.data.len(),
                    });
                }
                Ok((value, consumed))
            }
            pub fn name(&self) -> ::binparse::ParseResult<String> {
                self.name_raw().map(|(value, _)| value)
            }
            pub fn name_end_offset(&self) -> binparse::Len {
                ::binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
                    + ({
                        match self.name_raw() {
                            Ok((_, consumed)) => {
                                binparse::Len {
                                    byte: consumed,
                                    bit: 0,
                                }
                            }
                            Err(_) => binparse::Len::ZERO,
                        }
                    })
            }
            pub fn name_start_offset(&self) -> binparse::Len {
                self.len_end_offset()
            }
            pub fn name_bit_range(&self) -> ::core::ops::Range<usize> {
                self.name_start_offset().bits()..self.name_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn duplicate_struct_is_rejected() {
    let err = generate_err("struct Dup { a: u8 } struct Dup { b: u16 }");
    assert!(matches!(err, Error::DuplicateStruct { .. }));
}

#[test]
fn endian_on_u8_is_rejected() {
    let err = generate_err("struct Foo { @endian(little) a: u8 }");
    assert!(
        err.to_string()
            .contains("@endian cannot be applied to single-byte integers")
    );
}

#[test]
fn endian_on_bitfield_is_rejected() {
    let err = generate_err("struct Foo { @endian(little) a: b<4> }");
    assert!(
        err.to_string()
            .contains("@endian cannot be applied to bitfields")
    );
}

#[test]
fn endian_on_struct_ref_is_rejected() {
    let err = generate_err("struct Inner { x: u8 } struct Foo { @endian(big) inner: Inner }");
    assert!(
        err.to_string()
            .contains("@endian cannot be applied to struct ref")
    );
}

#[test]
fn invalid_endian_value_is_rejected() {
    let err = generate_err("struct Foo { @endian(middle) a: u16 }");
    assert!(err.to_string().contains("@endian argument must be"));
}

#[test]
fn endian_on_i8_is_rejected() {
    let err = generate_err("struct Foo { @endian(little) a: i8 }");
    assert!(
        err.to_string()
            .contains("@endian cannot be applied to single-byte integers")
    );
}

#[test]
fn endian_on_single_byte_array_is_rejected() {
    let err = generate_err("struct Foo { @endian(little) xs: [u8; 2] }");
    assert!(
        err.to_string()
            .contains("@endian cannot be applied to single-byte integers")
    );
}

#[test]
fn endian_on_bitfield_array_is_rejected() {
    let err = generate_err("struct Foo { @endian(little) xs: [b<4>; 2] }");
    assert!(
        err.to_string()
            .contains("@endian cannot be applied to bitfields")
    );
}

#[test]
fn endian_on_struct_ref_array_is_rejected() {
    let err =
        generate_err("struct Inner { x: u8 } struct Foo { @endian(big) xs: [Inner; 2] }");
    assert!(
        err.to_string()
            .contains("@endian cannot be applied to struct ref")
    );
}

#[test]
fn bit_order_on_primitive_is_rejected() {
    let err = generate_err("struct Foo { @bit_order(lsb) a: u16 }");
    assert!(
        err.to_string()
            .contains("@bit_order can only be applied to bitfields")
    );
}

#[test]
fn bit_order_on_struct_ref_is_rejected() {
    let err = generate_err("struct Inner { x: u8 } struct Foo { @bit_order(lsb) inner: Inner }");
    assert!(
        err.to_string()
            .contains("@bit_order can only be applied to bitfields")
    );
}

#[test]
fn invalid_bit_order_value_is_rejected() {
    let err = generate_err("struct Foo { @bit_order(big) a: b<4> }");
    assert!(err.to_string().contains("@bit_order argument must be"));
}

#[test]
fn endian_wrong_arg_count_is_rejected() {
    let err = generate_err("struct Foo { @endian(big, little) a: u16 }");
    assert!(err.to_string().contains("requires exactly 1 argument"));
}

#[test]
fn array_size_unknown_field_is_rejected() {
    let err = generate_err("struct Foo { items: [u16; nope] }");
    assert!(
        err.to_string()
            .contains("expression 'nope' references field 'nope' which is unknown or not yet parsed")
    );
}

#[test]
fn array_size_non_numeric_field_is_rejected() {
    let err = generate_err("struct Inner { x: u8 } struct Foo { inner: Inner, items: [u8; inner] }");
    assert!(
        err.to_string()
            .contains("expression 'inner' references field 'inner' which is not a numeric field")
    );
}

#[test]
fn union_unknown_argument_is_rejected() {
    let err = generate_err(
        r#"struct Foo {
            payload: union(kind) {
                1 => A { x: u8 },
                _ => B { },
            },
        }"#,
    );
    assert!(
        err.to_string()
            .contains("expression 'kind' references field 'kind' which is unknown or not yet parsed")
    );
}

#[test]
fn union_non_numeric_argument_is_rejected() {
    let err = generate_err(
        r#"struct Inner { x: u8 }
        struct Foo {
            inner: Inner,
            payload: union(inner) {
                1 => A { x: u8 },
                _ => B { },
            },
        }"#,
    );
    assert!(
        err.to_string()
            .contains("expression 'inner' references field 'inner' which is not a numeric field")
    );
}

#[test]
fn union_without_catch_all_is_rejected() {
    let err = generate_err(
        r#"struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { id: u16 },
            },
        }"#,
    );
    assert!(err.to_string().contains("union is not exhaustive"));
}

#[test]
fn union_with_wildcard_error_variant_is_exhaustive() {
    let code = generate(
        r#"error {
            UNKNOWN_TYPE,
        }

        struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { id: u16 },
                _ => @error(UNKNOWN_TYPE),
            },
        }"#,
    );
    assert!(code.contains("Err(Error::UNKNOWN_TYPE)"));
}

#[test]
fn union_tuple_of_wildcards_is_exhaustive() {
    let code = generate(
        r#"struct Packet {
            ty: u8,
            code: u8,
            payload: union(ty, code) {
                (1, 1) => Echo { id: u16 },
                (_, _) => Unknown { },
            },
        }"#,
    );
    assert!(code.contains("(_, _) =>"));
}

#[test]
fn union_matcher_arity_mismatch_is_rejected() {
    let err = generate_err(
        r#"struct Packet {
            ty: u8,
            payload: union(ty) {
                (1, 2) => Echo { id: u16 },
                _ => Unknown { },
            },
        }"#,
    );
    assert!(
        err.to_string()
            .contains("matcher has 2 elements but union has 1 arguments")
    );
}

#[test]
fn union_literal_matcher_on_tuple_union_is_rejected() {
    let err = generate_err(
        r#"struct Packet {
            ty: u8,
            code: u8,
            payload: union(ty, code) {
                1 => Echo { id: u16 },
                _ => Unknown { },
            },
        }"#,
    );
    assert!(
        err.to_string()
            .contains("matcher has 1 elements but union has 2 arguments")
    );
}

#[test]
fn union_unknown_error_variant_is_rejected() {
    let err = generate_err(
        r#"struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { id: u16 },
                _ => @error(UNKNOWN_TYPE),
            },
        }"#,
    );
    assert!(
        err.to_string()
            .contains("@error variant 'UNKNOWN_TYPE' is not declared in an error block")
    );
}

#[test]
fn union_error_variant_missing_field_is_rejected() {
    let err = generate_err(
        r#"error {
            UNKNOWN_TYPE { ty: u8, code: u8 },
        }

        struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { id: u16 },
                _ => @error(UNKNOWN_TYPE { ty: ty }),
            },
        }"#,
    );
    assert!(
        err.to_string()
            .contains("@error variant 'UNKNOWN_TYPE' is missing field 'code'")
    );
}

#[test]
fn union_error_variant_unknown_field_is_rejected() {
    let err = generate_err(
        r#"error {
            UNKNOWN_TYPE { ty: u8 },
        }

        struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { id: u16 },
                _ => @error(UNKNOWN_TYPE { ty: ty, extra: ty }),
            },
        }"#,
    );
    assert!(
        err.to_string()
            .contains("@error variant 'UNKNOWN_TYPE' has no declared field 'extra'")
    );
}

#[test]
fn unions_in_concat_get_distinct_names() {
    let code = generate(
        r#"struct Packet {
            a: u8,
            b: u8,
            pair: concat(
                union(a) { 1 => X { x: u8 }, _ => XOther { } },
                union(b) { 2 => Y { y: u16 }, _ => YOther { } }
            ),
        }"#,
    );
    assert!(code.contains("pub enum Packet_pair_0<'a>"));
    assert!(code.contains("pub enum Packet_pair_1<'a>"));
    assert!(code.contains("me.pair_0_union_check()?;"));
    assert!(code.contains("me.pair_1_union_check()?;"));
}

#[test]
fn error_struct_name_conflict_is_rejected() {
    let err = generate_err(
        r#"error {
            UNKNOWN_TYPE,
        }

        struct Error {
            ty: u8,
        }"#,
    );
    assert!(
        err.to_string()
            .contains("struct name 'Error' conflicts with the generated error enum")
    );
}

#[test]
fn error_struct_name_without_error_block_is_allowed() {
    let code = generate("struct Error { ty: u8 }");
    assert!(code.contains("pub struct Error<'a>"));
}

#[test]
fn parse_error_variant_name_is_rejected() {
    let err = generate_err("error { Parse }");
    assert!(
        err.to_string()
            .contains("error variant 'Parse' is reserved for wrapped parse errors")
    );
}

#[test]
fn duplicate_error_block_is_rejected() {
    let err = generate_err("error { A } error { B }");
    assert!(err.to_string().contains("duplicate error block"));
}

#[test]
fn duplicate_error_variant_is_rejected() {
    let err = generate_err("error { A, A }");
    assert!(err.to_string().contains("duplicate error variant 'A'"));
}

#[test]
fn union_variant_validation_is_generated_in_variant_parse() {
    let code = generate(
        r#"struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { version = 4 },
                _ => Unknown { },
            },
        }"#,
    );
    assert!(code.contains("fn version_validate"));
    assert!(code.contains("me.version_validate()?;"));
}

#[test]
fn array_size_forward_reference_is_rejected() {
    let err = generate_err("struct Foo { data: [u8; later], later: u8 }");
    assert!(
        err.to_string()
            .contains("references field 'later' which is unknown or not yet parsed")
    );
}

#[test]
fn array_size_bool_expr_is_rejected() {
    let err = generate_err("struct Foo { n: u8, data: [u8; n == 1] }");
    assert!(
        err.to_string()
            .contains("is a boolean but a number is required")
    );
}

#[test]
fn array_size_string_is_rejected() {
    let err = generate_err(r#"struct Foo { data: [u8; "two"] }"#);
    assert!(
        err.to_string()
            .contains("is a string but a number is required")
    );
}

#[test]
fn array_size_nested_path_is_rejected() {
    let err = generate_err("struct Foo { a: u8, data: [u8; a.len] }");
    assert!(err.to_string().contains("nested path 'a.len'"));
}

#[test]
fn array_size_const_overflow_is_rejected() {
    let err = generate_err("struct Foo { data: [u8; 4294967296 * 4294967296] }");
    assert!(err.to_string().contains("overflows"));
}

#[test]
fn array_size_division_by_zero_is_rejected() {
    let err = generate_err("struct Foo { data: [u8; 8 / 0] }");
    assert!(err.to_string().contains("divides by zero"));
}

#[test]
fn array_size_dynamic_divisor_is_rejected() {
    let err = generate_err("struct Foo { n: u8, data: [u8; 8 / n] }");
    assert!(
        err.to_string()
            .contains("divides by a non-constant value")
    );
}

#[test]
fn same_array_field_name_in_two_structs_does_not_collide() {
    let code = generate(
        "struct First { n: u8, xs: [u8; n] } struct Second { n: u8, xs: [u16; n] }",
    );
    assert!(code.contains("First_xs_Iterator"));
    assert!(code.contains("Second_xs_Iterator"));
}

#[test]
fn array_size_const_expr_is_folded() {
    let code = generate("struct Foo { data: [u8; 2 * 3 + 1] }");
    assert!(code.contains("count: 7usize"));
}

#[test]
fn array_size_subtraction_saturates() {
    let code = generate("struct Foo { n: u8, data: [u8; n - 1] }");
    assert!(code.contains("(self.n() as usize).saturating_sub(1usize)"));
}

fn constraint_expr<'a>(ast: &'a [binparse_dsl::Definition<'a>]) -> &'a binparse_dsl::Expr<'a> {
    let binparse_dsl::Definition::Struct(s) = &ast[0] else {
        panic!("expected struct");
    };
    let binparse_dsl::StructItem::Field(f) = &s.items[0] else {
        panic!("expected field");
    };
    let binparse_dsl::FieldValue::Constraint(e) = &f.value else {
        panic!("expected constraint");
    };
    e
}

fn numeric_done_fields() -> Vec<crate::struct_::DoneField> {
    ["n", "m"]
        .into_iter()
        .map(|name| crate::struct_::DoneField {
            name: name.to_string(),
            field_type: crate::struct_::DoneFieldType::Primitive,
            conditional: false,
        })
        .collect()
}

#[test]
fn lower_bool_expr() {
    let ast = binparse_dsl_parse::parse_str("struct Foo { c = n == 1 && m < 2 }").unwrap();
    let lowered = crate::expr::lower(
        constraint_expr(&ast),
        crate::expr::ExprType::Bool,
        &numeric_done_fields(),
    )
    .unwrap();
    let expected = quote::quote! {
        ((((self.n() as usize) == (1usize))) && (((self.m() as usize) < (2usize))))
    };
    assert_eq!(lowered.tokens.to_string(), expected.to_string());
}

#[test]
fn lower_bool_rejects_numeric_expr() {
    let ast = binparse_dsl_parse::parse_str("struct Foo { c = n + 1 }").unwrap();
    let err = crate::expr::lower(
        constraint_expr(&ast),
        crate::expr::ExprType::Bool,
        &numeric_done_fields(),
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("expression '(n + 1)' is a number but a boolean is required")
    );
}

#[test]
fn golden_constant_fields() {
    assert_generated_eq(
        "struct Magic { magic = xc0de, flags = b101 }",
        r#"
        pub struct Magic<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Magic<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.magic_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                me.magic_validate()?;
                {
                    let len = me.flags_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                me.flags_validate()?;
                let len = me.flags_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn magic(&self) -> u16 {
                u16::from_be_bytes(self.data[0usize..2usize].try_into().unwrap())
            }
            pub fn magic_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
            }
            pub fn magic_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn magic_bit_range(&self) -> ::core::ops::Range<usize> {
                self.magic_start_offset().bits()..self.magic_end_offset().bits()
            }
            #[allow(clippy::unnecessary_cast)]
            fn magic_validate(&self) -> Result<(), binparse::ParseError> {
                if self.magic() != 49374 {
                    return Err(binparse::ParseError::ValidationFailed {
                        field: "Magic.magic",
                        actual: self.magic() as u128,
                    });
                }
                Ok(())
            }
            #[allow(clippy::identity_op)]
            pub fn flags(&self) -> u8 {
                (self.data[2usize] >> 5usize) & 7u8
            }
            pub fn flags_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 3usize,
                }
            }
            pub fn flags_start_offset(&self) -> binparse::Len {
                self.magic_end_offset()
            }
            pub fn flags_bit_range(&self) -> ::core::ops::Range<usize> {
                self.flags_start_offset().bits()..self.flags_end_offset().bits()
            }
            #[allow(clippy::unnecessary_cast)]
            fn flags_validate(&self) -> Result<(), binparse::ParseError> {
                if self.flags() != 5 {
                    return Err(binparse::ParseError::ValidationFailed {
                        field: "Magic.flags",
                        actual: self.flags() as u128,
                    });
                }
                Ok(())
            }
        }
        "#,
    );
}

#[test]
fn golden_check_and_range() {
    assert_generated_eq(
        "struct Checked { n: u8, @range(1, n + 1) @check(m >= n) m: u8 }",
        r#"
        pub struct Checked<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Checked<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.n_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.m_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                me.m_validate()?;
                let len = me.m_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn n(&self) -> u8 {
                self.data[0usize]
            }
            pub fn n_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn n_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn n_bit_range(&self) -> ::core::ops::Range<usize> {
                self.n_start_offset().bits()..self.n_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn m(&self) -> u8 {
                self.data[1usize]
            }
            pub fn m_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
            }
            pub fn m_start_offset(&self) -> binparse::Len {
                self.n_end_offset()
            }
            pub fn m_bit_range(&self) -> ::core::ops::Range<usize> {
                self.m_start_offset().bits()..self.m_end_offset().bits()
            }
            #[allow(clippy::unnecessary_cast)]
            fn m_validate(&self) -> Result<(), binparse::ParseError> {
                if !((1usize)..=((self.n() as usize).saturating_add(1usize)))
                    .contains(&(self.m() as usize))
                {
                    return Err(binparse::ParseError::ValidationFailed {
                        field: "Checked.m",
                        actual: self.m() as u128,
                    });
                }
                if !((self.m() as usize) >= (self.n() as usize)) {
                    return Err(binparse::ParseError::ValidationFailed {
                        field: "Checked.m",
                        actual: self.m() as u128,
                    });
                }
                Ok(())
            }
        }
        "#,
    );
}

#[test]
fn constant_decimal_infers_smallest_type() {
    let code = generate("struct Foo { small = 10, medium = 65536 }");
    assert!(code.contains("pub fn small(&self) -> u8"));
    assert!(code.contains("pub fn medium(&self) -> u32"));
}

#[test]
fn constant_hex_infers_type_from_width() {
    let code = generate("struct Foo { a = x0f, b = x0102030405060708 }");
    assert!(code.contains("pub fn a(&self) -> u8"));
    assert!(code.contains("pub fn b(&self) -> u64"));
}

#[test]
fn validate_attribute_is_an_alias_for_check() {
    let code = generate("struct Foo { @validate(n == 1) n: u8 }");
    assert!(code.contains("fn n_validate"));
    assert!(code.contains("me.n_validate()?"));
}

#[test]
fn constant_binary_too_wide_is_rejected() {
    let err = generate_err("struct Foo { f = b101010101 }");
    assert!(
        err.to_string()
            .contains("constant field literal width 9 is not supported")
    );
}

#[test]
fn constant_field_rejects_endian_on_single_byte() {
    let err = generate_err("struct Foo { @endian(little) f = x01 }");
    assert!(
        err.to_string()
            .contains("@endian cannot be applied to single-byte integers")
    );
}

#[test]
fn check_on_struct_ref_is_rejected() {
    let err = generate_err("struct Inner { x: u8 } struct Foo { @check(1 == 1) inner: Inner }");
    assert!(
        err.to_string()
            .contains("@check and @range can only be applied to primitive and bitfield fields")
    );
}

#[test]
fn range_on_array_is_rejected() {
    let err = generate_err("struct Foo { @range(1, 2) xs: [u8; 4] }");
    assert!(
        err.to_string()
            .contains("@check and @range can only be applied to primitive and bitfield fields")
    );
}

#[test]
fn check_with_numeric_expr_is_rejected() {
    let err = generate_err("struct Foo { @check(n + 1) n: u8 }");
    assert!(
        err.to_string()
            .contains("is a number but a boolean is required")
    );
}

#[test]
fn range_with_bool_expr_is_rejected() {
    let err = generate_err("struct Foo { n: u8, @range(n == 1, 5) m: u8 }");
    assert!(
        err.to_string()
            .contains("is a boolean but a number is required")
    );
}

#[test]
fn check_unknown_field_is_rejected() {
    let err = generate_err("struct Foo { @check(later == 1) n: u8, later: u8 }");
    assert!(
        err.to_string()
            .contains("references field 'later' which is unknown or not yet parsed")
    );
}

#[test]
fn check_wrong_arg_count_is_rejected() {
    let err = generate_err("struct Foo { @check(n == 1, n == 2) n: u8 }");
    assert!(
        err.to_string()
            .contains("@check requires exactly 1 argument(s), got 2")
    );
}

#[test]
fn range_wrong_arg_count_is_rejected() {
    let err = generate_err("struct Foo { @range(1) n: u8 }");
    assert!(
        err.to_string()
            .contains("@range requires exactly 2 argument(s), got 1")
    );
}

#[test]
fn range_min_greater_than_max_is_rejected() {
    let err = generate_err("struct Foo { @range(5, 1) n: u8 }");
    assert!(
        err.to_string()
            .contains("@range minimum 5 is greater than maximum 1")
    );
}

#[test]
fn lower_bool_rejects_numeric_logic_operand() {
    let ast = binparse_dsl_parse::parse_str("struct Foo { c = n == 1 && 2 }").unwrap();
    let err = crate::expr::lower(
        constraint_expr(&ast),
        crate::expr::ExprType::Bool,
        &numeric_done_fields(),
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("is a number but a boolean is required")
    );
}

#[test]
fn golden_conditional_fields() {
    assert_generated_eq(
        r#"
        struct Cond {
            n: u8,
            if (n == 1) {
                x: u16,
            } else {
                y: u8,
            }
            tail: u8,
        }
        "#,
        r#"
        pub struct Cond<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Cond<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.n_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                if me.conditional_0_present() {
                    {
                        let len = me.x_end_offset();
                        let expected = len.byte_ceil();
                        if data.len() < expected {
                            return Err(binparse::ParseError::NotEnoughData {
                                expected,
                                got: data.len(),
                            });
                        }
                    }
                }
                if me.conditional_0_absent() {
                    {
                        let len = me.y_end_offset();
                        let expected = len.byte_ceil();
                        if data.len() < expected {
                            return Err(binparse::ParseError::NotEnoughData {
                                expected,
                                got: data.len(),
                            });
                        }
                    }
                }
                {
                    let len = me.tail_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.tail_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn n(&self) -> u8 {
                self.data[0usize]
            }
            pub fn n_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn n_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn n_bit_range(&self) -> ::core::ops::Range<usize> {
                self.n_start_offset().bits()..self.n_end_offset().bits()
            }
            #[allow(dead_code, unused_parens)]
            fn conditional_0_present(&self) -> bool {
                ((self.n() as usize) == (1usize))
            }
            #[allow(clippy::identity_op)]
            fn x_raw(&self) -> u16 {
                u16::from_be_bytes(self.data[1usize..3usize].try_into().unwrap())
            }
            pub fn x(&self) -> Option<u16> {
                if self.conditional_0_present() { Some(self.x_raw()) } else { None }
            }
            pub fn x_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 3usize,
                    bit: 0usize,
                }
            }
            pub fn x_start_offset(&self) -> binparse::Len {
                self.n_end_offset()
            }
            pub fn x_bit_range(&self) -> ::core::ops::Range<usize> {
                self.x_start_offset().bits()..self.x_end_offset().bits()
            }
            #[allow(dead_code, unused_parens)]
            fn conditional_0_absent(&self) -> bool {
                !((self.n() as usize) == (1usize))
            }
            #[allow(clippy::identity_op)]
            fn y_raw(&self) -> u8 {
                self.data[1usize]
            }
            pub fn y(&self) -> Option<u8> {
                if self.conditional_0_absent() { Some(self.y_raw()) } else { None }
            }
            pub fn y_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
            }
            pub fn y_start_offset(&self) -> binparse::Len {
                self.n_end_offset()
            }
            pub fn y_bit_range(&self) -> ::core::ops::Range<usize> {
                self.y_start_offset().bits()..self.y_end_offset().bits()
            }
            fn conditional_0_end_offset(&self) -> binparse::Len {
                if self.conditional_0_present() {
                    self.x_end_offset()
                } else {
                    self.y_end_offset()
                }
            }
            #[allow(clippy::identity_op)]
            pub fn tail(&self) -> u8 {
                {
                    let offset = self.conditional_0_end_offset();
                    debug_assert!(offset.bit == 0, "primitive requires byte alignment");
                    self.data[offset.byte]
                }
            }
            pub fn tail_end_offset(&self) -> binparse::Len {
                ({ self.conditional_0_end_offset() })
                    + ::binparse::Len {
                        byte: 1usize,
                        bit: 0usize,
                    }
            }
            pub fn tail_start_offset(&self) -> binparse::Len {
                self.conditional_0_end_offset()
            }
            pub fn tail_bit_range(&self) -> ::core::ops::Range<usize> {
                self.tail_start_offset().bits()..self.tail_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn conditional_field_reference_is_rejected() {
    let err = generate_err("struct Foo { n: u8, if (n == 1) { m: u8 } data: [u8; m] }");
    assert!(
        err.to_string()
            .contains("expression 'm' references conditional field 'm' which may be absent")
    );
}

#[test]
fn conditional_intra_branch_reference_is_rejected() {
    let err = generate_err("struct Foo { n: u8, if (n == 1) { m: u8, data: [u8; m] } }");
    assert!(
        err.to_string()
            .contains("expression 'm' references conditional field 'm' which may be absent")
    );
}

#[test]
fn conditional_numeric_condition_is_rejected() {
    let err = generate_err("struct Foo { n: u8, if (n) { m: u8 } }");
    assert!(
        err.to_string()
            .contains("expression 'n' is a number but a boolean is required")
    );
}

#[test]
fn conditional_forward_reference_is_rejected() {
    let err = generate_err("struct Foo { if (later == 1) { m: u8 } later: u8 }");
    assert!(
        err.to_string()
            .contains("expression '(later == 1)' references field 'later' which is unknown or not yet parsed")
    );
}

#[test]
fn unsized_array_without_strategy_is_rejected() {
    let err = generate_err("struct Foo { data: [u8] }");
    assert!(
        err.to_string()
            .contains("array without size requires @until, @greedy, or @hook")
    );
}

#[test]
fn until_on_sized_array_is_rejected() {
    let err = generate_err("struct Foo { @until(x00) data: [u8; 4] }");
    assert!(
        err.to_string()
            .contains("@until requires an array without an explicit size")
    );
}

#[test]
fn greedy_on_sized_array_is_rejected() {
    let err = generate_err("struct Foo { @greedy(unsafe_eof) data: [u8; 4] }");
    assert!(
        err.to_string()
            .contains("@greedy requires an array without an explicit size")
    );
}

#[test]
fn until_on_non_array_is_rejected() {
    let err = generate_err("struct Foo { @until(x00) data: u8 }");
    assert!(
        err.to_string()
            .contains("@until can only be applied to array fields")
    );
}

#[test]
fn max_iter_on_non_array_is_rejected() {
    let err = generate_err("struct Foo { @max_iter(4) data: u8 }");
    assert!(
        err.to_string()
            .contains("@max_iter can only be applied to array fields")
    );
}

#[test]
fn until_with_greedy_is_rejected() {
    let err = generate_err("struct Foo { @until(x00) @greedy(unsafe_eof) data: [u8] }");
    assert!(
        err.to_string()
            .contains("@until and @greedy cannot be combined")
    );
}

#[test]
fn greedy_with_hook_is_rejected() {
    let err = generate_err("struct Foo { @hook(f, u8) @greedy(unsafe_eof) data: [u8] }");
    assert!(
        err.to_string()
            .contains("@greedy cannot be combined with @hook")
    );
}

#[test]
fn hook_on_non_u8_vla_is_rejected() {
    let err = generate_err("struct Foo { @hook(f, u8) data: [u16] }");
    assert!(err.to_string().contains("@hook on VLA requires [u8] type"));
}

#[test]
fn invalid_greedy_value_is_rejected() {
    let err = generate_err("struct Foo { @greedy(eof) data: [u8] }");
    assert!(
        err.to_string()
            .contains("@greedy argument must be 'unsafe_eof', got 'eof'")
    );
}

#[test]
fn until_sentinel_too_wide_is_rejected() {
    let err = generate_err("struct Foo { @until(x0100) data: [u8] }");
    assert!(
        err.to_string()
            .contains("@until sentinel must be an integer literal fitting in one byte")
    );
}

#[test]
fn greedy_dynamic_elem_without_max_iter_is_rejected() {
    let err = generate_err(
        "struct Opt { kind: u8, if (kind > 0) { body: u8 } } struct Foo { @greedy(unsafe_eof) opts: [Opt] }",
    );
    assert!(
        err.to_string()
            .contains("@greedy with dynamic-length elements requires @max_iter")
    );
}

#[test]
fn greedy_zero_sized_elem_is_rejected() {
    let err = generate_err("struct Empty { } struct Foo { @greedy(unsafe_eof) xs: [Empty] }");
    assert!(err.to_string().contains("@greedy element type has zero length"));
}

#[test]
fn golden_padding_and_alignment() {
    assert_generated_eq(
        "struct Padded { a: u8, @pad(2) b: u8, @pad_to(4) c: u16, @align(2) d: u16 }",
        r#"
        pub struct Padded<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Padded<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.a_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.b_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.c_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.d_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.d_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn a(&self) -> u8 {
                self.data[0usize]
            }
            pub fn a_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn a_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn a_bit_range(&self) -> ::core::ops::Range<usize> {
                self.a_start_offset().bits()..self.a_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn b(&self) -> u8 {
                self.data[3usize]
            }
            pub fn b_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 4usize,
                    bit: 0usize,
                }
            }
            pub fn b_start_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 3usize,
                    bit: 0usize,
                }
            }
            pub fn b_bit_range(&self) -> ::core::ops::Range<usize> {
                self.b_start_offset().bits()..self.b_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn c(&self) -> u16 {
                u16::from_be_bytes(self.data[4usize..6usize].try_into().unwrap())
            }
            pub fn c_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 6usize,
                    bit: 0usize,
                }
            }
            pub fn c_start_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 4usize,
                    bit: 0usize,
                }
            }
            pub fn c_bit_range(&self) -> ::core::ops::Range<usize> {
                self.c_start_offset().bits()..self.c_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn d(&self) -> u16 {
                u16::from_be_bytes(self.data[6usize..8usize].try_into().unwrap())
            }
            pub fn d_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 8usize,
                    bit: 0usize,
                }
            }
            pub fn d_start_offset(&self) -> binparse::Len {
                self.c_end_offset()
            }
            pub fn d_bit_range(&self) -> ::core::ops::Range<usize> {
                self.d_start_offset().bits()..self.d_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn golden_skip_fields() {
    assert_generated_eq(
        "struct Skipped { @skip reserved: b<3>, flags: b<5>, pair: concat(b<4>, @skip b<4>) }",
        r#"
        pub struct Skipped<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Skipped<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.reserved_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.flags_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.pair_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.pair_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(dead_code)]
            #[allow(clippy::identity_op)]
            fn reserved(&self) -> u8 {
                (self.data[0usize] >> 5usize) & 7u8
            }
            #[allow(dead_code)]
            fn reserved_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 0usize,
                    bit: 3usize,
                }
            }
            #[allow(dead_code)]
            fn reserved_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            #[allow(dead_code)]
            fn reserved_bit_range(&self) -> ::core::ops::Range<usize> {
                self.reserved_start_offset().bits()..self.reserved_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn flags(&self) -> u8 {
                (self.data[0usize] >> 0usize) & 31u8
            }
            pub fn flags_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn flags_start_offset(&self) -> binparse::Len {
                self.reserved_end_offset()
            }
            pub fn flags_bit_range(&self) -> ::core::ops::Range<usize> {
                self.flags_start_offset().bits()..self.flags_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn pair_0(&self) -> u8 {
                (self.data[1usize] >> 4usize) & 15u8
            }
            #[allow(dead_code)]
            #[allow(clippy::identity_op)]
            fn pair_1(&self) -> u8 {
                (self.data[1usize] >> 0usize) & 15u8
            }
            #[allow(clippy::identity_op)]
            pub fn pair(&self) -> (u8,) {
                (self.pair_0(),)
            }
            pub fn pair_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
            }
            pub fn pair_start_offset(&self) -> binparse::Len {
                self.flags_end_offset()
            }
            pub fn pair_bit_range(&self) -> ::core::ops::Range<usize> {
                self.pair_start_offset().bits()..self.pair_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn align_on_misaligned_fixed_offset_is_rejected() {
    let err = generate_err("struct Foo { a: u8, @align(2) b: u8 }");
    assert!(
        err.to_string()
            .contains("@align(2) field starts at misaligned offset")
    );
}

#[test]
fn align_on_unaligned_bit_offset_is_rejected() {
    let err = generate_err("struct Foo { a: b<3>, @align(1) b: u8 }");
    assert!(
        err.to_string()
            .contains("@align(1) field starts at misaligned offset")
    );
}

#[test]
fn pad_with_pad_to_is_rejected() {
    let err = generate_err("struct Foo { @pad(1) @pad_to(4) a: u8 }");
    assert!(
        err.to_string()
            .contains("@pad and @pad_to cannot be combined")
    );
}

#[test]
fn zero_padding_arg_is_rejected() {
    let err = generate_err("struct Foo { @align(0) a: u8 }");
    assert!(
        err.to_string()
            .contains("@align argument must be a positive integer literal")
    );
}

#[test]
fn non_literal_padding_arg_is_rejected() {
    let err = generate_err("struct Foo { n: u8, @pad(n) a: u8 }");
    assert!(
        err.to_string()
            .contains("@pad argument must be a positive integer literal")
    );
}

#[test]
fn skip_with_args_is_rejected() {
    let err = generate_err("struct Foo { @skip(1) a: u8 }");
    assert!(
        err.to_string()
            .contains("@skip requires exactly 0 argument(s), got 1")
    );
}

#[test]
fn pad_wrong_arg_count_is_rejected() {
    let err = generate_err("struct Foo { @pad_to(1, 2) a: u8 }");
    assert!(
        err.to_string()
            .contains("@pad_to requires exactly 1 argument(s), got 2")
    );
}

#[test]
fn golden_len_bounded_struct_ref() {
    assert_generated_eq(
        "struct Inner { a: u8, b: u16 }
         struct Tlv { tag: u8, len: u8, @len(len) value: Inner, after: u8 }",
        r#"
        pub struct Inner<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Inner<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.a_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.b_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.b_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn a(&self) -> u8 {
                self.data[0usize]
            }
            pub fn a_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn a_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn a_bit_range(&self) -> ::core::ops::Range<usize> {
                self.a_start_offset().bits()..self.a_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn b(&self) -> u16 {
                u16::from_be_bytes(self.data[1usize..3usize].try_into().unwrap())
            }
            pub fn b_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 3usize,
                    bit: 0usize,
                }
            }
            pub fn b_start_offset(&self) -> binparse::Len {
                self.a_end_offset()
            }
            pub fn b_bit_range(&self) -> ::core::ops::Range<usize> {
                self.b_start_offset().bits()..self.b_end_offset().bits()
            }
        }
        pub struct Tlv<'a> {
            #[allow(dead_code)]
            data: &'a [u8],
        }
        impl<'a> Tlv<'a> {
            pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
                let me = Self { data };
                {
                    let len = me.tag_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.len_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.value_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                {
                    let len = me.after_end_offset();
                    let expected = len.byte_ceil();
                    if data.len() < expected {
                        return Err(binparse::ParseError::NotEnoughData {
                            expected,
                            got: data.len(),
                        });
                    }
                }
                let len = me.after_end_offset();
                if len.bit != 0 {
                    return Err(binparse::ParseError::UnalignedLength(len));
                }
                if data.len() < len.byte {
                    return Err(binparse::ParseError::NotEnoughData {
                        expected: len.byte,
                        got: data.len(),
                    });
                }
                Ok((me, &data[len.byte..]))
            }
            #[allow(clippy::identity_op)]
            pub fn tag(&self) -> u8 {
                self.data[0usize]
            }
            pub fn tag_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 1usize,
                    bit: 0usize,
                }
            }
            pub fn tag_start_offset(&self) -> binparse::Len {
                binparse::Len::ZERO
            }
            pub fn tag_bit_range(&self) -> ::core::ops::Range<usize> {
                self.tag_start_offset().bits()..self.tag_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn len(&self) -> u8 {
                self.data[1usize]
            }
            pub fn len_end_offset(&self) -> binparse::Len {
                binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
            }
            pub fn len_start_offset(&self) -> binparse::Len {
                self.tag_end_offset()
            }
            pub fn len_bit_range(&self) -> ::core::ops::Range<usize> {
                self.len_start_offset().bits()..self.len_end_offset().bits()
            }
            pub fn value_rest(&self) -> ::binparse::ParseResult<&[u8]> {
                let start = 2usize;
                let end = start.saturating_add(self.len() as usize);
                Inner::parse(&self.data[start..end]).map(|(_, rest)| rest)
            }
            #[allow(clippy::identity_op)]
            pub fn value(&self) -> ::binparse::ParseResult<Inner<'_>> {
                let start = 2usize;
                let end = start.saturating_add(self.len() as usize);
                Inner::parse(&self.data[start..end]).map(|(value, _)| value)
            }
            pub fn value_end_offset(&self) -> binparse::Len {
                ::binparse::Len {
                    byte: 2usize,
                    bit: 0usize,
                }
                    + ({
                        ::binparse::Len {
                            byte: self.len() as usize,
                            bit: 0,
                        }
                    })
            }
            pub fn value_start_offset(&self) -> binparse::Len {
                self.len_end_offset()
            }
            pub fn value_bit_range(&self) -> ::core::ops::Range<usize> {
                self.value_start_offset().bits()..self.value_end_offset().bits()
            }
            #[allow(clippy::identity_op)]
            pub fn after(&self) -> u8 {
                {
                    let offset = ::binparse::Len {
                        byte: 2usize,
                        bit: 0usize,
                    }
                        + ({
                            ::binparse::Len {
                                byte: self.len() as usize,
                                bit: 0,
                            }
                        });
                    debug_assert!(offset.bit == 0, "primitive requires byte alignment");
                    self.data[offset.byte]
                }
            }
            pub fn after_end_offset(&self) -> binparse::Len {
                ({
                    ::binparse::Len {
                        byte: 2usize,
                        bit: 0usize,
                    }
                        + ({
                            ::binparse::Len {
                                byte: self.len() as usize,
                                bit: 0,
                            }
                        })
                })
                    + ::binparse::Len {
                        byte: 1usize,
                        bit: 0usize,
                    }
            }
            pub fn after_start_offset(&self) -> binparse::Len {
                self.value_end_offset()
            }
            pub fn after_bit_range(&self) -> ::core::ops::Range<usize> {
                self.after_start_offset().bits()..self.after_end_offset().bits()
            }
        }
        "#,
    );
}

#[test]
fn len_on_primitive_is_rejected() {
    let err = generate_err("struct Foo { n: u8, @len(n) a: u8 }");
    assert!(
        err.to_string()
            .contains("@len can only be applied to struct ref fields")
    );
}

#[test]
fn len_on_bitfield_is_rejected() {
    let err = generate_err("struct Foo { n: u8, @len(n) a: b<4> }");
    assert!(
        err.to_string()
            .contains("@len can only be applied to struct ref fields")
    );
}

#[test]
fn len_on_constant_field_is_rejected() {
    let err = generate_err("struct Foo { n: u8, @len(n) magic = xff }");
    assert!(
        err.to_string()
            .contains("@len can only be applied to struct ref fields")
    );
}

#[test]
fn len_wrong_arg_count_is_rejected() {
    let err = generate_err(
        "struct Inner { a: u8 }
         struct Foo { n: u8, @len(n, 2) value: Inner }",
    );
    assert!(
        err.to_string()
            .contains("@len requires exactly 1 argument(s), got 2")
    );
}

#[test]
fn len_bound_smaller_than_fixed_inner_is_rejected() {
    let err = generate_err(
        "struct Inner { a: u32 }
         struct Foo { @len(2) value: Inner }",
    );
    assert!(
        err.to_string()
            .contains("@len(2) is smaller than the referenced struct's fixed length of 4 bytes")
    );
}

#[test]
fn len_unknown_field_is_rejected() {
    let err = generate_err(
        "struct Inner { a: u8 }
         struct Foo { @len(nope) value: Inner }",
    );
    assert!(err.to_string().contains("references field 'nope'"));
}

#[test]
fn len_bound_equal_to_fixed_inner_is_accepted() {
    let code = generate(
        "struct Inner { a: u32 }
         struct Foo { @len(4) value: Inner }",
    );
    assert!(code.contains("fn value_rest"));
}
