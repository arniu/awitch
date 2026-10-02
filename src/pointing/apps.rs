//! The app registry: where each declared agent keeps its config and which
//! keys a `point` writes. See `docs/pointing-spec.md` for the shared
//! point/undo semantics these specs plug into.

use super::schema::{App, dir, file, key_path};

/// Claude Code, see docs/research/app-pointing/claude.md.
///
/// Accepts when `~/.claude/settings.json` exists (`CLAUDE_CONFIG_DIR` override).
/// Writes into its `env` block:
/// - `ANTHROPIC_BASE_URL` = gateway URL
/// - `ANTHROPIC_AUTH_TOKEN` = app key
///
/// Point upserts the keys into `env`, creating the block if absent.
pub static CLAUDE: App = App {
    name: "claude",
    base_dir: dir("~/.claude").env_override("CLAUDE_CONFIG_DIR"),
    files: &[file(
        "settings.json",
        &[
            key_path("env.ANTHROPIC_BASE_URL").set_url(),
            key_path("env.ANTHROPIC_AUTH_TOKEN").set_key(),
        ],
    )
    .trim_empty(&["env"])],
};

/// Codex, see docs/research/app-pointing/codex.md.
///
/// Accepts when `~/.codex/config.toml` exists (`CODEX_HOME` override). Writes:
/// - config.toml:
///   - `[model_providers.awitch]` = { name, base_url, env_key `AWITCH_CODEX_KEY`, wire_api `responses` }
///   - `model_provider` = `awitch`
/// - auth.json:
///   - `AWITCH_CODEX_KEY` = app key
///
/// Point writes the provider entry + pointer into config.toml and the key
/// into auth.json.
pub static CODEX: App = App {
    name: "codex",
    base_dir: dir("~/.codex").env_override("CODEX_HOME"),
    files: &[
        // Credential file first (I3).
        file("auth.json", &[
            key_path("AWITCH_CODEX_KEY").set_key(),
        ]),
        file("config.toml", &[
            key_path("model_provider").set_pointer("awitch"),
            key_path("model_providers.awitch").set_entry(
                r#"{"name":"awitch gateway","base_url":"{url}","env_key":"AWITCH_CODEX_KEY","wire_api":"responses"}"#,
            ),
        ]),
    ],
};

/// Gemini CLI, see docs/research/app-pointing/gemini.md.
///
/// Accepts when `~/.gemini/.env` exists (`GEMINI_CLI_HOME` override). Writes:
/// - `GOOGLE_GEMINI_BASE_URL` = gateway URL
/// - `GEMINI_API_KEY` = app key
///
/// Point upserts the lines into `.env`.
pub static GEMINI: App = App {
    name: "gemini",
    base_dir: dir("~/.gemini").env_override("GEMINI_CLI_HOME"),
    files: &[file(
        ".env",
        &[
            key_path("GOOGLE_GEMINI_BASE_URL").set_url(),
            key_path("GEMINI_API_KEY").set_key(),
        ],
    )],
};

/// Hermes, see docs/research/app-pointing/hermes.md.
///
/// Accepts when `~/.hermes/config.yaml` exists (`HERMES_HOME` override). Writes:
/// - config.yaml:
///   - `providers.awitch` = { base_url, key_env `AWITCH_HERMES_KEY` }
///   - `model.provider` = `awitch`
/// - `.env`:
///   - `AWITCH_HERMES_KEY` = app key
///
/// Point writes the provider entry + pointer into config.yaml and the key
/// into `.env`.
pub static HERMES: App = App {
    name: "hermes",
    base_dir: dir("~/.hermes").env_override("HERMES_HOME"),
    files: &[
        // Credential file first (I3).
        file(".env", &[key_path("AWITCH_HERMES_KEY").set_key()]),
        file(
            "config.yaml",
            &[
                key_path("model.provider").set_pointer("awitch"),
                key_path("providers.awitch")
                    .set_entry(r#"{"base_url":"{url}","key_env":"AWITCH_HERMES_KEY"}"#),
            ],
        ),
    ],
};

/// OpenClaw, see docs/research/app-pointing/openclaw.md.
///
/// Accepts when `~/.openclaw/openclaw.json` exists (`OPENCLAW_HOME` override).
/// Writes the `models.providers.awitch` entry:
/// - `baseUrl` = gateway URL
/// - `apiKey` = app key
/// - `api` = `openai-completions`
///
/// The file is JSON5 — parsed leniently (comments, trailing commas), written
/// back as plain JSON.
///
/// Point writes the entry into `models.providers`.
pub static OPENCLAW: App = App {
    name: "openclaw",
    base_dir: dir("~/.openclaw").env_override("OPENCLAW_HOME"),
    files: &[file(
        "openclaw.json",
        &[key_path("models.providers.awitch")
            .set_entry(r#"{"baseUrl":"{url}","apiKey":"{key}","api":"openai-completions"}"#)],
    )],
};

/// OpenCode, see docs/research/app-pointing/opencode.md.
///
/// Accepts when `~/.config/opencode/opencode.json` exists. Writes:
/// - opencode.json:
///   - `provider.awitch` = { npm `@ai-sdk/openai-compatible`, name,
///     `options.baseURL` = gateway URL, models }
/// - auth.json (`~/.local/share/opencode/auth.json`):
///   - `awitch` = app key
///
/// Point writes the provider entry into opencode.json and the key into
/// auth.json.
pub static OPENCODE: App = App {
    name: "opencode",
    base_dir: dir("~/.config/opencode"),
    files: &[
        // Credential file first (I3) — it lives under the home dir.
        file("~/.local/share/opencode/auth.json", &[
            key_path("awitch").set_key(),
        ]),
        file("opencode.json", &[
            key_path("provider.awitch").set_entry(
                r#"{"npm":"@ai-sdk/openai-compatible","name":"awitch gateway","options":{"baseURL":"{url}"},"models":{}}"#,
            ),
        ]),
    ],
};

/// pi, see docs/research/app-pointing/pi.md.
///
/// Accepts when `~/.pi/agent/models.json` exists (`PI_HOME` override). Writes
/// the `providers.awitch` entry:
/// - `baseUrl` = gateway URL
/// - `apiKey` = app key
/// - `api` = `openai-completions`
///
/// Point writes the entry into `providers`.
pub static PI: App = App {
    name: "pi",
    base_dir: dir("~/.pi").env_override("PI_HOME"),
    files: &[file(
        "agent/models.json",
        &[key_path("providers.awitch")
            .set_entry(r#"{"baseUrl":"{url}","api":"openai-completions","apiKey":"{key}"}"#)],
    )],
};

/// Every declared app spec, with whether awitch offers it.
static SPECS: [(App, bool); 7] = [
    (CLAUDE, true),
    (CODEX, true),
    (HERMES, true),
    (OPENCLAW, true),
    (OPENCODE, true),
    (PI, true),
    (GEMINI, false),
];

/// The specs awitch offers.
pub fn supported() -> impl Iterator<Item = &'static App> + Clone {
    SPECS
        .iter()
        .filter(|(_, offered)| *offered)
        .map(|(app, _)| app)
}

/// Every declared spec, offered or not: what the specs' own invariants run
/// over, and what the operation tests point.
#[cfg(test)]
pub fn declared() -> impl Iterator<Item = &'static App> + Clone {
    SPECS.iter().map(|(app, _)| app)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashSet;

    #[test]
    fn app_names_are_unique() {
        let mut seen = HashSet::new();
        for app in declared() {
            assert!(seen.insert(app.name), "duplicate app name '{}'", app.name);
        }
    }

    #[test]
    fn app_names_are_nonempty() {
        for app in declared() {
            assert!(
                !app.name.is_empty(),
                "an app with an empty name cannot be addressed"
            );
        }
    }

    #[test]
    fn file_paths_are_unique_within_an_app() {
        for app in declared() {
            let mut seen = HashSet::new();
            for file in app.files {
                assert!(
                    seen.insert(file.path),
                    "app '{}': duplicate file path '{}'",
                    app.name,
                    file.path
                );
            }
        }
    }

    #[test]
    fn key_paths_are_unique_within_a_file() {
        for app in declared() {
            for file in app.files {
                let mut seen = HashSet::new();
                for patch in file.patches {
                    assert!(
                        seen.insert(patch.key_path),
                        "app '{}', file '{}': duplicate key path '{}'",
                        app.name,
                        file.path,
                        patch.key_path
                    );
                }
            }
        }
    }

    #[test]
    fn every_app_points_at_least_one_patch() {
        for app in declared() {
            let total: usize = app.files.iter().map(|f| f.patches.len()).sum();
            assert!(total > 0, "app '{}' points nothing", app.name);
        }
    }
}
