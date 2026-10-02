use super::resolve_apps;
use crate::cli::args::{PinAction, PinArgs};
use crate::cli::ctx::Ctx;
use crate::provider::Pin;

pub fn run(ctx: &Ctx, args: PinArgs) -> anyhow::Result<()> {
    match args.action {
        Some(PinAction::Remove { provider, app, all }) => {
            if all {
                ctx.client()?.remove_pins_for_provider(&provider)?;
                println!("removed provider '{provider}' from every app");
            } else if let Some(app) = app {
                ctx.client()?.remove_pin(&app, &provider)?;
                println!("unpinned provider '{provider}' from app '{app}'");
            }
        }
        Some(PinAction::Clear { app, all }) => {
            // --all names no app: the whole-table clear is clear_pins(None)
            match (all, app) {
                (true, _) => {
                    ctx.client()?.clear_pins(None)?;
                    println!("cleared all pins — auto routing");
                }
                (false, Some(app)) => {
                    ctx.client()?.clear_pins(Some(&app))?;
                    println!("cleared pin for app '{app}' — auto routing");
                }
                (false, None) => unreachable!("clap requires --app or --all"),
            }
        }
        Some(PinAction::List { app }) => list(ctx, app.as_deref())?,
        // No subcommand: the default action is pin (candidate-set add).
        None => {
            let provider = args.provider.ok_or_else(|| {
                anyhow::anyhow!("pin requires a provider id, or one of: remove, clear, list")
            })?;
            let apps = match (args.all, args.app) {
                (true, _) => resolve_apps(&[]),
                (false, Some(app)) => vec![app],
                (false, None) => anyhow::bail!("pin {provider} requires --app <APP> or --all"),
            };
            for app in apps {
                ctx.client()?.add_pin(&app, &provider)?;
                println!("pinned app '{app}' to provider '{provider}'");
            }
        }
    }
    Ok(())
}
fn list(ctx: &Ctx, app: Option<&str>) -> anyhow::Result<()> {
    let client = ctx.client()?;
    let lines: Vec<(String, Vec<String>)> = match app {
        Some(app) => vec![(app.to_string(), client.list_pinned_providers(app)?)],
        None => group_by_app(client.list_pins()?),
    };

    if lines.is_empty() {
        println!("no pins — auto routing");
    }

    for (app, providers) in lines {
        if providers.is_empty() {
            println!("{app}: auto routing");
        } else {
            println!("{app}: {}", providers.join(", "));
        }
    }
    Ok(())
}

/// The rows come grouped by app; fold them back into per-app lines.
fn group_by_app(rows: Vec<Pin>) -> Vec<(String, Vec<String>)> {
    let mut lines: Vec<(String, Vec<String>)> = Vec::new();
    for pin in rows {
        match lines.last_mut() {
            Some((app, providers)) if app == &pin.app => providers.push(pin.provider_id),
            _ => lines.push((pin.app, vec![pin.provider_id])),
        }
    }
    lines
}
