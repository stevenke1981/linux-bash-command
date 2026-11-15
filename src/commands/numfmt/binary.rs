//! The 64-bit significand used by x86 GNU long double, implemented with integers.
//! Rounding each operation avoids depending on the host's floating-point ABI.
use super::number::Round;
#[derive(Clone, Copy)]
pub(super) struct Binary {
    mantissa: u128,
    exponent: i32,
    pub negative: bool,
}
impl Binary {
    pub fn ratio(numerator: u128, denominator: u128) -> Self {
        if numerator == 0 {
            return Self {
                mantissa: 0,
                exponent: 0,
                negative: false,
            };
        }
        let mut exponent = denominator.leading_zeros() as i32 - numerator.leading_zeros() as i32;
        if if exponent >= 0 {
            numerator < denominator << exponent
        } else {
            numerator << -exponent < denominator
        } {
            exponent -= 1
        }
        let (n, d) = if exponent >= 0 {
            (numerator, denominator << exponent)
        } else {
            (numerator << -exponent, denominator)
        };
        let mut remainder = n - d;
        let mut mantissa = 1u128;
        for _ in 0..63 {
            mantissa <<= 1;
            if remainder >= d - remainder {
                remainder -= d - remainder;
                mantissa += 1
            } else {
                remainder *= 2
            }
        }
        if remainder > d - remainder || (remainder == d - remainder && mantissa & 1 != 0) {
            mantissa += 1;
            if mantissa == 1u128 << 64 {
                mantissa >>= 1;
                exponent += 1
            }
        }
        Self {
            mantissa,
            exponent: exponent - 63,
            negative: false,
        }
    }
    pub fn integer(value: i128) -> Self {
        Self::ratio(value.unsigned_abs(), 1)
    }
    pub fn multiply(self, other: Self) -> Self {
        let mut value = Self::ratio(self.mantissa * other.mantissa, 1);
        value.exponent += self.exponent + other.exponent;
        value.negative = self.negative ^ other.negative;
        value
    }
    pub fn divide(self, other: Self) -> Self {
        let mut value = Self::ratio(self.mantissa, other.mantissa);
        value.exponent += self.exponent - other.exponent;
        value.negative = self.negative ^ other.negative;
        value
    }
    pub fn rounded(self, round: Round) -> Result<i128, String> {
        if self.mantissa == 0 {
            return Ok(0);
        }
        let (whole, remainder, half) = if self.exponent >= 0 {
            (
                self.mantissa
                    .checked_shl(self.exponent as u32)
                    .ok_or("value too large to be printed")?,
                false,
                false,
            )
        } else {
            let bits = (-self.exponent) as u32;
            if bits >= 128 {
                (0, true, false)
            } else {
                let mask = (1u128 << bits) - 1;
                let r = self.mantissa & mask;
                (self.mantissa >> bits, r != 0, r >= 1u128 << (bits - 1))
            }
        };
        let increase = match round {
            Round::Up => !self.negative && remainder,
            Round::Down => self.negative && remainder,
            Round::Away => remainder,
            Round::Towards => false,
            Round::Nearest => half,
        };
        let magnitude = i128::try_from(whole + u128::from(increase))
            .map_err(|_| "value too large to be printed")?;
        Ok(if self.negative { -magnitude } else { magnitude })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decimal_scale_rounding_matches_extended_precision() {
        let value = Binary::ratio(120, 100).multiply(Binary::integer(100));
        assert_eq!(value.rounded(Round::Away).unwrap(), 121);
    }
    #[test]
    fn unit_multiplication_rounds_each_step() {
        let value = Binary::ratio(120, 100)
            .multiply(Binary::integer(3))
            .multiply(Binary::integer(100));
        assert_eq!(value.rounded(Round::Away).unwrap(), 360);
    }
    #[test]
    fn negative_ceiling() {
        let mut value = Binary::ratio(1, 2);
        value.negative = true;
        assert_eq!(value.rounded(Round::Up).unwrap(), 0);
    }
}
