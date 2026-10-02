//! Store commands, one module per subject.
//!
//! A command touches only what [`Ctx`] exposes — never the actor itself, so no
//! command can ask the actor from inside it. Its reply address lives in the
//! mailbox item, not in the command.

pub(in crate::db) mod app_keys;
pub(in crate::db) mod attempts;
pub(in crate::db) mod balance;
pub(in crate::db) mod ledger;
pub(in crate::db) mod pins;
pub(in crate::db) mod providers;
pub(in crate::db) mod routing;
pub(in crate::db) mod settings;

use std::collections::HashMap;

use rusqlite::Connection;

use crate::settings::Settings;

use super::cache::RoutingCache;

pub(in crate::db) use super::actor::Command;

/// A command may touch nothing beyond this.
pub(super) struct Ctx<'a> {
    pub(super) conn: &'a Connection,
    pub(super) settings: &'a mut Settings,
    pub(super) routing: &'a mut RoutingCache,
    pub(super) keys: &'a mut HashMap<String, String>,
}
