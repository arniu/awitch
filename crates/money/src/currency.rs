//! ISO 4217 [`Currency`] codes.

#[derive(Debug, thiserror::Error)]
#[error("unknown currency: {code}")]
pub struct UnknownCurrencyError {
    pub code: String,
}

/// ISO 4217 currency codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "UPPERCASE"))]
pub enum Currency {
    /// `A$` · Australian Dollar
    Aud,
    /// `C$` · Canadian Dollar
    Cad,
    /// `CHF` · Swiss Franc
    Chf,
    /// `¥` · Yuan Renminbi · China
    Cny,
    /// `€` · Euro · Eurozone
    Eur,
    /// `£` · Pound Sterling · United Kingdom
    Gbp,
    /// `HK$` · Hong Kong Dollar
    Hkd,
    /// `₹` · Indian Rupee
    Inr,
    /// `¥` · Yen · Japan
    Jpy,
    /// `₩` · Won · South Korea
    Krw,
    /// `S$` · Singapore Dollar
    Sgd,
    /// `$` · US Dollar
    Usd,
}

impl std::fmt::Display for Currency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Currency::Aud => "AUD",
            Currency::Cad => "CAD",
            Currency::Chf => "CHF",
            Currency::Cny => "CNY",
            Currency::Eur => "EUR",
            Currency::Gbp => "GBP",
            Currency::Hkd => "HKD",
            Currency::Inr => "INR",
            Currency::Jpy => "JPY",
            Currency::Krw => "KRW",
            Currency::Sgd => "SGD",
            Currency::Usd => "USD",
        })
    }
}

impl std::str::FromStr for Currency {
    type Err = UnknownCurrencyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "AUD" => Ok(Currency::Aud),
            "CAD" => Ok(Currency::Cad),
            "CHF" => Ok(Currency::Chf),
            "CNY" => Ok(Currency::Cny),
            "EUR" => Ok(Currency::Eur),
            "GBP" => Ok(Currency::Gbp),
            "HKD" => Ok(Currency::Hkd),
            "INR" => Ok(Currency::Inr),
            "JPY" => Ok(Currency::Jpy),
            "KRW" => Ok(Currency::Krw),
            "SGD" => Ok(Currency::Sgd),
            "USD" => Ok(Currency::Usd),
            other => Err(UnknownCurrencyError {
                code: other.to_string(),
            }),
        }
    }
}

#[cfg(feature = "rusqlite")]
impl rusqlite::types::ToSql for Currency {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.to_string()),
        ))
    }
}

#[cfg(feature = "rusqlite")]
impl rusqlite::types::FromSql for Currency {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        value
            .as_str()?
            .parse()
            .map_err(|e: UnknownCurrencyError| rusqlite::types::FromSqlError::Other(Box::new(e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn currency_round_trips() {
        assert_eq!("CNY".parse::<Currency>().unwrap(), Currency::Cny);
        assert!("GBP!".parse::<Currency>().is_err());
        assert_eq!(Currency::Cny.to_string(), "CNY");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn currency_serializes_as_its_uppercase_code() {
        let v = serde_json::to_value(Currency::Usd).unwrap();
        assert_eq!(v, serde_json::json!("USD"));
    }
}
