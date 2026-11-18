//! Exact unsigned amounts shared by offline chain codecs. No secrets or I/O.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidAmount,
    Overflow,
    InvalidAddress,
    InvalidEncoding,
    UnsupportedTransfer,
    WrongNetwork,
    InvalidSignature,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

/// Unsigned 256-bit base-unit amount. No floating point or silent truncation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Amount([u8; 32]);
impl Amount {
    pub const ZERO: Self = Self([0; 32]);
    pub const fn from_be_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub const fn to_be_bytes(self) -> [u8; 32] {
        self.0
    }
    pub fn checked_add(self, other: Self) -> Result<Self> {
        let mut result = [0; 32];
        let mut carry = 0u16;
        for index in (0..32).rev() {
            let value = self.0[index] as u16 + other.0[index] as u16 + carry;
            result[index] = value as u8;
            carry = value >> 8;
        }
        if carry != 0 {
            return Err(Error::Overflow);
        }
        Ok(Self(result))
    }
    pub fn checked_sub(self, other: Self) -> Result<Self> {
        let mut result = [0; 32];
        let mut borrow = 0i16;
        for index in (0..32).rev() {
            let value = self.0[index] as i16 - other.0[index] as i16 - borrow;
            result[index] = value as u8;
            borrow = i16::from(value < 0);
        }
        if borrow != 0 { return Err(Error::Overflow); }
        Ok(Self(result))
    }
    pub fn checked_mul_u64(self, multiplier: u64) -> Result<Self> {
        let mut result = [0; 32];
        let mut carry = 0u128;
        for index in (0..32).rev() {
            let value = self.0[index] as u128 * multiplier as u128 + carry;
            result[index] = value as u8;
            carry = value >> 8;
        }
        if carry != 0 {
            return Err(Error::Overflow);
        }
        Ok(Self(result))
    }
    pub fn from_u64(value: u64) -> Self {
        let mut bytes = [0; 32];
        bytes[24..].copy_from_slice(&value.to_be_bytes());
        Self(bytes)
    }
    pub fn to_u64(self) -> Result<u64> {
        if self.0[..24].iter().any(|b| *b != 0) {
            return Err(Error::Overflow);
        }
        Ok(u64::from_be_bytes(
            self.0[24..].try_into().expect("fixed width"),
        ))
    }
    pub fn parse_units(text: &str, decimals: u8) -> Result<Self> {
        // 78 digits suffice for uint256; fractional scale is bounded as well.
        if decimals > 77 || text.is_empty() || text.len() > 157 {
            return Err(Error::InvalidAmount);
        }
        let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
        if whole.is_empty()
            || (text.contains('.') && fraction.is_empty())
            || (whole.len() > 1 && whole.starts_with('0'))
            || fraction.len() > decimals as usize
            || !whole
                .bytes()
                .chain(fraction.bytes())
                .all(|b| b.is_ascii_digit())
        {
            return Err(Error::InvalidAmount);
        }
        let mut amount = Self::ZERO;
        for digit in whole
            .bytes()
            .chain(fraction.bytes())
            .map(|b| b - b'0')
            .chain(std::iter::repeat_n(0, decimals as usize - fraction.len()))
        {
            let mut carry = digit as u16;
            for byte in amount.0.iter_mut().rev() {
                let value = *byte as u16 * 10 + carry;
                *byte = value as u8;
                carry = value >> 8;
            }
            if carry != 0 {
                return Err(Error::Overflow);
            }
        }
        Ok(amount)
    }
    pub fn format_units(self, decimals: u8) -> Result<String> {
        if decimals > 77 {
            return Err(Error::InvalidAmount);
        }
        let mut bytes = self.0;
        let mut digits = Vec::new();
        loop {
            let mut remainder = 0u16;
            for byte in &mut bytes {
                let value = remainder * 256 + *byte as u16;
                *byte = (value / 10) as u8;
                remainder = value % 10;
            }
            digits.push((b'0' + remainder as u8) as char);
            if bytes.iter().all(|b| *b == 0) {
                break;
            }
        }
        while digits.len() <= decimals as usize {
            digits.push('0');
        }
        digits.reverse();
        if decimals != 0 {
            digits.insert(digits.len() - decimals as usize, '.');
        }
        Ok(digits.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn send_all_subtracts_fee_exactly() {
        let balance = Amount::parse_units("0.007981020000000001", 18).unwrap();
        let fee = Amount::from_u64(1_000_000_000).checked_mul_u64(21000).unwrap();
        let send = balance.checked_sub(fee).unwrap();
        assert_eq!(send, Amount::parse_units("0.007960020000000001",18).unwrap());
        assert_eq!(send.checked_add(fee).unwrap(), balance);
        assert_eq!(fee.checked_sub(fee).unwrap(), Amount::ZERO);
        assert!(Amount::ZERO.checked_sub(fee).is_err());
        let max = Amount::from_be_bytes([255;32]);
        assert_eq!(max.checked_sub(fee).unwrap().checked_add(fee).unwrap(),max);
    }
    #[test]
    fn fee_math_overflow_is_rejected() {
        assert_eq!(
            Amount::from_u64(1_000_000_000)
                .checked_mul_u64(21000)
                .unwrap()
                .to_u64()
                .unwrap(),
            21_000_000_000_000
        );
        let max = Amount::from_be_bytes([255; 32]);
        assert_eq!(max.checked_add(Amount::from_u64(1)), Err(Error::Overflow));
        assert_eq!(max.checked_mul_u64(2), Err(Error::Overflow));
        assert_eq!(max.checked_mul_u64(0).unwrap(), Amount::ZERO);
        assert_eq!(max.checked_mul_u64(1).unwrap(), max);
    }
    #[test]
    fn exact_decimals_and_extremes() {
        assert_eq!(
            Amount::parse_units("0.000001", 6).unwrap(),
            Amount::from_u64(1)
        );
        assert_eq!(
            Amount::from_u64(1).format_units(18).unwrap(),
            "0.000000000000000001"
        );
        assert_eq!(Amount::ZERO.format_units(6).unwrap(), "0.000000");
        let max = "115792089237316195423570985008687907853269984665640564039457584007913129639935";
        let value = Amount::parse_units(max, 0).unwrap();
        assert_eq!(value.to_be_bytes(), [255; 32]);
        assert_eq!(value.format_units(0).unwrap(), max);
        assert_eq!(
            Amount::parse_units(&max.replace("935", "936"), 0),
            Err(Error::Overflow)
        );
        assert_eq!(value.to_u64(), Err(Error::Overflow));
    }
    #[test]
    fn rejects_ambiguous_or_lossy_input() {
        for text in [
            "",
            " 1",
            "1 ",
            "-1",
            "+1",
            "1e3",
            "1,2",
            ".1",
            "1.",
            "00",
            "01",
            "1.0000001",
            "１",
            "1.2.3",
        ] {
            assert!(Amount::parse_units(text, 6).is_err(), "{text}");
        }
        assert!(Amount::parse_units("1", 78).is_err());
    }
    #[test]
    fn decimal_roundtrip_across_scales() {
        for scale in [0, 6, 18, 77] {
            for value in [0, 1, 255, 256, u32::MAX as u64, u64::MAX] {
                let amount = Amount::from_u64(value);
                assert_eq!(
                    Amount::parse_units(&amount.format_units(scale).unwrap(), scale).unwrap(),
                    amount
                );
            }
        }
    }
}
