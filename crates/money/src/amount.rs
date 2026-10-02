//! Fixed-point [`Amount`].

#[cfg(feature = "serde")]
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Fixed-point scale — fractional decimal digit count.
const SCALE: u32 = 9;

/// The scale factor (`10^SCALE`): smallest units per whole amount.
const SCALE_FACTOR: u64 = 10u64.pow(SCALE);

/// Fixed-point amount: an `i64` count of 10⁻⁹ units.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Amount(i64);

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ParseAmountError {
    #[error("invalid amount {value:?}")]
    Invalid { value: String },
    #[error("amount {value:?} has more than {} decimal places", SCALE)]
    TooManyDecimals { value: String },
}

impl Amount {
    /// Zero.
    pub const ZERO: Amount = Amount::new(0, 0);

    /// The lowest representable amount.
    pub const MIN: Amount = Amount(i64::MIN);

    /// The greatest representable amount.
    pub const MAX: Amount = Amount(i64::MAX);

    /// The raw 10⁻⁹ units this amount counts.
    pub const fn units(self) -> i64 {
        self.0
    }

    /// From raw 10⁻⁹ units — the inverse of `units`.
    pub const fn from_units(units: i64) -> Amount {
        Amount(units)
    }

    /// From the significant digits and their scale:
    /// - `Amount::new(24906, 2)` = 249.06
    /// - `Amount::new(249, 0)` = 249
    /// - `Amount::new(6, 2)` = 0.06
    pub const fn new(mantissa: i64, scale: u32) -> Amount {
        Amount::try_new(mantissa, scale).expect("amount is not representable in 10⁻⁹ units")
    }

    pub const fn try_new(mantissa: i64, scale: u32) -> Option<Amount> {
        if scale > SCALE {
            return None;
        }

        let units = (mantissa as i128) * 10i128.pow(SCALE - scale);
        if units > i64::MAX as i128 || units < i64::MIN as i128 {
            return None;
        }

        Some(Amount(units as i64))
    }

    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// Absolute value. `Amount::MIN` has none — panics in debug builds.
    pub const fn abs(self) -> Amount {
        Amount(self.0.abs())
    }

    /// Exact addition, `None` on overflow.
    pub const fn checked_add(self, other: Amount) -> Option<Amount> {
        match self.0.checked_add(other.0) {
            Some(units) => Some(Amount(units)),
            None => None,
        }
    }

    /// `self × numerator / denominator`, truncated toward zero.
    /// `None` if the result exceeds `i64` or `denominator` is zero.
    pub const fn checked_mul_div(self, numerator: i64, denominator: i64) -> Option<Amount> {
        if denominator == 0 {
            return None;
        }

        let product = (self.0 as i128) * (numerator as i128);
        let scaled = product / (denominator as i128);
        if scaled > i64::MAX as i128 || scaled < i64::MIN as i128 {
            return None;
        }

        Some(Amount(scaled as i64))
    }

    /// Round to `dp` decimal places, half away from zero. `dp ≤ SCALE`.
    pub const fn round_dp(self, dp: u32) -> Amount {
        Amount::try_round_dp(self, dp).expect("rounding beyond the fixed-point scale")
    }

    pub const fn try_round_dp(self, dp: u32) -> Option<Amount> {
        if dp > SCALE {
            return None;
        }

        let units = self.0;
        let granularity = 10i64.pow(SCALE - dp);
        let half = units.signum() * (granularity / 2);
        let Some(shifted) = units.checked_add(half) else {
            return None;
        };

        Some(Amount(shifted / granularity * granularity))
    }

    /// f64 view for scoring — lossy.
    pub const fn to_f64(self) -> f64 {
        self.0 as f64 / SCALE_FACTOR as f64
    }
}

impl Default for Amount {
    fn default() -> Self {
        Amount::ZERO
    }
}

impl std::fmt::Debug for Amount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

impl std::fmt::Display for Amount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        let int = abs / SCALE_FACTOR;
        let frac = abs % SCALE_FACTOR;
        if frac == 0 {
            write!(f, "{sign}{int}")
        } else {
            let frac = format!("{frac:0width$}", width = SCALE as usize);
            let frac = frac.trim_end_matches('0');
            write!(f, "{sign}{int}.{frac}")
        }
    }
}

impl std::str::FromStr for Amount {
    type Err = ParseAmountError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || ParseAmountError::Invalid {
            value: s.to_string(),
        };

        let (neg, rest) = s.strip_prefix('-').map_or((false, s), |r| (true, r));
        let (int_part, frac_part) = rest.split_once('.').unwrap_or((rest, ""));
        if int_part.is_empty() && frac_part.is_empty() {
            return Err(invalid());
        }

        if frac_part.len() > SCALE as usize {
            return Err(ParseAmountError::TooManyDecimals {
                value: s.to_string(),
            });
        }

        let part = |digits: &str| {
            if digits.is_empty() {
                Ok(0)
            } else {
                digits.parse::<i64>().map_err(|_| invalid())
            }
        };

        let int = part(int_part)?;
        let frac = part(frac_part)?;
        let scale = frac_part.len() as u32;
        let mantissa = (int as i128) * 10i128.pow(scale) + frac as i128;
        let mantissa =
            i64::try_from(if neg { -mantissa } else { mantissa }).map_err(|_| invalid())?;

        Amount::try_new(mantissa, scale).ok_or_else(invalid)
    }
}

#[cfg(feature = "serde")]
impl Serialize for Amount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for Amount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct AmountVisitor;

        impl serde::de::Visitor<'_> for AmountVisitor {
            type Value = Amount;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a decimal amount as a string or a number")
            }

            fn visit_str<E>(self, v: &str) -> Result<Amount, E>
            where
                E: serde::de::Error,
            {
                v.parse().map_err(serde::de::Error::custom)
            }

            fn visit_i64<E>(self, v: i64) -> Result<Amount, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(&v.to_string())
            }

            fn visit_u64<E>(self, v: u64) -> Result<Amount, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(&v.to_string())
            }

            fn visit_f64<E>(self, v: f64) -> Result<Amount, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(&v.to_string())
            }
        }

        deserializer.deserialize_any(AmountVisitor)
    }
}

#[cfg(feature = "rusqlite")]
impl rusqlite::types::ToSql for Amount {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Integer(self.units()),
        ))
    }
}

#[cfg(feature = "rusqlite")]
impl rusqlite::types::FromSql for Amount {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        value.as_i64().map(Amount::from_units)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decimal_strings_exactly() {
        assert_eq!("249.06".parse::<Amount>().unwrap().to_string(), "249.06");
        assert_eq!("2".parse::<Amount>().unwrap().to_string(), "2");
        assert_eq!(
            "0.000001".parse::<Amount>().unwrap().to_string(),
            "0.000001"
        );
        assert_eq!("-1.5".parse::<Amount>().unwrap().to_string(), "-1.5");
        assert_eq!("0".parse::<Amount>().unwrap().to_string(), "0");
        assert_eq!("0.5".parse::<Amount>().unwrap().to_string(), "0.5");
        assert_eq!(".5".parse::<Amount>().unwrap().to_string(), "0.5");
        assert_eq!("-.5".parse::<Amount>().unwrap().to_string(), "-0.5");
        assert_eq!("1.".parse::<Amount>().unwrap().to_string(), "1");
        assert_eq!(
            "-9223372036.854775808".parse::<Amount>().unwrap(),
            Amount::MIN
        );
    }

    #[test]
    fn rejects_malformed_or_too_precise_amounts() {
        assert!("".parse::<Amount>().is_err());
        assert!("abc".parse::<Amount>().is_err());
        assert!(".".parse::<Amount>().is_err());
        assert!("-".parse::<Amount>().is_err());
        assert!(".1234567890".parse::<Amount>().is_err());
        assert!("1.2.3".parse::<Amount>().is_err());
        assert!("1.1234567890".parse::<Amount>().is_err());
        assert!(matches!(
            "1.1234567890".parse::<Amount>(),
            Err(ParseAmountError::TooManyDecimals { .. })
        ));
    }

    #[test]
    fn displays_without_trailing_zeros() {
        let a: Amount = "249.06".parse().unwrap();
        assert_eq!(a.to_string(), "249.06");
        assert_eq!(format!("{a:?}"), "249.06");
        assert_eq!(Amount::ZERO.to_string(), "0");
        assert_eq!("2".parse::<Amount>().unwrap().to_string(), "2");
        assert_eq!(
            "0.000001".parse::<Amount>().unwrap().to_string(),
            "0.000001"
        );
        assert_eq!("-1.5".parse::<Amount>().unwrap().to_string(), "-1.5");
    }

    #[test]
    fn round_dp_is_half_away_from_zero() {
        let a: Amount = "249.061234".parse().unwrap();
        assert_eq!(a.round_dp(4).to_string(), "249.0612");
        assert_eq!(a.round_dp(2).to_string(), "249.06");
        assert_eq!(
            "0.0000005"
                .parse::<Amount>()
                .unwrap()
                .round_dp(6)
                .to_string(),
            "0.000001"
        );
        assert_eq!(
            "-0.0000005"
                .parse::<Amount>()
                .unwrap()
                .round_dp(6)
                .to_string(),
            "-0.000001"
        );
        let a: Amount = "249.061234567".parse().unwrap();
        assert_eq!(a.round_dp(4), a.round_dp(4).round_dp(4));
    }

    #[test]
    fn new_from_mantissa_and_scale() {
        assert_eq!(Amount::new(0, 0).to_string(), "0");
        assert_eq!(Amount::new(1, 0).to_string(), "1");
        assert_eq!(Amount::new(249, 0).to_string(), "249");
        assert_eq!(Amount::new(2496, 1).to_string(), "249.6");
        assert_eq!(Amount::new(24906, 2).to_string(), "249.06");
        assert_eq!(Amount::new(6, 2).to_string(), "0.06");
        assert_eq!(Amount::new(1, 3).to_string(), "0.001");
        assert_eq!(Amount::new(-15, 1).to_string(), "-1.5");
    }

    #[test]
    fn units_round_trip_through_the_representation() {
        assert_eq!(Amount::new(2, 6).units(), 2_000);
        assert_eq!(Amount::new(-15, 1).units(), -1_500_000_000);
        assert_eq!(Amount::try_new(i64::MAX, SCALE).unwrap().units(), i64::MAX);
        assert_eq!(Amount::from_units(1_500_000_000).to_string(), "1.5");
        assert_eq!(Amount::from_units(2_000), Amount::new(2, 6));
    }

    #[test]
    fn try_new_and_try_round_dp_report_what_the_plain_ones_panic_on() {
        assert_eq!(Amount::try_new(1, SCALE + 1), None);
        assert_eq!(Amount::try_new(i64::MAX, 0), None);
        assert_eq!(Amount::try_new(24906, 2), Some("249.06".parse().unwrap()));
        assert_eq!(Amount::MAX.try_round_dp(0), None);
        assert_eq!(Amount::new(24906, 2).try_round_dp(SCALE + 1), None);
        assert_eq!(
            Amount::new(24906, 2).try_round_dp(2),
            Some(Amount::new(24906, 2))
        );
    }

    #[test]
    #[should_panic]
    fn new_rejects_scale_over_the_max() {
        let _ = Amount::new(1, SCALE + 1);
    }

    #[test]
    #[should_panic]
    fn round_dp_rejects_scale_over_the_max() {
        let _ = Amount::new(1, 0).round_dp(SCALE + 1);
    }

    #[test]
    #[should_panic]
    fn new_rejects_mantissa_overflow() {
        let _ = Amount::new(i64::MAX, 0);
    }

    #[test]
    fn checked_mul_div_truncates_and_reports_failure() {
        let a: Amount = "2".parse().unwrap();
        assert_eq!(a.checked_mul_div(3, 2), Some("3".parse().unwrap()));
        assert_eq!(
            a.checked_mul_div(1, 3),
            Some("0.666666666".parse().unwrap())
        );
        assert_eq!(a.checked_mul_div(1, 0), None);
        let big: Amount = "9000000000".parse().unwrap();
        assert_eq!(big.checked_mul_div(2, 1), None);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn serde_round_trips_as_string() {
        let a: Amount = "249.06".parse().unwrap();
        let v = serde_json::to_value(a).unwrap();
        assert_eq!(v, serde_json::json!("249.06"));
        let back: Amount = serde_json::from_value(v).unwrap();
        assert_eq!(back, a);
        let from_num: Amount = serde_json::from_value(serde_json::json!(2.5)).unwrap();
        assert_eq!(from_num.to_string(), "2.5");
        let from_int: Amount = serde_json::from_value(serde_json::json!(2)).unwrap();
        assert_eq!(from_int.to_string(), "2");
        let from_neg: Amount = serde_json::from_value(serde_json::json!(-2)).unwrap();
        assert_eq!(from_neg.to_string(), "-2");
    }
}
