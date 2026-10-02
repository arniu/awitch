use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::corpus;

#[derive(Debug, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub source: Vec<Source>,
}

#[derive(Debug, Deserialize)]
pub struct Source {
    pub repo: String,
    pub rev: String,
    pub license: String,
    #[serde(default = "default_license_path")]
    pub license_path: String,
    #[serde(default)]
    pub fixture: Vec<Fixture>,
}

#[derive(Debug, Deserialize)]
pub struct Fixture {
    pub protocol: Protocol,
    pub kind: Kind,
    pub path: String,
    pub local: String,
    #[serde(default)]
    pub extract: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Anthropic,
    OpenaiChat,
    OpenaiResponses,
}

/// How much a fixture can vouch for the protocol, strongest first. Where two
/// grades disagree about a field the stronger decides, so a spec example never
/// overrides the schema it sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Recorded,
    Synthetic,
    SpecSchema,
    SpecExample,
}

impl Protocol {
    /// The protocols in the order the crate's generated corpus lists them.
    pub const ALL: [Protocol; 3] = [
        Protocol::Anthropic,
        Protocol::OpenaiChat,
        Protocol::OpenaiResponses,
    ];

    pub fn dir(self) -> &'static str {
        match self {
            Protocol::Anthropic => "anthropic",
            Protocol::OpenaiChat => "openai_chat",
            Protocol::OpenaiResponses => "openai_responses",
        }
    }

    /// The name of the generated constant carrying this protocol's streams.
    pub fn streams(self) -> &'static str {
        match self {
            Protocol::Anthropic => "ANTHROPIC_STREAMS",
            Protocol::OpenaiChat => "CHAT_STREAMS",
            Protocol::OpenaiResponses => "RESPONSES_STREAMS",
        }
    }
}

impl Kind {
    /// The grades in precedence order.
    pub const ALL: [Kind; 4] = [
        Kind::Recorded,
        Kind::Synthetic,
        Kind::SpecSchema,
        Kind::SpecExample,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Recorded => "recorded",
            Kind::Synthetic => "synthetic",
            Kind::SpecExample => "spec-example",
            Kind::SpecSchema => "spec-schema",
        }
    }

    pub fn evidence(self) -> &'static str {
        match self {
            Kind::Recorded => "bytes the upstream SDK captured from the live API",
            Kind::Synthetic => "bytes the upstream authored, with fabricated ids",
            Kind::SpecExample => {
                "an example the published spec carries at the revision its source names"
            }
            Kind::SpecSchema => {
                "a `required` list the published spec carries, extracted and never transcribed"
            }
        }
    }

    pub fn is_extracted(self) -> bool {
        matches!(self, Kind::SpecExample | Kind::SpecSchema)
    }
}

impl Fixture {
    pub fn rel(&self) -> String {
        format!("{}/{}", self.protocol.dir(), self.local)
    }
}

impl Source {
    pub fn license_rel(&self) -> String {
        format!("licenses/{}.LICENSE", self.repo.replace('/', "-"))
    }
}

impl Manifest {
    pub fn load(root: &Path) -> Result<Manifest> {
        let path = root.join(corpus::SOURCES);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let manifest: Manifest =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn fixtures(&self) -> impl Iterator<Item = (&Source, &Fixture)> {
        self.source
            .iter()
            .flat_map(|source| source.fixture.iter().map(move |fixture| (source, fixture)))
    }

    pub fn repos(&self) -> Vec<String> {
        let mut repos = BTreeSet::new();
        for source in &self.source {
            repos.insert(source.repo.clone());
        }
        repos.into_iter().collect()
    }

    pub fn revs_of(&self, repo: &str) -> Vec<String> {
        let mut revs = BTreeSet::new();
        for source in &self.source {
            if source.repo == repo {
                revs.insert(source.rev.clone());
            }
        }
        revs.into_iter().collect()
    }

    pub fn files(&self) -> Vec<String> {
        let mut files: Vec<String> = self.fixtures().map(|(_, fixture)| fixture.rel()).collect();
        files.extend(self.source.iter().map(Source::license_rel));
        files.sort();
        files
    }

    fn validate(&self) -> Result<()> {
        if self.source.is_empty() {
            bail!("{}: no source", corpus::SOURCES);
        }
        let mut written = BTreeSet::new();
        for source in &self.source {
            check_rev(&source.repo, &source.rev)?;
            if source.license.is_empty() {
                bail!("{}: no license", source.repo);
            }
            for fixture in &source.fixture {
                if fixture.local.is_empty() {
                    bail!("{}: a fixture has no local name", source.repo);
                }
                if fixture.local.contains('/') {
                    bail!(
                        "{}: a fixture local is a file name under its protocol directory",
                        fixture.local
                    );
                }
                if !written.insert(fixture.rel()) {
                    bail!("{}: two fixtures write one file", fixture.rel());
                }
                if fixture.path.is_empty() {
                    bail!("{}: no upstream path", fixture.local);
                }
                if fixture.kind.is_extracted() != fixture.extract.is_some() {
                    bail!(
                        "{}: a {} fixture is {}extracted",
                        fixture.local,
                        fixture.kind.label(),
                        if fixture.kind.is_extracted() {
                            "always "
                        } else {
                            "never "
                        }
                    );
                }
            }
        }
        Ok(())
    }
}

fn default_license_path() -> String {
    "LICENSE".to_string()
}

fn check_rev(rel: &str, rev: &str) -> Result<()> {
    let sha = rev.len() == 40
        && rev
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !sha {
        bail!("{rel}: {rev} is not a commit sha");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(text: &str) -> Result<Manifest> {
        let manifest: Manifest = toml::from_str(text)?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn source(rev: &str) -> String {
        format!("[[source]]\nrepo = \"o/r\"\nrev = \"{rev}\"\nlicense = \"MIT\"\n")
    }

    #[test]
    fn a_branch_is_not_a_revision() {
        assert!(manifest(&source("main")).is_err());
        assert!(manifest(&source(&"a".repeat(40))).is_ok());
    }

    #[test]
    fn a_fixture_is_extracted_exactly_when_its_kind_says_so() {
        let declared = source(&"a".repeat(40));
        let stream = format!(
            "{declared}\n[[source.fixture]]\nprotocol = \"anthropic\"\nkind = \"recorded\"\n\
             path = \"p\"\nlocal = \"l.txt\"\n"
        );
        assert!(manifest(&stream).is_ok());

        let extracted = format!(
            "{declared}\n[[source.fixture]]\nprotocol = \"anthropic\"\nkind = \"spec-schema\"\n\
             path = \"p\"\nlocal = \"l.json\"\n"
        );
        assert!(manifest(&extracted).is_err());

        let both = format!(
            "{declared}\n[[source.fixture]]\nprotocol = \"anthropic\"\nkind = \"recorded\"\n\
             path = \"p\"\nlocal = \"l.txt\"\nextract = \"/a\"\n"
        );
        assert!(manifest(&both).is_err());
    }

    #[test]
    fn two_fixtures_never_write_one_file() {
        let declared = source(&"a".repeat(40));
        let one = format!(
            "{declared}\n[[source.fixture]]\nprotocol = \"anthropic\"\nkind = \"recorded\"\n\
             path = \"p\"\nlocal = \"l.txt\"\n"
        );
        assert!(manifest(&one).is_ok());

        let twice = format!(
            "{one}\n[[source.fixture]]\nprotocol = \"anthropic\"\nkind = \"recorded\"\n\
             path = \"other\"\nlocal = \"l.txt\"\n"
        );
        assert!(
            manifest(&twice).is_err(),
            "two fixtures may not share a path"
        );

        let elsewhere = format!(
            "{one}\n[[source.fixture]]\nprotocol = \"openai_chat\"\nkind = \"recorded\"\n\
             path = \"p\"\nlocal = \"l.txt\"\n"
        );
        assert!(
            manifest(&elsewhere).is_ok(),
            "another protocol writes its own directory"
        );

        let unnamed = format!(
            "{declared}\n[[source.fixture]]\nprotocol = \"anthropic\"\nkind = \"recorded\"\n\
             path = \"p\"\nlocal = \"\"\n"
        );
        assert!(manifest(&unnamed).is_err(), "a fixture names its file");
    }

    #[test]
    fn the_license_file_is_derived_from_the_repo() {
        let parsed = manifest(&source(&"a".repeat(40))).expect("the manifest parses");
        assert_eq!(
            parsed.source[0].license_rel(),
            "licenses/o-r.LICENSE",
            "a license lands beside the corpus under its own repo name"
        );
    }
}
