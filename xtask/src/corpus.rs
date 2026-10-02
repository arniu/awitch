use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::extract;
use crate::fetch;
use crate::manifest::{Kind, Manifest, Protocol};

pub const SOURCES: &str = "fixtures/sources.toml";
const CHECKSUMS: &str = "fixtures/CHECKSUMS.sha256";
const PROVENANCE: &str = "fixtures/PROVENANCE.md";
const MODULE: &str = "src/protocol/corpus.rs";

pub fn root() -> Result<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .context("xtask sits in the repository root")?;
    Ok(root.to_path_buf())
}

pub fn sync(named: Option<&str>) -> Result<()> {
    let root = root()?;
    let manifest = Manifest::load(&root)?;
    let allow = |rel: &str| named.is_some_and(|name| name == rel);
    if let Some(name) = named
        && !manifest.files().iter().any(|rel| rel == name)
    {
        bail!("{SOURCES}: no source names {name}");
    }
    write_corpus(&root, &manifest, allow)?;
    write_generated(&root, &manifest)?;
    verify(&root, &manifest)?;
    println!(
        "fixtures: {} files match sources.toml",
        manifest.files().len()
    );
    Ok(())
}

pub fn check() -> Result<()> {
    let root = root()?;
    let manifest = Manifest::load(&root)?;
    verify(&root, &manifest)?;
    println!(
        "fixtures: {} files match CHECKSUMS.sha256 and sources.toml",
        manifest.files().len()
    );
    Ok(())
}

pub fn update(named: Option<&str>) -> Result<()> {
    let root = root()?;
    restore_sources(&root, || refetch(&root, named))
}

fn restore_sources(root: &Path, run: impl FnOnce() -> Result<()>) -> Result<()> {
    let path = root.join(SOURCES);
    let written =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    if let Err(error) = run() {
        std::fs::write(&path, &written).with_context(|| format!("writing {}", path.display()))?;
        println!("{SOURCES}: the revisions are the ones the run started with");
        return Err(error);
    }
    Ok(())
}

fn refetch(root: &Path, named: Option<&str>) -> Result<()> {
    let manifest = Manifest::load(root)?;
    for repo in manifest.repos() {
        if named.is_some_and(|name| name != repo) {
            continue;
        }
        let head = fetch::head(&repo)?;
        let revs = manifest.revs_of(&repo);
        if revs.iter().all(|rev| *rev == head) {
            println!("{repo}: {SOURCES} already names {head}");
            continue;
        }
        println!("{repo}: {} -> {head}", revs.join(" "));
        update_revs(root, &revs, &head)?;
    }
    let manifest = Manifest::load(root)?;
    write_corpus(root, &manifest, |_| true)?;
    write_generated(root, &manifest)?;
    verify(root, &manifest)?;
    println!(
        "fixtures: {} files match sources.toml",
        manifest.files().len()
    );
    Ok(())
}

fn update_revs(root: &Path, revs: &[String], head: &str) -> Result<()> {
    let path = root.join(SOURCES);
    let mut text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    for rev in revs {
        text = text.replace(&format!("rev = \"{rev}\""), &format!("rev = \"{head}\""));
    }
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn write_corpus(root: &Path, manifest: &Manifest, allow: impl Fn(&str) -> bool) -> Result<()> {
    let mut cache = Cache::default();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for (source, fixture) in manifest.fixtures() {
        let rel = fixture.rel();
        let fetched = cache.get(&source.repo, &source.rev, &fixture.path)?;
        let bytes = match fixture.extract.as_deref() {
            Some(pointer) => {
                let document = extract::document(fetched, &rel)?;
                let value = extract::pointer(&document, pointer, &rel)?;
                extract::render(value, &rel, fixture.kind)?
            }
            None => {
                validate_stream(&rel, fetched)?;
                fetched.to_vec()
            }
        };
        entries.push((rel, bytes));
    }
    for source in &manifest.source {
        let bytes = cache.get(&source.repo, &source.rev, &source.license_path)?;
        entries.push((source.license_rel(), bytes.to_vec()));
    }
    for (rel, bytes) in &entries {
        write(root, rel, bytes, allow(rel))?;
    }
    Ok(())
}

/// One fetches openapi.json once, however many fixtures extract from it.
#[derive(Default)]
struct Cache(BTreeMap<(String, String, String), Vec<u8>>);

impl Cache {
    fn get(&mut self, repo: &str, rev: &str, path: &str) -> Result<&[u8]> {
        let key = (repo.to_string(), rev.to_string(), path.to_string());
        if !self.0.contains_key(&key) {
            let bytes = fetch::document(repo, rev, path)?;
            self.0.insert(key.clone(), bytes);
        }
        self.0
            .get(&key)
            .map(Vec::as_slice)
            .context("the document was just fetched")
    }
}

fn validate_stream(rel: &str, bytes: &[u8]) -> Result<()> {
    let text = std::str::from_utf8(bytes)
        .with_context(|| format!("{rel}: a stream fixture is not utf-8"))?;
    if !framed(text) {
        bail!("{rel}: a stream fixture carries no data: or event: line");
    }
    Ok(())
}

/// Whether a body is frames rather than a document. This is the corpus' own test
/// of a stream, and what the generated lists select on.
fn framed(text: &str) -> bool {
    text.lines()
        .any(|line| line.starts_with("data:") || line.starts_with("event:"))
}

fn write(root: &Path, rel: &str, bytes: &[u8], allow: bool) -> Result<()> {
    let path = root.join("fixtures").join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    match std::fs::read(&path) {
        Ok(existing) if existing != bytes => {
            if !allow {
                bail!(
                    "{rel}: the vendored bytes differ from the revision sources.toml names; \
                     run `cargo xtask fixtures sync {rel}`"
                );
            }
            println!(
                "updated {rel}: {} -> {}",
                short(&sha256_hex(&existing)),
                short(&sha256_hex(bytes))
            );
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => println!("added {rel}"),
        Err(error) => {
            return Err(error).with_context(|| format!("reading {}", path.display()));
        }
    }
    std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn write_generated(root: &Path, manifest: &Manifest) -> Result<()> {
    let mut checksums = String::new();
    for rel in manifest.files() {
        checksums.push_str(&checksum_row(&rel, &sha256_hex(&read(root, &rel)?)));
    }
    std::fs::write(root.join(CHECKSUMS), checksums)
        .with_context(|| format!("writing {CHECKSUMS}"))?;
    std::fs::write(root.join(PROVENANCE), provenance())
        .with_context(|| format!("writing {PROVENANCE}"))?;
    std::fs::write(
        root.join(MODULE),
        corpus_module(&vendored_corpus(root, manifest)?),
    )
    .with_context(|| format!("writing {MODULE}"))?;
    Ok(())
}

/// Every fixture sources.toml names, with the bytes vendored for it.
fn vendored_corpus(root: &Path, manifest: &Manifest) -> Result<Vec<(Protocol, String, Vec<u8>)>> {
    manifest
        .fixtures()
        .map(|(_, fixture)| {
            let rel = fixture.rel();
            let bytes = read(root, &rel)?;
            Ok((fixture.protocol, rel, bytes))
        })
        .collect()
}

/// The crate's view of the corpus: each protocol's streams, named from sources.toml
/// rather than listed by hand, so a stream the corpus gains is swept the revision
/// it lands in.
fn corpus_module(corpus: &[(Protocol, String, Vec<u8>)]) -> String {
    let mut text = String::from(
        "//! The vendored corpus as the tests see it: every fixture of a protocol whose\n\
         //! bytes are frames — its streams — whatever the model makes of them.\n\
         //! Generated by `cargo xtask fixtures sync`. Do not edit, and do not reformat:\n\
         //! each list is skipped, so the text here is the text the tool writes.\n",
    );
    for protocol in Protocol::ALL {
        let mut streams: Vec<&str> = corpus
            .iter()
            .filter(|(other, _, bytes)| {
                *other == protocol && std::str::from_utf8(bytes).is_ok_and(framed)
            })
            .map(|(_, rel, _)| rel.as_str())
            .collect();
        streams.sort();
        text.push_str(&format!(
            "\n#[rustfmt::skip]\npub const {}: &[(&str, &str)] = &[\n",
            protocol.streams()
        ));
        for rel in streams {
            text.push_str(&format!(
                "    (\n        \"{rel}\",\n        include_str!(\"../../fixtures/{rel}\"),\n    ),\n"
            ));
        }
        text.push_str("];\n");
    }
    text
}

fn verify_module(root: &Path, manifest: &Manifest) -> Result<()> {
    let path = root.join(MODULE);
    let written =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    if written != corpus_module(&vendored_corpus(root, manifest)?) {
        bail!(
            "{MODULE}: a stream sources.toml names is not listed here; run `cargo xtask fixtures sync`"
        );
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn checksum_row(rel: &str, digest: &str) -> String {
    format!("{digest}  {rel}\n")
}

fn provenance() -> String {
    let mut text = String::from(
        "# Provenance\n\n\
         Every file in this directory except `sources.toml` is fetched or derived by\n\
         `cargo xtask fixtures sync` from the revisions `sources.toml` names; a\n\
         fixture is never written by hand. `CHECKSUMS.sha256` carries each vendored\n\
         file's digest, and `licenses/` carries each source's LICENSE notice.\n\n\
         The corpus is a dated snapshot of those revisions, not a canon: its date is the\n\
         date of the commit that last changed a fixture, and a revision bump is a\n\
         reviewable commit rather than a download. `kind` records the evidence grade,\n\
         strongest first.\n\n\
         | Kind | Evidence |\n\
         |---|---|\n",
    );
    for kind in Kind::ALL {
        text.push_str(&format!("| `{}` | {} |\n", kind.label(), kind.evidence()));
    }
    text
}

fn verify(root: &Path, manifest: &Manifest) -> Result<()> {
    let recorded = read_checksums(root)?;
    let mut claimed = BTreeSet::new();
    for rel in manifest.files() {
        let digest = sha256_hex(&read(root, &rel)?);
        match recorded.get(&rel) {
            None => bail!("{rel} is named by sources.toml but has no CHECKSUMS.sha256 row"),
            Some(row) if *row != digest => {
                bail!("{rel}: {digest} does not match {row} in CHECKSUMS.sha256");
            }
            Some(_) => {}
        }
        claimed.insert(rel);
    }
    for rel in recorded.keys() {
        if !claimed.contains(rel) {
            bail!("{rel} is in CHECKSUMS.sha256 but no source names it");
        }
    }
    for rel in walk(&root.join("fixtures"))? {
        if rel == SOURCES.strip_prefix("fixtures/").unwrap_or(SOURCES)
            || rel == "CHECKSUMS.sha256"
            || rel == "PROVENANCE.md"
        {
            continue;
        }
        if !claimed.contains(&rel) {
            bail!("{rel} is vendored but no source names it");
        }
    }
    verify_read_by_tests(root, manifest)?;
    verify_provenance(root)?;
    verify_module(root, manifest)?;
    Ok(())
}

fn verify_provenance(root: &Path) -> Result<()> {
    let path = root.join(PROVENANCE);
    let written =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    if written != provenance() {
        bail!("{PROVENANCE}: not the notice sources.toml carries; run `cargo xtask fixtures sync`");
    }
    Ok(())
}

/// A fixture no source file names is evidence nothing asserts against, and a
/// vendored stream missing from a test's list passes every other check.
fn verify_read_by_tests(root: &Path, manifest: &Manifest) -> Result<()> {
    let referenced = includes(root)?;
    for (_, fixture) in manifest.fixtures() {
        let rel = fixture.rel();
        if !referenced.contains(&format!("fixtures/{rel}")) {
            bail!("{rel} is vendored but no source file names it");
        }
    }
    Ok(())
}

/// Every `include_str!` the crate's own sources and integration tests carry, as
/// repository-relative paths. A target built at compile time (`concat!`, `env!`)
/// is out of a scan's reach, so this is a floor rather than a proof.
fn includes(root: &Path) -> Result<BTreeSet<String>> {
    let mut referenced = BTreeSet::new();
    for rel in sources(root)? {
        let path = root.join(&rel);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let dir = Path::new(&rel).parent().unwrap_or(Path::new(""));
        for target in include_targets(&text) {
            referenced.insert(resolve(dir, target)?);
        }
    }
    Ok(referenced)
}

fn sources(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    for dir in ["src", "tests", "benches", "examples"] {
        for rel in walk(&root.join(dir))? {
            if rel.ends_with(".rs") {
                files.push(format!("{dir}/{rel}"));
            }
        }
    }
    files.sort();
    Ok(files)
}

/// The literal targets of a source's `include_str!` calls. The scan tracks
/// comments and string literals, so a path quoted in prose or inside a string is
/// not a call.
fn include_targets(text: &str) -> Vec<&str> {
    const KEYWORD: &str = "include_str!";
    let bytes = text.as_bytes();
    let mut targets = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii() {
            i += 1;
        } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i = text[i..]
                .find('\n')
                .map_or(bytes.len(), |step| i + step + 1);
        } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = text[i..]
                .find("*/")
                .map_or(bytes.len(), |step| i + step + 2);
        } else if bytes[i] == b'"' {
            i = end_of_string(bytes, i + 1);
        } else if bytes[i] == b'\'' {
            i += char_literal(text, i).unwrap_or(1);
        } else if let Some(length) = raw_string(text, i) {
            i += length;
        } else if let Some(target) = text[i..]
            .strip_prefix(KEYWORD)
            .and_then(|rest| literal_argument(rest))
        {
            targets.push(target);
            i += KEYWORD.len();
        } else {
            i += 1;
        }
    }
    targets
}

/// The argument of a macro call, when it is one plain string literal.
fn literal_argument(after: &str) -> Option<&str> {
    let rest = after.trim_start().strip_prefix('(')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn end_of_string(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// The length of the char literal that starts at `i`, when one does: an
/// apostrophe that opens no literal is a lifetime's.
fn char_literal(text: &str, i: usize) -> Option<usize> {
    let rest = text[i..].strip_prefix('\'')?;
    let end = match rest.chars().next()? {
        '\\' => match rest.chars().nth(1)? {
            'u' => rest.find('}')? + 1,
            _ => 2,
        },
        '\'' => return None,
        first => first.len_utf8(),
    };
    rest[end..].starts_with('\'').then_some(end + 2)
}

/// The length of the raw string that starts at `i`, when one does: `r"…"`,
/// `r#"…"#`, and the `br…` forms the scan reaches at its `r`.
fn raw_string(text: &str, i: usize) -> Option<usize> {
    let rest = text[i..].strip_prefix('r')?;
    let hashes = rest.len() - rest.trim_start_matches('#').len();
    let body = rest[hashes..].strip_prefix('"')?;
    let close = format!("\"{}", "#".repeat(hashes));
    Some(1 + hashes + 1 + body.find(&close)? + close.len())
}

fn resolve(dir: &Path, target: &str) -> Result<String> {
    let mut parts: Vec<String> = Vec::new();
    for part in dir.join(target).components() {
        match part {
            std::path::Component::ParentDir => {
                if parts.pop().is_none() {
                    bail!("{target}: an include reaches outside the repository");
                }
            }
            std::path::Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => {}
        }
    }
    Ok(parts.join("/"))
}

fn read(root: &Path, rel: &str) -> Result<Vec<u8>> {
    let path = root.join("fixtures").join(rel);
    std::fs::read(&path).with_context(|| format!("{rel} is named by sources.toml but not vendored"))
}

fn read_checksums(root: &Path) -> Result<BTreeMap<String, String>> {
    let path = root.join(CHECKSUMS);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    parse_checksums(&text)
}

fn parse_checksums(text: &str) -> Result<BTreeMap<String, String>> {
    let mut rows = BTreeMap::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let (digest, rel) = line
            .split_once("  ")
            .with_context(|| format!("{CHECKSUMS}: {line} is not a checksum row"))?;
        rows.insert(rel.to_string(), digest.to_string());
    }
    Ok(rows)
}

fn walk(dir: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    collect(dir, dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<String>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("reading {}", dir.display()));
        }
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            collect(root, &path, files)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            files.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

fn short(digest: &str) -> &str {
    digest.get(..12).unwrap_or(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vendored_corpus_verifies() {
        check().expect("the checked-in fixtures match CHECKSUMS.sha256 and sources.toml");
    }

    fn resolved(dir: &str, text: &str) -> Vec<String> {
        include_targets(text)
            .into_iter()
            .map(|target| resolve(Path::new(dir), target))
            .collect::<Result<Vec<String>>>()
            .expect("the include stays inside the repository")
    }

    #[test]
    fn a_stream_fixture_without_frames_is_rejected() {
        assert!(validate_stream("anthropic/x.txt", b"{\"type\":\"ping\"}\n").is_err());
        assert!(validate_stream("anthropic/x.txt", b"data: {\"a\":1}\n").is_ok());
        assert!(validate_stream("anthropic/x.txt", b"event: message_stop\n").is_ok());
    }

    #[test]
    fn a_checksum_row_round_trips_through_the_reader() {
        let digest = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let rows = parse_checksums(&checksum_row("anthropic/basic_response.txt", digest))
            .expect("a written row parses");
        assert_eq!(
            rows.get("anthropic/basic_response.txt").map(String::as_str),
            Some(digest),
            "a written row reads back as the digest of the file it names"
        );
        assert!(
            parse_checksums("e3b0c442 anthropic/basic_response.txt").is_err(),
            "a row the writer did not write is not a checksum row"
        );
    }

    #[test]
    fn an_include_resolves_to_the_file_it_names() {
        let text = concat!(
            "const A: &str = include_str!(\"../../../fixtures/anthropic/basic_response.txt\");\n",
            "const B: &str = include_str!(\"../../openai_responses/stream_sample.sse\");\n",
        );
        assert_eq!(
            resolved("src/protocol/translate", text),
            [
                "fixtures/anthropic/basic_response.txt",
                "src/openai_responses/stream_sample.sse",
            ],
            "an include is resolved against the file that carries it"
        );
    }

    #[test]
    fn a_path_in_prose_is_not_an_include() {
        let text = concat!(
            "// include_str!(\"../../fixtures/anthropic/basic_response.txt\")\n",
            "/* include_str!(\"../../fixtures/anthropic/basic_response.txt\") */\n",
            "const A: &str = \"include_str!(\\\"../../fixtures/anthropic/basic_response.txt\\\")\";\n",
            "const B: &str = include_str!( \"../../fixtures/anthropic/basic_response.txt\");\n",
        );
        assert_eq!(
            resolved("src/protocol", text),
            ["fixtures/anthropic/basic_response.txt"],
            "only the call is an include, however the macro is spaced"
        );
    }

    #[test]
    fn a_quote_in_a_raw_string_opens_no_string() {
        let text = concat!(
            "const A: &str = r#\"one \" two\"#;\n",
            "const B: &str = include_str!(\"../../fixtures/anthropic/basic_response.txt\");\n",
        );
        assert_eq!(
            resolved("src/protocol", text),
            ["fixtures/anthropic/basic_response.txt"],
            "a quote the scan meets inside a raw string opens nothing"
        );
    }

    #[test]
    fn a_quote_in_a_char_literal_opens_no_string() {
        let text = concat!(
            "const A: char = '\"';\n",
            "const B: &str = include_str!(\"../../fixtures/anthropic/basic_response.txt\");\n",
        );
        assert_eq!(
            resolved("src/protocol", text),
            ["fixtures/anthropic/basic_response.txt"],
            "a quote the scan meets inside a char literal opens nothing"
        );
    }

    #[test]
    fn a_lifetime_is_not_a_char_literal() {
        let text = concat!(
            "struct A<'a> { text: &'a str }\n",
            "const B: &str = include_str!(\"../../fixtures/anthropic/basic_response.txt\");\n",
        );
        assert_eq!(
            resolved("src/protocol", text),
            ["fixtures/anthropic/basic_response.txt"],
            "an apostrophe that opens no literal does not swallow what follows it"
        );
    }

    #[test]
    fn an_include_that_escapes_the_repository_is_rejected() {
        assert!(
            resolve(Path::new("src"), "../../../../fixtures/x.txt").is_err(),
            "a path that pops past the repository root names no file"
        );
        assert!(resolve(Path::new("src/protocol"), "../../fixtures/x.txt").is_ok());
    }

    fn vendored(protocol: Protocol, rel: &str, bytes: &[u8]) -> (Protocol, String, Vec<u8>) {
        (protocol, rel.to_string(), bytes.to_vec())
    }

    #[test]
    fn the_module_lists_every_stream_of_a_protocol_and_no_document() {
        let corpus = [
            vendored(
                Protocol::Anthropic,
                "anthropic/basic_response.txt",
                b"event: message_start\ndata: {\"type\":\"message_start\"}\n",
            ),
            vendored(
                Protocol::Anthropic,
                "anthropic/chat_request_required.json",
                b"[\n  \"id\"\n]\n",
            ),
            vendored(
                Protocol::OpenaiChat,
                "openai_chat/tool_call.sse",
                b"data: {\"id\":\"x\"}\n",
            ),
        ];
        let module = corpus_module(&corpus);
        for protocol in Protocol::ALL {
            assert!(
                module.contains(&format!("pub const {}: ", protocol.streams())),
                "{} is listed however many of its streams the sources carry",
                protocol.streams()
            );
        }
        assert!(
            module.contains("(\n        \"anthropic/basic_response.txt\",\n        include_str!(\"../../fixtures/anthropic/basic_response.txt\"),\n    ),"),
            "a stream is listed under the name the tests look it up by, with the path the fixture sits at"
        );
        assert!(
            !module.contains("chat_request_required.json"),
            "a fixture that is a document is not a stream"
        );
    }

    #[test]
    fn the_module_lists_a_protocols_streams_sorted_by_name() {
        let corpus = [
            vendored(Protocol::Anthropic, "anthropic/b.sse", b"data: {}\n"),
            vendored(Protocol::Anthropic, "anthropic/a.sse", b"data: {}\n"),
        ];
        let module = corpus_module(&corpus);
        let first = module
            .find("anthropic/a.sse")
            .expect("a stream a source carries is listed");
        let second = module
            .find("anthropic/b.sse")
            .expect("a stream a source carries is listed");
        assert!(
            first < second,
            "the streams are listed in the order of their names"
        );
    }

    const TREE_SOURCES: &str = "\
[[source]]
repo = \"o/r\"
rev = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"
license = \"MIT\"

[[source.fixture]]
protocol = \"anthropic\"
kind = \"recorded\"
path = \"p\"
local = \"basic_response.txt\"

[[source.fixture]]
protocol = \"anthropic\"
kind = \"spec-schema\"
path = \"p\"
local = \"chat_request_required.json\"
extract = \"/required\"
";

    const TREE_FILES: [(&str, &str); 3] = [
        (
            "anthropic/basic_response.txt",
            "event: message_start\ndata: {\"type\":\"message_start\"}\n",
        ),
        ("anthropic/chat_request_required.json", "[\n  \"id\"\n]\n"),
        ("licenses/o-r.LICENSE", "MIT\n"),
    ];

    /// The stream is listed by the generated module; the document has to be
    /// named by a source file of its own.
    const TREE_PROBE: &str =
        "const A: &str = include_str!(\"../fixtures/anthropic/chat_request_required.json\");\n";

    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str) -> Tree {
            let root = std::env::temp_dir().join(format!("awitch-xtask-{name}"));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("fixtures")).expect("the tree is writable");
            std::fs::create_dir_all(root.join("src/protocol")).expect("the tree is writable");
            std::fs::write(root.join(SOURCES), TREE_SOURCES).expect("the sources are written");
            let manifest = Manifest::load(&root).expect("the tree's sources parse");
            for (rel, bytes) in TREE_FILES {
                write(&root, rel, bytes.as_bytes(), true).expect("the fixture is written");
            }
            std::fs::write(root.join("src/probe.rs"), TREE_PROBE).expect("the probe is written");
            write_generated(&root, &manifest).expect("the generated files are written");
            Tree(root)
        }

        fn verify(&self) -> Result<()> {
            verify(&self.0, &Manifest::load(&self.0)?)
        }

        fn put(&self, rel: &str, bytes: &str) {
            let path = self.0.join(rel);
            std::fs::write(&path, bytes).unwrap_or_else(|error| panic!("writing {rel}: {error}"));
        }
    }

    #[test]
    fn a_coherent_tree_verifies() {
        assert!(
            Tree::new("coherent").verify().is_ok(),
            "what the emitters write is what the checks read"
        );
    }

    #[test]
    fn a_fixture_the_checksums_do_not_carry_is_rejected() {
        let tree = Tree::new("rowless");
        let rows: Vec<String> = std::fs::read_to_string(tree.0.join(CHECKSUMS))
            .expect("the tree carries checksums")
            .lines()
            .filter(|row| !row.ends_with("anthropic/basic_response.txt"))
            .map(str::to_string)
            .collect();
        tree.put(CHECKSUMS, &(rows.join("\n") + "\n"));
        assert!(
            tree.verify().is_err(),
            "a fixture with no row is not verified"
        );
    }

    #[test]
    fn a_fixture_whose_bytes_moved_is_rejected() {
        let tree = Tree::new("moved");
        tree.put(
            "fixtures/anthropic/chat_request_required.json",
            "[\n  \"other\"\n]\n",
        );
        assert!(
            tree.verify().is_err(),
            "the bytes on disk are the ones the checksums name"
        );
    }

    #[test]
    fn a_vendored_file_no_source_names_is_rejected() {
        let tree = Tree::new("stray");
        tree.put("fixtures/anthropic/stray.txt", "data: {}\n");
        assert!(
            tree.verify().is_err(),
            "a file under fixtures is claimed by a source"
        );
    }

    #[test]
    fn a_fixture_no_source_file_reads_is_rejected() {
        let tree = Tree::new("unread");
        tree.put("src/probe.rs", "const A: &str = \"nothing\";\n");
        assert!(
            tree.verify().is_err(),
            "a vendored fixture nothing reads is evidence nothing asserts against"
        );
    }

    #[test]
    fn a_failed_run_leaves_the_revisions_it_started_with() {
        let tree = Tree::new("rollback");
        let before = std::fs::read(tree.0.join(SOURCES)).expect("the tree carries sources");
        let moved = "b".repeat(40);
        let error = restore_sources(&tree.0, || {
            update_revs(&tree.0, &["a".repeat(40)], &moved)?;
            bail!("the fetch failed")
        })
        .expect_err("a failed run reports the failure");
        assert_eq!(error.to_string(), "the fetch failed");
        assert_eq!(
            std::fs::read(tree.0.join(SOURCES)).expect("the tree carries sources"),
            before,
            "sources.toml holds the revisions the run started with"
        );
    }

    #[test]
    fn write_refuses_bytes_the_source_does_not_carry() {
        let tree = Tree::new("guard");
        let rel = "anthropic/basic_response.txt";
        let moved = b"data: {}\n";
        assert!(
            write(&tree.0, rel, moved, false).is_err(),
            "a fixture is not quietly rewritten"
        );
        assert!(
            write(&tree.0, rel, moved, true).is_ok(),
            "a run that names it rewrites it"
        );
        assert_eq!(
            std::fs::read(tree.0.join("fixtures").join(rel)).expect("the fixture is there"),
            moved,
            "the named run writes the bytes it was given"
        );
    }
}
