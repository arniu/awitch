# Codex

> Source: `github.com/openai/codex` @ `rust-v0.147.0`

## Config-file hierarchy

Primary source: `openai/codex` repo `codex-rs/config/src/loader/README.md`.

Priority high → low:

1. MDM `managed_config.toml` (being deprecated)
2. Session flags (`codex login -c key=value`, i.e. CLI overrides)
3. Project config `.codex/config.toml`
4. User profile config
5. User config `~/.codex/config.toml`
6. Enterprise cloud-hosted layer
7. System config `/etc/codex/config.toml`

## Provider definition

Primary source: `codex-rs/config/src/config_toml.rs` +
`codex-rs/model-provider-info/src/lib.rs`.

`config.toml` top level:

```toml
model_provider = "custom"        # id of the effective provider (pointer)
model = "deepseek-v4-flash"      # current model
model_reasoning_effort = "high"
model_catalog_json = "..."       # optional: model catalog override
```

`[model_providers.<id>]` table fields (`ModelProviderInfo` struct, source:
`codex-rs/model-provider-info/src/lib.rs`):

```rust
pub struct ModelProviderInfo {
    pub name: String,                                      // display name
    pub base_url: Option<String>,                          // OpenAI-compatible endpoint
    pub env_key: Option<String>,                           // env var for the API key (recommended)
    pub env_key_instructions: Option<String>,              // hint text for obtaining the key
    pub experimental_bearer_token: Option<String>,         // inline Bearer token (not recommended)
    pub auth: Option<ModelProviderAuthInfo>,               // command-based token ({command, ...})
    pub aws: Option<ModelProviderAwsAuthInfo>,             // AWS SigV4 auth ({profile, region})
    pub wire_api: WireApi,                                 // wire protocol; currently only "responses" ("chat" removed)
    pub query_params: Option<HashMap<String, String>>,     // extra request params
    pub http_headers: Option<HashMap<String, String>>,     // extra request headers
    pub env_http_headers: Option<HashMap<String, String>>, // env-based extra headers
    pub request_max_retries: Option<u64>,                  // request retry count
    pub stream_max_retries: Option<u64>,                   // stream retry count
    pub stream_idle_timeout_ms: Option<u64>,               // stream idle timeout
    pub websocket_connect_timeout_ms: Option<u64>,         // WebSocket connect timeout
    pub requires_openai_auth: bool,                        // require official OpenAI auth (proxy-forwarding scenario)
    pub supports_websockets: bool,                         // WebSocket transport support
    pub supports_standalone_web_search: bool,              // standalone web search support
}
```

- `aws` is mutually exclusive with `env_key` / `experimental_bearer_token` /
  `auth` / `requires_openai_auth`; `auth.command` must not be empty.
- `wire_api` enum currently only `responses` (`"chat"` was removed and errors,
  see `CHAT_WIRE_API_REMOVED_ERROR`).

Top-level legacy `base_url` is honored when no `model_provider` is set.

## Credentials

- `~/.codex/auth.json`: `{"OPENAI_API_KEY": "sk-..."}`; ChatGPT OAuth login also
  stores tokens (`codex login`).
- `codex login --with-api-key` (reads key from stdin), `codex logout`,
  `codex login status`.
- Providers can use `env_key` pointing at an env var (e.g. `DEEPSEEK_API_KEY`);
  the OpenAI key lives in `~/.codex/auth.json` (`OPENAI_API_KEY` or ChatGPT OAuth
  `tokens`).

## Real example (local `~/.codex/config.toml`, redacted)

```toml
model_provider = "custom"
model = "deepseek-v4-flash"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "deepseek"
base_url = "https://api.deepseek.com"
wire_api = "responses"
requires_openai_auth = true
```

combined with `OPENAI_API_KEY` (a DeepSeek key) in `~/.codex/auth.json` completes
the switch.

## Switch notes

- Switch = rewrite the top-level `model_provider` pointer + `model`, and maintain
  the target provider definition in `[model_providers.<id>]` (base_url / wire_api
  / auth); keys live in auth.json.
- Multiple provider definitions can coexist under `[model_providers]`
  (additive), but only one is effective (switch semantics).

## Sources

- `github.com/openai/codex`:
  - `codex-rs/config/src/config_toml.rs`
  - `codex-rs/model-provider-info/src/lib.rs`
  - `codex-rs/config/src/loader/README.md`
- Local `~/.codex/config.toml` + `~/.codex/auth.json` (real config examples,
  redacted, structure confirmation only, not sources)
- `codex --help` / `codex login --help` (CLI usage text)
- Note: `developers.openai.com` (incl. the official manual `codex-manual.md`) is
  403 for this network as a whole (Cloudflare regional block, not transient), so
  the repo source is authoritative; the manual's canonical address is
  `https://developers.openai.com/codex/codex-manual.md` (the source the built-in
  `fetch-codex-manual.mjs` skill script points at).
