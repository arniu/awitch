use std::collections::HashSet;
use std::fmt;
use std::path::Path;

use anyhow::bail;

use super::print_probe;
use crate::api_types::ProviderAdd;
use crate::cli::args::MigrateCmd;
use crate::cli::ctx::Ctx;
use crate::external_apps::{Source, cc_switch};
use crate::provider::{Endpoint, Provider};

enum ProviderImport {
    Template { id: String, key: String },
    TemplateFree(ProviderAdd),
}

impl ProviderImport {
    fn id(&self) -> &str {
        match self {
            ProviderImport::Template { id, .. } => id,
            ProviderImport::TemplateFree(add) => add.id.as_deref().unwrap_or(""),
        }
    }

    fn set_id(&mut self, id: String) {
        match self {
            ProviderImport::Template { id: eid, .. } => *eid = id,
            ProviderImport::TemplateFree(add) => add.id = Some(id),
        }
    }
}

impl fmt::Display for ProviderImport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderImport::Template { id, key } => {
                write!(
                    f,
                    "provider: {:<12} template={id}  key={}",
                    id,
                    fingerprint(key)
                )
            }
            ProviderImport::TemplateFree(add) => {
                let protocols = add
                    .protocols
                    .keys()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                write!(
                    f,
                    "provider: {:<12} template-free base={}  protocols=[{protocols}]  key={}",
                    add.id.as_deref().unwrap_or(""),
                    add.base_url,
                    fingerprint(&add.key),
                )?;
                if add.models_url.is_none() && add.balance_url.is_none() {
                    write!(
                        f,
                        "\n  note: no provider template matches '{}' — balance endpoint unset",
                        add.id.as_deref().unwrap_or("")
                    )?;
                }
                Ok(())
            }
        }
    }
}

pub fn run(ctx: &Ctx, cmd: MigrateCmd) -> anyhow::Result<()> {
    match cmd {
        MigrateCmd::CcSwitch {
            db,
            dry_run,
            no_verify,
        } => cc_switch(ctx, db.as_deref(), dry_run, no_verify),
    }
}

fn cc_switch(ctx: &Ctx, db: Option<&Path>, dry_run: bool, no_verify: bool) -> anyhow::Result<()> {
    let sources = cc_switch::read(db)?;

    if sources.is_empty() {
        println!("no importable providers");
        return Ok(());
    }

    let total: usize = sources.iter().map(|s| s.providers.len()).sum();
    let imports = filter_existing(collect_imports(&sources), &existing_provider_ids(ctx)?);
    let skipped = total - imports.len();

    if dry_run {
        println!(
            "dry run: {} provider(s) to import, {skipped} skipped — nothing written",
            imports.len()
        );
        return Ok(());
    }

    let imported = execute_imports(ctx, &imports)?;
    println!("imported {}, skipped {skipped}", imported.len());

    if !no_verify {
        for provider in &imported {
            print_probe(provider);
        }
    }

    Ok(())
}

fn existing_provider_ids(ctx: &Ctx) -> anyhow::Result<HashSet<String>> {
    let providers = ctx.client()?.list_providers()?;
    Ok(providers.into_iter().map(|p| p.provider.id).collect())
}

// --- collect: source rows → import intents --------------------------------

enum TemplateMatch {
    Linked(String),
    Seeded(String),
}

/// Map external sources to awitch import intents.
fn collect_imports(sources: &[Source]) -> Vec<ProviderImport> {
    let mut out: Vec<ProviderImport> = sources
        .iter()
        .flat_map(|source| source.providers.iter().map(|sp| map_provider(source, sp)))
        .collect();

    suffix_collisions(&mut out);
    out
}

fn map_provider(source: &Source, sp: &crate::external_apps::Provider) -> ProviderImport {
    let id = slugify(&source.vendor, &sp.base_url);

    match template_match(&id, &sp.base_url) {
        Some(TemplateMatch::Linked(tid)) => ProviderImport::Template {
            id: tid,
            key: sp.key.clone(),
        },
        Some(TemplateMatch::Seeded(tid)) => {
            let template =
                crate::provider::builtin_template(&tid).expect("matched template must exist");
            template_free_import(
                id,
                sp,
                &source.vendor,
                template.models_url.clone(),
                template.balance_url.clone(),
            )
        }
        None => template_free_import(id, sp, &source.vendor, None, None),
    }
}

fn template_free_import(
    id: String,
    sp: &crate::external_apps::Provider,
    vendor: &str,
    models_url: Option<String>,
    balance_url: Option<String>,
) -> ProviderImport {
    ProviderImport::TemplateFree(ProviderAdd {
        id: Some(id),
        key: sp.key.clone(),
        base_url: sp.base_url.clone(),
        name: Some(vendor.to_string()),
        protocols: sp
            .protocols
            .iter()
            .map(|protocol| (*protocol, Endpoint::Canonical))
            .collect(),
        models_url,
        balance_url,
    })
}

fn suffix_collisions(imports: &mut [ProviderImport]) {
    let mut used: HashSet<String> = HashSet::new();
    for import in imports {
        let base = import.id().to_string();
        let mut id = base.clone();
        let mut n = 1;
        while !used.insert(id.clone()) {
            n += 1;
            id = format!("{base}-{n}");
        }
        import.set_id(id);
    }
}

/// The template matched for a group: the exact canonical base first (a link),
/// then the closest path-prefix match, then the vendor-name slug — both of
/// those seed only.
fn template_match(vendor_slug: &str, base_url: &str) -> Option<TemplateMatch> {
    let base = base_url.trim_end_matches('/');

    let mut seeded = None;
    for t in crate::provider::builtin_templates() {
        let tb = t.base_url.trim_end_matches('/');
        if base == tb {
            return Some(TemplateMatch::Linked(t.id));
        }
        if seeded.is_none()
            && base
                .strip_prefix(tb)
                .is_some_and(|rest| rest.starts_with('/'))
        {
            seeded = Some(t.id);
        }
    }

    seeded.map(TemplateMatch::Seeded).or_else(|| {
        crate::provider::is_builtin_template(vendor_slug)
            .then(|| TemplateMatch::Seeded(vendor_slug.to_string()))
    })
}

/// The provider id for a row: the display name lowercased to a slug (provider
/// template ids follow the same shape), falling back to the base URL's host
/// when the name is not sluggable.
fn slugify(name: &str, base_url: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug: Vec<&str> = slug.split('-').filter(|s| !s.is_empty()).collect();
    if slug.is_empty() {
        extract_host_label(base_url)
    } else {
        slug.join("-")
    }
}

/// The first label of a URL's host (`https://api.deepseek.com/x` → `api`).
fn extract_host_label(base_url: &str) -> String {
    base_url
        .split("://")
        .nth(1)
        .unwrap_or(base_url)
        .split('/')
        .next()
        .unwrap_or("provider")
        .split('.')
        .next()
        .unwrap_or("provider")
        .to_string()
}

// --- filter + execute ----------------------------------------------------

fn filter_existing(
    imports: Vec<ProviderImport>,
    existing: &HashSet<String>,
) -> Vec<ProviderImport> {
    imports
        .into_iter()
        .filter(|import| {
            if existing.contains(import.id()) {
                println!(
                    "skip: {} — already in the pool — delete it to re-import",
                    import.id()
                );
                false
            } else {
                true
            }
        })
        .collect()
}

fn execute_imports(ctx: &Ctx, imports: &[ProviderImport]) -> anyhow::Result<Vec<Provider>> {
    let mut imported = Vec::new();
    let mut errors = Vec::new();
    for import in imports {
        let result = ctx
            .client()
            .map_err(|e| format!("{}: {e}", import.id()))
            .and_then(|client| {
                let r = match import {
                    ProviderImport::Template { id, key } => {
                        client.instantiate_template(id, None, key)
                    }
                    ProviderImport::TemplateFree(add) => client.add_provider(add),
                };
                r.map_err(|e| format!("{}: {e}", import.id()))
            });
        match result {
            Ok(created) => {
                println!("imported {}", created.provider.id);
                imported.push(created.provider);
            }
            Err(e) => {
                eprintln!("import {e}");
                errors.push(import.id().to_string());
            }
        }
    }

    if !errors.is_empty() {
        bail!(
            "{} provider(s) failed to import: {}",
            errors.len(),
            errors.join(", ")
        );
    }

    Ok(imported)
}

fn fingerprint(key: &str) -> String {
    let head: String = key.chars().take(3).collect();
    let tail_len = key.len().min(4);
    let tail = &key[key.len() - tail_len..];
    format!("{head}…{tail}")
}
