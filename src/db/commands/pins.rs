//! The pins subject: which providers an app may route within.

use crate::provider::Pin;

use super::{Command, Ctx};
use crate::db::Result;
use crate::db::cache::Stale;
use crate::db::sql::{
    clear_all_pins, clear_pins, delete_pin, has_pin, insert_pin, list_pins, pins_by_app,
    pins_by_provider,
};

pub(in crate::db) struct InsertPin {
    pub app: String,
    pub provider_id: String,
}

impl Command for InsertPin {
    type Reply = ();

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        insert_pin(cx.conn, &self.app, &self.provider_id)?;
        cx.routing.mark(Stale::Pins);
        Ok(())
    }
}

pub(in crate::db) struct DeletePin {
    pub app: String,
    pub provider_id: String,
}

impl Command for DeletePin {
    type Reply = bool;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        let removed = delete_pin(cx.conn, &self.app, &self.provider_id)?;
        cx.routing.mark(Stale::Pins);
        Ok(removed)
    }
}

pub(in crate::db) struct ClearPins {
    pub app: String,
}

impl Command for ClearPins {
    type Reply = ();

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        clear_pins(cx.conn, &self.app)?;
        cx.routing.mark(Stale::Pins);
        Ok(())
    }
}

pub(in crate::db) struct ClearAllPins;

impl Command for ClearAllPins {
    type Reply = ();

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        clear_all_pins(cx.conn)?;
        cx.routing.mark(Stale::Pins);
        Ok(())
    }
}

pub(in crate::db) struct ListPins;

impl Command for ListPins {
    type Reply = Vec<Pin>;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        list_pins(cx.conn)
    }
}

pub(in crate::db) struct HasPin {
    pub app: String,
    pub provider_id: String,
}

impl Command for HasPin {
    type Reply = bool;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        has_pin(cx.conn, &self.app, &self.provider_id)
    }
}

pub(in crate::db) struct PinsByApp {
    pub app: String,
}

impl Command for PinsByApp {
    type Reply = Vec<Pin>;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        pins_by_app(cx.conn, &self.app)
    }
}

pub(in crate::db) struct PinsByProvider {
    pub provider_id: String,
}

impl Command for PinsByProvider {
    type Reply = Vec<Pin>;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        pins_by_provider(cx.conn, &self.provider_id)
    }
}
