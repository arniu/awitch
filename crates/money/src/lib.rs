mod amount;
mod currency;

pub use amount::{Amount, ParseAmountError};
pub use currency::{Currency, UnknownCurrencyError};

/// An amount of money in one currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Money {
    pub amount: Amount,
    pub currency: Currency,
}
