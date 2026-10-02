use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};

use super::{failure_rule, failures_str, money_str, print_probe};
use crate::api_types;
use crate::cli::args::{ProviderCmd, TemplateArgs};
use crate::cli::ctx::Ctx;
use crate::protocol::Protocol;
use crate::provider::{Endpoint, Provider, ProviderPatch};

/// Parse `--protocol` value:
/// - `family` claims the canonical endpoint;
/// - `family=path` spells a rooted endpoint.
fn parse_protocol(value: &str) -> anyhow::Result<(Protocol, Endpoint)> {
    let value = value.trim();
    let (family, path) = match value.split_once('=') {
        Some((f, p)) => (f.trim(), Some(p.trim())),
        None => (value, None),
    };

    let protocol = family
        .parse()
        .map_err(|_| anyhow!("unknown protocol '{family}'"))?;
    let endpoint = path.map_or(Endpoint::Canonical, |path| Endpoint::Path(path.into()));

    Ok((protocol, endpoint))
}

/// Parse the repeated `--protocol` flags.
fn parse_protocol_flags(flags: &[String]) -> anyhow::Result<BTreeMap<Protocol, Endpoint>> {
    flags.iter().map(|flag| parse_protocol(flag)).collect()
}

pub fn run(ctx: &Ctx, cmd: ProviderCmd) -> anyhow::Result<()> {
    match cmd {
        ProviderCmd::Template(args) => template_cmd(args),
        ProviderCmd::List => list(ctx),
        ProviderCmd::Show { id } => show(ctx, &id),
        ProviderCmd::Add {
            template,
            file,
            base_url,
            key,
            key_file,
            name,
            protocol,
            models_url,
            balance_url,
            no_verify,
        } => {
            let key = resolve_key(key, key_file)?.ok_or_else(|| {
                anyhow!(
                    "a new provider needs an API key: pass --key '…', --key - (stdin), or --key-file <path>"
                )
            })?;

            // A template-backed add instantiates under the template's own
            // resource; a template-free add carries its fields to the pool
            // directly.
            let provider = if let Some(template_id) = &template {
                ctx.client()?
                    .instantiate_template(template_id, None, &key)?
            } else if let Some(path) = &file {
                let template = local_template(path)?;
                let create = api_types::ProviderAdd {
                    id: None,
                    key,
                    base_url: template.base_url,
                    name: template.name,
                    protocols: template.protocols,
                    models_url: template.models_url,
                    balance_url: template.balance_url,
                };
                ctx.client()?.add_provider(&create)?
            } else {
                // clap requires base_url unless -t / --file is given
                let Some(base_url) = base_url else {
                    bail!("--base-url is required without -t or --file");
                };

                let create = api_types::ProviderAdd {
                    id: None,
                    key,
                    base_url,
                    name,
                    protocols: parse_protocol_flags(&protocol)?,
                    models_url: models_url
                        .map(|u| url_override("--models-url", u))
                        .transpose()?,
                    balance_url: balance_url
                        .map(|u| url_override("--balance-url", u))
                        .transpose()?,
                };
                ctx.client()?.add_provider(&create)?
            };
            println!("added provider '{}'", provider.provider.id);
            if !no_verify && !provider.provider.protocols.is_empty() {
                print_probe(&provider.provider);
            }
            Ok(())
        }
        ProviderCmd::Delete { id } => delete(ctx, &id),
        ProviderCmd::Edit {
            id,
            base_url,
            key,
            key_file,
            name,
            protocol,
            models_url,
            balance_url,
        } => {
            let key = resolve_key(key, key_file)?;
            let protocols = if protocol.is_empty() {
                None
            } else {
                Some(parse_protocol_flags(&protocol)?)
            };

            let patch = ProviderPatch {
                base_url,
                key,
                name,
                protocols,
                models_url,
                balance_url,
            };

            ctx.client()?.update_provider(&id, &patch)?;
            println!("updated provider '{id}'");
            Ok(())
        }
        ProviderCmd::Balance { id } => balance(ctx, &id),
        ProviderCmd::Probe { id } => probe(ctx, &id),
    }
}

fn template_cmd(args: TemplateArgs) -> anyhow::Result<()> {
    let TemplateArgs { id, output } = args;
    match id {
        None => {
            for t in crate::provider::builtin_templates() {
                let protocols = t
                    .protocols
                    .keys()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                println!(
                    "{:<20} {:<24} protocols=[{protocols}]",
                    t.id,
                    t.name.as_deref().unwrap_or(&t.id)
                );
            }
            Ok(())
        }
        Some(id) => {
            let raw = crate::provider::builtin_template_file(&id).ok_or_else(|| {
                anyhow!(
                    "unknown provider template '{id}' — known: {}",
                    crate::provider::builtin_template_ids()
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            let path = match output {
                Some(path) => path,
                None => std::env::current_dir()?.join(format!("{id}.toml")),
            };
            if path.exists() {
                bail!(
                    "{} already exists — delete it or write elsewhere with --output",
                    path.display()
                );
            }
            std::fs::write(&path, raw).with_context(|| format!("write {}", path.display()))?;
            println!("wrote {}", path.display());
            Ok(())
        }
    }
}

fn list(ctx: &Ctx) -> anyhow::Result<()> {
    let providers = ctx.client()?.list_providers()?;
    if providers.is_empty() {
        println!("no providers in the pool");
        return Ok(());
    }
    let settings = ctx.client()?.get_settings()?;
    let now = crate::utils::now_unix_secs();

    // Build provider → apps map from pins.
    let mut apps_by_provider: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for pin in ctx.client()?.list_pins()? {
        apps_by_provider
            .entry(pin.provider_id)
            .or_default()
            .push(pin.app);
    }

    for view in &providers {
        let p = &view.provider;
        let protocols = p
            .protocols
            .keys()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let failures = failures_str(
            view.metrics.consecutive_failures,
            view.metrics.last_attempt_at,
            now,
        );
        let apps = apps_by_provider
            .get(&p.id)
            .map(|v| v.join(", "))
            .unwrap_or_default();
        let mut line = format!(
            "{:<20} {:<20} {:>2} models   {}",
            p.id,
            protocols,
            p.models.len(),
            failures
        );
        if !apps.is_empty() {
            line.push_str(&format!("   used by: {apps}"));
        }
        println!("{line}");
    }
    println!("failure rule: {}", failure_rule(&settings));
    Ok(())
}

fn show(ctx: &Ctx, id: &str) -> anyhow::Result<()> {
    let view = ctx.client()?.get_provider(id)?;
    print_provider(&view.provider);

    println!("  metrics:");
    println!(
        "    failures: {}",
        failures_str(
            view.metrics.consecutive_failures,
            view.metrics.last_attempt_at,
            crate::utils::now_unix_secs()
        )
    );
    if let Some(balance) = &view.metrics.balance {
        println!("    balance: {}", money_str(balance));
    }
    if let Some(p50) = view.metrics.p50_latency {
        println!("    p50: {p50}ms");
    }
    let pinned_by: Vec<String> = ctx
        .client()?
        .list_pins()?
        .into_iter()
        .filter(|pin| pin.provider_id == id)
        .map(|pin| pin.app)
        .collect();
    if !pinned_by.is_empty() {
        println!("  apps: {}", pinned_by.join(", "));
    }

    Ok(())
}

fn delete(ctx: &Ctx, id: &str) -> anyhow::Result<()> {
    ctx.client()?.delete_provider(id)?;
    println!("deleted provider '{id}'");
    Ok(())
}

fn balance(ctx: &Ctx, id: &str) -> anyhow::Result<()> {
    let money = ctx.client()?.poll_balance(id)?;
    println!("{id}: balance = {}", money_str(&money));
    Ok(())
}

fn probe(ctx: &Ctx, id: &str) -> anyhow::Result<()> {
    let view = ctx.client()?.get_provider(id)?;
    if view.provider.protocols.is_empty() {
        println!("no protocols configured — nothing to probe");
    } else {
        print_probe(&view.provider);
    }

    Ok(())
}

fn url_override(flag: &str, url: String) -> anyhow::Result<String> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(url)
    } else {
        bail!("{flag} must be an http(s) URL")
    }
}

/// The API key from --key 'value', --key - (one stdin line), or --key-file.
/// Edit lets the key be absent, meaning "leave it unchanged".
fn resolve_key(key: Option<String>, key_file: Option<PathBuf>) -> anyhow::Result<Option<String>> {
    match (key, key_file) {
        (Some(_), Some(_)) => bail!("pass the API key as --key or --key-file, not both"),
        (Some(k), None) if k == "-" => {
            let mut line = String::new();
            std::io::stdin()
                .lock()
                .read_line(&mut line)
                .context("read the API key from stdin")?;
            Ok(Some(non_empty_key(line, "no API key on stdin")?))
        }
        (Some(k), None) => Ok(Some(k)),
        (None, Some(path)) => {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("read {}", path.display()))?;
            Ok(Some(non_empty_key(
                raw,
                &format!("{} holds no API key", path.display()),
            )?))
        }
        (None, None) => Ok(None),
    }
}

/// A key just read off disk/stdin: drop one trailing newline, reject empty.
fn non_empty_key(mut text: String, empty_msg: &str) -> anyhow::Result<String> {
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    if text.is_empty() {
        bail!("{empty_msg}");
    }
    Ok(text)
}

fn local_template(path: &Path) -> anyhow::Result<crate::provider::ProviderTemplate> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    // the file's template id is never stored — a template-free add only takes
    // its connection fields, so a placeholder suffices
    crate::provider::ProviderTemplate::parse(&raw, "local")
        .map_err(|e| anyhow!("invalid definition file {}: {e}", path.display()))
}

/// Render a single provider record (`provider show`). Secrets are masked.
fn print_provider(p: &Provider) {
    println!("{}", p.id);
    if let Some(template_id) = &p.template_id {
        println!("  template: {template_id}");
    }
    if let Some(name) = &p.name {
        println!("  name: {name}");
    }
    if p.protocols.is_empty() {
        println!("  protocols: (none)");
    } else {
        let protocols = p
            .protocols
            .iter()
            .map(|(protocol, setting)| {
                let base = protocol.to_string();
                match setting {
                    Endpoint::Path(path) => format!("{base} ({path})"),
                    Endpoint::Canonical => base,
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!("  protocols: {protocols}");
    }

    println!("  base_url: {}", p.base_url);
    println!("  key: {}", mask_secret(&p.key));
    let count = p.models.len();
    println!("  models ({count}):");
    if count == 0 {
        println!("    (none)");
    }

    for m in &p.models {
        let mut line = format!("    {}", m.id);
        if let Some(c) = m.context_window {
            line.push_str(&format!("  ctx={c}"));
        }
        if let Some(r) = m.reasoning {
            line.push_str(&format!("  reasoning={r}"));
        }
        println!("{line}");
    }
}

/// Mask a secret for display (never print a full token).
fn mask_secret(s: &str) -> String {
    if s.is_empty() {
        "(unset)".to_string()
    } else {
        "*".repeat(s.len().min(8))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_absent_and_literal() {
        assert_eq!(resolve_key(None, None).unwrap(), None);
        assert_eq!(
            resolve_key(Some("sk-abc".into()), None).unwrap(),
            Some("sk-abc".into())
        );
        assert!(resolve_key(Some("sk-a".into()), Some("key".into())).is_err());
    }

    #[test]
    fn key_file_strips_one_trailing_newline() {
        let path = std::env::temp_dir().join(format!("awitch-key-{}", std::process::id()));
        std::fs::write(&path, "sk-file\n").unwrap();
        let got = resolve_key(None, Some(path.clone())).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(got, Some("sk-file".into()));
    }

    #[test]
    fn empty_key_source_is_rejected() {
        let path = std::env::temp_dir().join(format!("awitch-empty-{}", std::process::id()));
        std::fs::write(&path, "\n").unwrap();
        let err = resolve_key(None, Some(path.clone())).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(err.to_string().contains("holds no API key"));
    }

    #[test]
    fn parse_protocol_entries() {
        assert_eq!(
            parse_protocol("anthropic").unwrap(),
            (Protocol::Anthropic, Endpoint::Canonical)
        );
        assert_eq!(
            parse_protocol("openai_chat=/v1/chat").unwrap(),
            (Protocol::OpenaiChat, Endpoint::Path("/v1/chat".into()))
        );
    }

    #[test]
    fn parse_protocol_rejects_bad_families() {
        assert!(parse_protocol("bogus").is_err());
        assert!(parse_protocol("=x").is_err()); // empty family
    }
}
