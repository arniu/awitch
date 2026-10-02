pub(crate) mod cc_switch;
pub(crate) mod cheapskate;

use crate::protocol::Protocol;

/// An external data source's provider pool — one per source read.
#[derive(Debug)]
pub(crate) struct Source {
    pub(crate) vendor: String,
    pub(crate) providers: Vec<Provider>,
}

/// One provider configuration from an external source.
#[derive(Debug)]
pub(crate) struct Provider {
    pub(crate) base_url: String,
    pub(crate) key: String,
    pub(crate) protocols: Vec<Protocol>,
}
