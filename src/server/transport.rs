use serde_json::Value;

use crate::server::error::ServerError;

#[derive(Clone)]
pub(crate) struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub(crate) fn new() -> ReqwestTransport {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("reqwest client");
        ReqwestTransport { client }
    }

    pub(crate) fn http_client(&self) -> &reqwest::Client {
        &self.client
    }

    pub(crate) async fn send(
        &self,
        url: &str,
        body: &Value,
        headers: Vec<(String, String)>,
    ) -> Result<reqwest::Response, ServerError> {
        let mut req = self.client.post(url).json(body);
        for (k, v) in headers {
            req = req.header(k, v);
        }

        req.send().await.map_err(|e| ServerError::Transport {
            body: e.to_string(),
            status: 0,
        })
    }
}
