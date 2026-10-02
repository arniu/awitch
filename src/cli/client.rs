use serde_json::Value;

use crate::api_types;
use crate::ledger::BucketWidth;
use crate::provider::{Pin, ProviderPatch};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("binary is v{cli}, server is v{server} — run 'awitch service restart' first")]
    VersionMismatch { cli: String, server: String },
    #[error("HTTP {status}: {message}")]
    ApiError { status: u16, message: String },
    /// The gateway responded, but with an unexpected status (e.g. 401 token).
    #[error("health returned HTTP {0}")]
    HealthStatus(u16),
    #[error("{0}")]
    Http(String),
}

pub type Result<T> = std::result::Result<T, ClientError>;

/// Control-plane client.
pub struct Client {
    http: reqwest::blocking::Client,
    base: String,
    /// True once the handshake passed.
    ///
    /// Success is cached; a failure is not, so an attempt is re-dialed.
    verified: std::cell::RefCell<bool>,
}

impl Client {
    /// A control client at `url`, authenticated by `token`.
    pub fn new(url: &str, token: &str) -> Result<Client> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| ClientError::Http(format!("bad control token: {e}")))?,
        );

        let http = reqwest::blocking::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| ClientError::Http(e.to_string()))?;
        Ok(Client {
            http,
            base: url.to_string(),
            verified: std::cell::RefCell::new(false),
        })
    }

    /// Probe the gateway now — reachability + token + version.
    pub fn health(&self) -> Result<()> {
        self.ensure()
    }

    /// The handshake, done once on the first request that reaches the gateway.
    fn ensure(&self) -> Result<()> {
        if *self.verified.borrow() {
            return Ok(());
        }

        match self.health_raw() {
            Ok(_) => {
                *self.verified.borrow_mut() = true;
                Ok(())
            }
            Err(ClientError::HealthStatus(status)) => Err(ClientError::Http(format!(
                "gateway is running but rejected the control token (HTTP {status})"
            ))),
            Err(ClientError::Http(_)) => Err(ClientError::Http(
                "gateway not running — run 'awitch service start'".into(),
            )),
            Err(e) => Err(e),
        }
    }

    /// GET /api/health — the handshake's error mapping left to the caller.
    fn health_raw(&self) -> Result<()> {
        let (status, body) = self.request_raw("GET", "/api/health", None)?;
        if status != 200 {
            return Err(ClientError::HealthStatus(status));
        }
        let health: api_types::Health<'_> = serde_json::from_slice(&body)
            .map_err(|e| ClientError::Http(format!("bad health json: {e}")))?;
        let cli_version = env!("CARGO_PKG_VERSION");
        if health.version != cli_version {
            return Err(ClientError::VersionMismatch {
                cli: cli_version.to_string(),
                server: health.version.to_string(),
            });
        }
        Ok(())
    }

    // ---- HTTP ---------------------------------------------------------------

    /// A control request: the handshake rides the first call, then the request
    /// goes out. Every request funnels here, so clients connect on demand.
    fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, Vec<u8>)> {
        self.ensure()?;
        self.request_raw(method, path, body)
    }

    /// Send one HTTP request — no handshake (health_raw's path).
    fn request_raw(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Vec<u8>)> {
        let url = format!("{}{path}", self.base);
        let mut req = match method {
            "GET" => self.http.get(&url),
            "POST" => self.http.post(&url),
            "PUT" => self.http.put(&url),
            "PATCH" => self.http.patch(&url),
            "DELETE" => self.http.delete(&url),
            _ => return Err(ClientError::Http(format!("unsupported method: {method}"))),
        };
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().map_err(|e| ClientError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().map_err(|e| ClientError::Http(e.to_string()))?;
        Ok((status, bytes.to_vec()))
    }

    /// A 2xx JSON response, parsed into `T`. Any other status raises the
    /// server's error envelope as an `ApiError` — one error exit for all
    /// non-health calls.
    fn request_json<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<T> {
        let (status, bytes) = self.request(method, path, body)?;
        expect_ok(status, &bytes)?;
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Http(format!("bad json: {e}")))
    }

    fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.request_json("GET", path, None)
    }

    /// A 2xx action (the body, if any, is ignored).
    fn send(&self, method: &str, path: &str, body: Option<&Value>) -> Result<()> {
        let (status, bytes) = self.request(method, path, body)?;
        expect_ok(status, &bytes)
    }

    // ---- high-level methods (one per CLI command) ---------------------------

    pub fn list_providers(&self) -> Result<Vec<api_types::ProviderView>> {
        let list: api_types::List<api_types::ProviderView> = self.get_json("/api/providers")?;
        Ok(list.data)
    }

    pub fn get_provider(&self, id: &str) -> Result<api_types::ProviderView> {
        self.get_json(&format!("/api/providers/{id}"))
    }

    pub fn add_provider(&self, create: &api_types::ProviderAdd) -> Result<api_types::ProviderView> {
        self.request_json("POST", "/api/providers", Some(&to_value(create)?))
    }

    pub fn instantiate_template(
        &self,
        template_id: &str,
        id: Option<&str>,
        key: &str,
    ) -> Result<api_types::ProviderView> {
        let body = to_value(&api_types::TemplateInstantiate {
            id: id.map(str::to_string),
            key: key.to_string(),
        })?;
        self.request_json(
            "POST",
            &format!("/api/provider-templates/{template_id}/providers"),
            Some(&body),
        )
    }

    pub fn delete_provider(&self, id: &str) -> Result<()> {
        self.send("DELETE", &format!("/api/providers/{id}"), None)
    }

    pub fn update_provider(
        &self,
        id: &str,
        patch: &ProviderPatch,
    ) -> Result<api_types::ProviderView> {
        self.request_json(
            "PATCH",
            &format!("/api/providers/{id}"),
            Some(&to_value(patch)?),
        )
    }

    pub fn poll_balance(&self, id: &str) -> Result<money::Money> {
        self.request_json("POST", &format!("/api/providers/{id}/balance"), None)
    }

    pub fn patch_settings(&self, patch: &Value) -> Result<()> {
        self.send("PATCH", "/api/settings", Some(patch))
    }

    pub fn add_pin(&self, app: &str, provider: &str) -> Result<()> {
        self.send("PUT", &format!("/api/pins/{app}/{provider}"), None)
    }

    pub fn remove_pin(&self, app: &str, provider: &str) -> Result<()> {
        self.send("DELETE", &format!("/api/pins/{app}/{provider}"), None)
    }

    /// Remove a provider from every app that pins it.
    ///
    /// The provider-wide unpin composes member deletes: the apps come from the
    /// provider's own pins, and each unpin is one member delete.
    pub fn remove_pins_for_provider(&self, provider: &str) -> Result<()> {
        for app in self.list_pinned_apps(provider)? {
            self.remove_pin(&app, provider)?;
        }
        Ok(())
    }

    /// The providers an app pins, in the order it pinned them.
    pub fn list_pinned_providers(&self, app: &str) -> Result<Vec<String>> {
        let list: api_types::List<api_types::ProviderView> =
            self.get_json(&format!("/api/pins/{app}"))?;
        Ok(list.data.into_iter().map(|view| view.provider.id).collect())
    }

    /// The apps pinning a provider, in the order they pinned it.
    pub fn list_pinned_apps(&self, provider: &str) -> Result<Vec<String>> {
        let list: api_types::List<String> =
            self.get_json(&format!("/api/providers/{provider}/pins"))?;
        Ok(list.data)
    }

    /// Clear the pins of one app — or of every app when `app` is None
    /// (→ auto routing): both are one clear on the app resource or the
    /// collection.
    pub fn clear_pins(&self, app: Option<&str>) -> Result<()> {
        match app {
            Some(app) => self.send("DELETE", &format!("/api/pins/{app}"), None),
            None => self.send("DELETE", "/api/pins", None),
        }
    }

    /// Every pinned (app, provider) row, ordered by app.
    pub fn list_pins(&self) -> Result<Vec<Pin>> {
        let list: api_types::List<Pin> = self.get_json("/api/pins")?;
        Ok(list.data)
    }

    /// One page of usage starting at `from`, narrowed to `app` or
    /// `provider_id` when given.
    pub fn usage(
        &self,
        from: i64,
        bucket_width: BucketWidth,
        app: Option<&str>,
        provider_id: Option<&str>,
    ) -> Result<api_types::UsageReport> {
        let scope = match (app, provider_id) {
            (Some(app), _) => format!("/api/usage/app/{app}"),
            (None, Some(id)) => format!("/api/usage/provider/{id}"),
            (None, None) => "/api/usage".to_string(),
        };

        self.get_json(&format!(
            "{scope}?from={from}&bucket_width={}",
            bucket_width.as_str()
        ))
    }

    pub fn get_settings(&self) -> Result<crate::settings::Settings> {
        self.get_json("/api/settings")
    }

    pub fn issue_app_key(&self, app: &str) -> Result<api_types::AppKeyIssued> {
        self.request_json(
            "POST",
            "/api/app-keys",
            Some(&to_value(&api_types::AppKeyIssue {
                app: app.to_string(),
            })?),
        )
    }

    // The management command that lists/revokes app keys is not wired yet.
    #[expect(dead_code)]
    pub fn list_app_keys(&self, app: Option<&str>) -> Result<Vec<api_types::AppKeyView>> {
        let path = match app {
            Some(app) => format!("/api/app-keys?app={app}"),
            None => "/api/app-keys".to_string(),
        };
        let list: api_types::List<api_types::AppKeyView> = self.get_json(&path)?;
        Ok(list.data)
    }

    // The management command that lists/revokes app keys is not wired yet.
    #[expect(dead_code)]
    pub fn revoke_app_key(&self, id: &str) -> Result<()> {
        self.send("DELETE", &format!("/api/app-keys/{id}"), None)
    }
}

// ---- helpers ---------------------------------------------------------------

fn to_value<T: serde::Serialize>(v: &T) -> Result<Value> {
    serde_json::to_value(v).map_err(|e| ClientError::Http(e.to_string()))
}

/// 2xx passes; anything else maps the server's error envelope into an
/// `ApiError` (a body with no message reads as "unknown error").
fn expect_ok(status: u16, body: &[u8]) -> Result<()> {
    if (200..300).contains(&status) {
        return Ok(());
    }

    let v: Value = serde_json::from_slice(body).unwrap_or_default();
    let message = v["error"]["message"].as_str().unwrap_or("unknown error");
    Err(ClientError::ApiError {
        status,
        message: message.to_string(),
    })
}
