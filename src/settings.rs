use std::collections::HashMap;
use std::str::FromStr;

use money::Amount;
use serde::{Deserialize, Serialize};

use crate::routing::RoutingMode;

pub(crate) struct Field<T> {
    pub(crate) key: &'static str,
    pub(crate) default: T,
}

pub(crate) trait FieldExt: Sync {
    fn key(&self) -> &'static str;
    fn validate(&self, raw: &str) -> Result<(), String>;
}

impl<T: FromStr + Copy> Field<T> {
    pub(crate) fn parse_or_default(&self, value: Option<&str>) -> T {
        match value {
            Some(raw) => raw.parse().unwrap_or_else(|_| {
                tracing::warn!("invalid setting {} = {raw:?}", self.key);
                self.default
            }),
            None => self.default,
        }
    }
}

impl<T: FromStr + Copy + Sync + std::fmt::Display> FieldExt for Field<T>
where
    T::Err: std::fmt::Display,
{
    fn key(&self) -> &'static str {
        self.key
    }

    fn validate(&self, raw: &str) -> Result<(), String> {
        raw.parse::<T>()
            .map(|_| ())
            .map_err(|e| format!("invalid {} '{raw}': {e}", self.key))
    }
}

macro_rules! define_settings {
    ($( $field:ident : $ty:ty as $key:literal ($default:expr) ),+ $(,)?) => {
        $(#[expect(non_upper_case_globals)]
        const $field: Field<$ty> = Field { key: $key, default: $default };)+

        pub(crate) static SETTINGS: &[&dyn FieldExt] = &[ $( &$field, )+ ];

        #[derive(Serialize, Deserialize)]
        pub(crate) struct Settings {
            $(#[serde(rename = $key)]
            pub(crate) $field: $ty,)+
        }

        impl Settings {
            pub(crate) fn parse_from(stored: &HashMap<String, String>) -> Settings {
                for key in stored.keys() {
                    if !SETTINGS.iter().any(|s| s.key() == key) {
                        tracing::warn!("stored setting {key:?} is not a setting; ignored");
                    }
                }

                Settings {
                    $( $field: $field.parse_or_default(stored.get($field.key).map(String::as_str)), )+
                }
            }
        }
    };
}

define_settings!(
    routing_mode:              RoutingMode as "routing.mode"              (RoutingMode::Eco),
    routing_balance_threshold: Amount      as "routing.balance_threshold" (Amount::new(1, 0)),
    routing_failure_threshold: u32         as "routing.failure_threshold" (5),
    routing_cooldown_secs:     i64         as "routing.cooldown_secs"     (10 * 60),
    ledger_retention_secs:     i64         as "ledger.retention_secs"     (90 * 24 * 60 * 60),
    attempts_max_per_provider: u32         as "attempts.max_per_provider" (1000),
);
