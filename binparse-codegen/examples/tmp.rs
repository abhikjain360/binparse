pub struct LittleEndianPacket<'a> {
    #[allow(dead_code)]
    data: &'a [u8],
}
impl<'a> LittleEndianPacket<'a> {
    pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
        let me = Self { data };
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
}
#[allow(non_camel_case_types)]
pub struct MyPacket_payload_Echo<'a> {
    #[allow(dead_code)]
    data: &'a [u8],
}
impl<'a> MyPacket_payload_Echo<'a> {
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
    #[allow(clippy::identity_op)]
    pub fn seq(&self) -> u16 {
        u16::from_be_bytes(self.data[2usize..4usize].try_into().unwrap())
    }
    pub fn seq_end_offset(&self) -> binparse::Len {
        binparse::Len {
            byte: 4usize,
            bit: 0usize,
        }
    }
}
#[allow(non_camel_case_types)]
pub struct MyPacket_payload_DestUnreachable<'a> {
    #[allow(dead_code)]
    data: &'a [u8],
}
impl<'a> MyPacket_payload_DestUnreachable<'a> {
    #[allow(clippy::identity_op)]
    pub fn unused(&self) -> u32 {
        u32::from_le_bytes(self.data[0usize..4usize].try_into().unwrap())
    }
    pub fn unused_end_offset(&self) -> binparse::Len {
        binparse::Len {
            byte: 4usize,
            bit: 0usize,
        }
    }
}
#[allow(non_camel_case_types)]
pub struct MyPacket_payload_TimeExceeded<'a> {
    #[allow(dead_code)]
    data: &'a [u8],
}
impl<'a> MyPacket_payload_TimeExceeded<'a> {
    #[allow(clippy::identity_op)]
    pub fn unused(&self) -> u32 {
        u32::from_be_bytes(self.data[0usize..4usize].try_into().unwrap())
    }
    pub fn unused_end_offset(&self) -> binparse::Len {
        binparse::Len {
            byte: 4usize,
            bit: 0usize,
        }
    }
}
#[allow(non_camel_case_types)]
pub struct MyPacket_payload_Unknown<'a> {
    #[allow(dead_code)]
    data: &'a [u8],
}
impl<'a> MyPacket_payload_Unknown<'a> {}
#[allow(non_camel_case_types)]
pub enum MyPacket_payload<'a> {
    Echo(MyPacket_payload_Echo<'a>),
    DestUnreachable(MyPacket_payload_DestUnreachable<'a>),
    TimeExceeded(MyPacket_payload_TimeExceeded<'a>),
    Unknown(MyPacket_payload_Unknown<'a>),
}
pub struct MyPacket<'a> {
    #[allow(dead_code)]
    data: &'a [u8],
}
impl<'a> MyPacket<'a> {
    pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
        let me = Self { data };
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
    #[allow(clippy::identity_op)]
    pub fn checksum(&self) -> u16 {
        u16::from_be_bytes(self.data[2usize..4usize].try_into().unwrap())
    }
    pub fn checksum_end_offset(&self) -> binparse::Len {
        binparse::Len {
            byte: 4usize,
            bit: 0usize,
        }
    }
    #[allow(clippy::identity_op)]
    pub fn something(&self) -> u16 {
        u16::from_be_bytes(self.data[4usize..6usize].try_into().unwrap())
    }
    pub fn something_end_offset(&self) -> binparse::Len {
        binparse::Len {
            byte: 6usize,
            bit: 0usize,
        }
    }
    #[allow(clippy::identity_op)]
    pub fn conct_0(&self) -> u16 {
        u16::from_be_bytes(self.data[6usize..8usize].try_into().unwrap())
    }
    #[allow(clippy::identity_op)]
    pub fn conct_1(&self) -> u16 {
        u16::from_be_bytes(self.data[8usize..10usize].try_into().unwrap())
    }
    #[allow(clippy::identity_op)]
    pub fn conct(&self) -> (u16, u16) {
        (self.conct_0(), self.conct_1())
    }
    pub fn conct_end_offset(&self) -> binparse::Len {
        binparse::Len {
            byte: 10usize,
            bit: 0usize,
        }
    }
    #[allow(clippy::identity_op)]
    pub fn payload(&self) -> MyPacket_payload<'_> {
        match (self.ty(), self.something()) {
            (0, 0) | (0, 8) => MyPacket_payload::Echo(MyPacket_payload_Echo {
                data: &self.data[10usize..],
            }),
            (3, 0) => MyPacket_payload::DestUnreachable(MyPacket_payload_DestUnreachable {
                data: &self.data[10usize..],
            }),
            (11, 2) => MyPacket_payload::TimeExceeded(MyPacket_payload_TimeExceeded {
                data: &self.data[10usize..],
            }),
            _ => MyPacket_payload::Unknown(MyPacket_payload_Unknown {
                data: &self.data[10usize..],
            }),
        }
    }
    pub fn payload_end_offset(&self) -> binparse::Len {
        ::binparse::Len {
            byte: 10usize,
            bit: 0usize,
        } + (match (self.ty(), self.something()) {
            (0, 0) | (0, 8) => ::binparse::Len {
                byte: 4usize,
                bit: 0,
            },
            (3, 0) => ::binparse::Len {
                byte: 4usize,
                bit: 0,
            },
            (11, 2) => ::binparse::Len {
                byte: 4usize,
                bit: 0,
            },
            _ => ::binparse::Len {
                byte: 0usize,
                bit: 0,
            },
        })
    }
}
pub struct BigEndianPacket<'a> {
    #[allow(dead_code)]
    data: &'a [u8],
}
impl<'a> BigEndianPacket<'a> {
    pub fn parse(data: &'a [u8]) -> Result<(Self, &'a [u8]), binparse::ParseError> {
        let me = Self { data };
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
    pub fn value(&self) -> u64 {
        u64::from_be_bytes(self.data[0usize..8usize].try_into().unwrap())
    }
    pub fn value_end_offset(&self) -> binparse::Len {
        binparse::Len {
            byte: 8usize,
            bit: 0usize,
        }
    }
}

fn main() {
    todo!();
}
