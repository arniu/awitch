use anyhow::bail;
use serde_json::Value;

use crate::cli::args::SettingsArgs;
use crate::cli::ctx::Ctx;

pub fn run(ctx: &Ctx, args: SettingsArgs) -> anyhow::Result<()> {
    let Some(field) = args.field else {
        let s = serde_json::to_value(ctx.client()?.get_settings()?)?;
        for (k, v) in s.as_object().into_iter().flat_map(|o| o.iter()) {
            println!("{k} = {}", render_value(v));
        }

        return Ok(());
    };

    let Some(value) = args.value else {
        find_field(&field)?;
        let s = serde_json::to_value(ctx.client()?.get_settings()?)?;
        match s.get(&field) {
            Some(v) => println!("{field} = {}", render_value(v)),
            None => bail!("setting '{field}' has no stored value"),
        }

        return Ok(());
    };

    find_field(&field)?;
    let body = serde_json::json!({ field.clone(): value });
    ctx.client()?.patch_settings(&body)?;
    println!("{field} = {value}");
    Ok(())
}

fn find_field(key: &str) -> anyhow::Result<&'static dyn crate::settings::FieldExt> {
    crate::settings::SETTINGS
        .iter()
        .find(|s| s.key() == key)
        .copied()
        .ok_or_else(|| {
            let keys: Vec<_> = crate::settings::SETTINGS.iter().map(|s| s.key()).collect();
            anyhow::anyhow!("unknown setting '{key}' (valid: {})", keys.join(", "))
        })
}

fn render_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
