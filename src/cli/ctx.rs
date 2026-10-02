use std::path::PathBuf;

use anyhow::anyhow;

use super::client::Client;
use crate::config::Config;

/// Shared state for one command invocation — the loaded config, plus a lazy
/// control client modules ask for when they need the gateway.
pub struct Ctx {
    pub config: Config,
    pub config_dir: PathBuf,
    client: std::cell::RefCell<Option<std::rc::Rc<Client>>>,
}

impl Ctx {
    /// Config only — the client is built lazily from its module.
    pub fn load() -> anyhow::Result<Ctx> {
        let dir = crate::config::config_dir();
        let config = Config::load(&dir)?;
        Ok(Ctx {
            config,
            config_dir: dir,
            client: std::cell::RefCell::new(None),
        })
    }

    /// A control client for this invocation.
    pub fn client(&self) -> anyhow::Result<std::rc::Rc<Client>> {
        if let Some(c) = self.client.borrow().as_ref() {
            return Ok(std::rc::Rc::clone(c));
        }

        let url = self.control_url();
        let token = self.control_token()?;
        let c = std::rc::Rc::new(Client::new(&url, &token)?);
        *self.client.borrow_mut() = Some(std::rc::Rc::clone(&c));

        Ok(c)
    }

    /// The control token.
    fn control_token(&self) -> anyhow::Result<String> {
        let path = self.config_dir.join("control.token");
        let token = std::fs::read_to_string(&path)
            .map_err(|_| anyhow!("gateway not running — run 'awitch service start'"))?;
        let token = token.trim().to_string();
        if token.is_empty() {
            return Err(anyhow!("gateway not running — run 'awitch service start'"));
        }

        Ok(token)
    }

    /// The control URL — where the CLI reaches the control plane.
    pub fn control_url(&self) -> String {
        format!(
            "http://{}:{}",
            self.config.control_host(),
            self.config.control_port()
        )
    }

    /// The data URL — the gateway address pointing writes.
    pub fn url(&self) -> String {
        format!("http://{}:{}", self.config.host(), self.config.port())
    }
}
