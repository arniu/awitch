use anyhow::{anyhow, bail};

use super::{resolve_apps, resolve_installed_apps, service, suffix_failed};
use crate::cli::args::{PointAction, PointArgs, ServiceCmd};
use crate::cli::ctx::Ctx;
use crate::pointing::{Target, app_by_name, apps};

pub fn run(ctx: &Ctx, args: PointArgs) -> anyhow::Result<()> {
    let PointArgs {
        app,
        all,
        no_service,
        action,
    } = args;
    match action {
        Some(PointAction::Undo { app, all }) => point_undo_apps(ctx, &targets(app, all)),
        Some(PointAction::Reset { app, all }) => point_reset_apps(ctx, &targets(app, all)),
        // No subcommand: the default action is point.
        None => {
            if !all && app.is_none() {
                bail!("point requires --app <APP> or --all");
            }
            point_apps(ctx, &targets(app, all), no_service)
        }
    }
}

/// --app names one app; --all encodes "every pointing app" as the empty
/// slice (the helpers resolve an empty list to all).
fn targets(app: Option<String>, all: bool) -> Vec<String> {
    if all {
        Vec::new()
    } else {
        app.into_iter().collect()
    }
}

/// Point apps at the gateway. Each point issues a fresh app key and writes the
/// target; keys are never revoked here — an old key stays live until it is
/// revoked explicitly (API-key convention).
fn point_apps(ctx: &Ctx, apps: &[String], no_service: bool) -> anyhow::Result<()> {
    ensure_service(ctx, no_service)?;
    let targets = resolve_installed_apps(&ctx.config_dir, apps);
    let (mut pointed, mut errors) = (0usize, Vec::new());
    for app in targets {
        if let Err(e) = point_app(ctx, &app) {
            eprintln!("point {app}: {e}");
            errors.push(app.to_string());
        } else {
            pointed += 1;
        }
    }

    if apps.is_empty() {
        println!("pointed {pointed}{}", suffix_failed(errors.len()));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        bail!(
            "{} app(s) failed to point: {}",
            errors.len(),
            errors.join(", ")
        )
    }
}

/// Connect, auto-installing the service first (unless `--no-service`).
fn ensure_service(ctx: &Ctx, no_service: bool) -> anyhow::Result<()> {
    if no_service {
        return reachable(ctx);
    }

    if reachable(ctx).is_err() {
        eprintln!("gateway not running — installing the service…");
        service::run(ctx, ServiceCmd::Install)?;
        return connect_waiting(ctx);
    }

    Ok(())
}

/// The explicit handshake — point decides offline vs. online from it (the
/// transparent handle alone never touches the network until a request).
fn reachable(ctx: &Ctx) -> anyhow::Result<()> {
    ctx.client()?.health().map_err(anyhow::Error::from)
}

fn connect_waiting(ctx: &Ctx) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut last = String::new();
    while std::time::Instant::now() < deadline {
        match reachable(ctx) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = e.to_string();
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
    }

    Err(anyhow!(last))
}

/// Reset corrupt undo records for apps whose records are damaged. Works even
/// for uninstalled apps; idempotent — a rerun after an interrupt picks up
/// where it left off.
fn point_reset_apps(ctx: &Ctx, apps: &[String]) -> anyhow::Result<()> {
    let (mut reset, mut errors) = (0usize, Vec::new());
    let targets = resolve_apps(apps);
    for app in targets {
        match point_reset_app(ctx, &app) {
            Ok(()) => reset += 1,
            Err(e) => {
                eprintln!("reset {app}: {e}");
                errors.push(app.to_string());
            }
        }
    }

    if !errors.is_empty() {
        bail!(
            "{} app(s) failed to reset: {}",
            errors.len(),
            errors.join(", ")
        );
    }

    println!("reset {reset} app(s)");

    Ok(())
}

fn point_reset_app(ctx: &Ctx, app: &str) -> anyhow::Result<()> {
    let reset = app_by_name(app)
        .ok_or_else(|| anyhow!("app '{app}': unknown app"))?
        .bind(&ctx.config_dir)
        .reset()?;
    if reset {
        println!("reset {app}");
    } else {
        println!("skipped {app} (no corrupt record)");
    }

    Ok(())
}

fn point_undo_apps(ctx: &Ctx, apps: &[String]) -> anyhow::Result<()> {
    let (mut undone, mut skipped, mut errors) = (0usize, 0usize, Vec::new());
    let targets = resolve_installed_apps(&ctx.config_dir, apps);
    for app in targets {
        match point_undo_app(ctx, &app) {
            Ok(true) => undone += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                eprintln!("undo {app}: {e}");
                errors.push(app.to_string());
            }
        }
    }

    if apps.is_empty() {
        println!(
            "undone {undone} app(s), skipped {skipped}{}",
            suffix_failed(errors.len())
        );
    }

    if errors.is_empty() {
        Ok(())
    } else {
        bail!(
            "{} app(s) failed to undo: {}",
            errors.len(),
            errors.join(", ")
        )
    }
}

fn point_undo_app(ctx: &Ctx, app: &str) -> anyhow::Result<bool> {
    let undone = app_by_name(app)
        .ok_or_else(|| anyhow!("app '{app}': unknown app"))?
        .bind(&ctx.config_dir)
        .undo()?;
    if undone {
        println!("undone {app}");
    } else {
        println!("skipped {app} (not pointed)");
    }

    Ok(undone)
}

fn point_app(ctx: &Ctx, app: &str) -> anyhow::Result<()> {
    // The gateway is local — its data URL is the only legitimate target.
    let gateway_url = ctx.url();
    let issued = ctx.client()?.issue_app_key(app)?;
    let target = Target {
        url: gateway_url.clone(),
        key: issued.key,
    };

    app_by_name(app)
        .ok_or_else(|| anyhow!("app '{app}': unknown app"))?
        .bind(&ctx.config_dir)
        .point(&target)?;

    println!("pointed {app} → {gateway_url}");

    Ok(())
}

/// Whether any agent is still pointed.
pub(super) fn any_pointed(ctx: &Ctx) -> bool {
    apps().any(|def| {
        def.bind(&ctx.config_dir)
            .check()
            .is_ok_and(|st| st.state.is_pointed())
    })
}
