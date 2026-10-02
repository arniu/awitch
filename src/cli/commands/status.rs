use super::{point, resolve_apps};
use crate::cli::args::StatusArgs;
use crate::cli::ctx::Ctx;
use crate::pointing::app_by_name;

pub fn run(ctx: &Ctx, args: StatusArgs) -> anyhow::Result<()> {
    if let Some(app) = &args.app {
        return show_app(ctx, app);
    }

    show_global(ctx)
}

fn show_global(ctx: &Ctx) -> anyhow::Result<()> {
    let providers = ctx.client()?.list_providers()?;
    let settings = ctx.client()?.get_settings()?;

    // The app set is local (pointing); pins are the only per-app fact the
    // gateway holds, so an unpinned app simply has no rows here.
    let mut pin_by_app: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for pin in ctx.client()?.list_pins()? {
        pin_by_app.entry(pin.app).or_default().push(pin.provider_id);
    }

    let mut apps = resolve_apps(&[]);
    for app in pin_by_app.keys() {
        if !apps.contains(app) {
            apps.push(app.clone());
        }
    }
    apps.sort();

    // Gateway section.
    println!("gateway:");
    println!("  mode: {}", settings.routing_mode.as_str());
    // Facts, not a verdict: which providers routing ejects is a per-request
    // decision (ADR-0009); here we report what was observed and the parameters.
    let with_failures = providers
        .iter()
        .filter(|p| p.metrics.consecutive_failures > 0)
        .count();
    if with_failures == 0 {
        println!("  providers: {} total", providers.len());
    } else {
        println!(
            "  providers: {} total, {} with failures",
            providers.len(),
            with_failures
        );
    }
    println!("  failure rule: {}", super::failure_rule(&settings));

    // Apps section.
    if !apps.is_empty() {
        println!();
        println!("apps:");
    }
    for app in &apps {
        let pointing = match app_by_name(app) {
            Some(def) => match def.bind(&ctx.config_dir).check() {
                Ok(st) => st.state.as_str().to_string(),
                Err(err) => format!("error: {err:#}"),
            },
            None => "unknown".to_string(),
        };
        let pin = match pin_by_app.get(app) {
            Some(providers) if !providers.is_empty() => providers.join(", "),
            _ => "auto".to_string(),
        };
        println!("  {app}: {pointing}, pin={pin}");
    }

    show_hints(providers.len(), point::any_pointed(ctx), !apps.is_empty());
    Ok(())
}

fn show_app(ctx: &Ctx, app: &str) -> anyhow::Result<()> {
    let providers = ctx.client()?.list_pinned_providers(app)?;

    println!("app: {app}");

    match app_by_name(app) {
        Some(def) => match def.bind(&ctx.config_dir).check() {
            Ok(st) => println!("  pointing: {}", st.state.as_str()),
            Err(err) => println!("  pointing: error: {err:#}"),
        },
        None => println!("  pointing: unknown app"),
    }

    let pin = if providers.is_empty() {
        "auto".to_string()
    } else {
        providers.join(", ")
    };
    println!("  pin: {pin}");

    Ok(())
}

/// Fresh-setup nudges, separated from the status block by a blank line.
fn show_hints(n_providers: usize, any_pointed: bool, has_apps: bool) {
    let no_providers = n_providers == 0;
    let nothing_pointed = has_apps && !any_pointed;
    if no_providers || nothing_pointed {
        println!();
    }

    if no_providers {
        println!("hint: no providers configured");
        println!("  run 'awitch provider add --base-url <url> --key <key>' or import data");
    }

    if nothing_pointed {
        println!("hint: nothing pointed");
        println!("  run 'awitch point' to route your agents through the gateway");
    }
}
