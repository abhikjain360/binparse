#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Len {
    pub byte: usize,
    pub bit: usize,
}

impl std::ops::Add for Len {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        let mut byte = self.byte + other.byte;
        let mut bit = self.bit + other.bit;
        if bit >= 8 {
            byte += 1;
            bit -= 8;
        }
        Self { byte, bit }
    }
}
