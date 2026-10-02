//! The settings subject: read the stored document, write named fields.

use rusqlite::Connection;

use crate::settings::Settings;

use super::{Command, Ctx};
use crate::db::Result;
use crate::db::sql::{set_settings, stored_settings};

/// The settings as the gateway reads them: the stored rows, each field parsed
/// with its own default. The storage layer deals in rows only.
pub(in crate::db) fn read_settings(conn: &Connection) -> Result<Settings> {
    Ok(Settings::parse_from(&stored_settings(conn)?))
}

pub(in crate::db) struct GetSettings;

impl Command for GetSettings {
    type Reply = Settings;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        read_settings(cx.conn)
    }
}

pub(in crate::db) struct SetSettings {
    pub entries: Vec<(String, String)>,
}

impl Command for SetSettings {
    type Reply = ();

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        set_settings(cx.conn, &self.entries)?;

        // The write landed; a failed reload would leave the health parameters
        // stale, so it is not swallowed.
        match read_settings(cx.conn) {
            Ok(fresh) => *cx.settings = fresh,
            Err(e) => tracing::error!("settings reload failed: {e}"),
        }

        Ok(())
    }
}
