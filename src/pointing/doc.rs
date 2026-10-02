use std::path::Path;

use anyhow::anyhow;

/// Key paths are dot-separated locators into a doc.
const PATH_SEP: char = '.';

/// The format a config file stores — derived from its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Env,
    Json,
    Toml,
    Yaml,
}

impl Format {
    pub fn from_path(path: &Path) -> Option<Format> {
        if path.file_name()? == ".env" {
            return Some(Format::Env);
        }

        match path.extension()?.to_str()? {
            "json" => Some(Format::Json),
            "toml" => Some(Format::Toml),
            "yaml" | "yml" => Some(Format::Yaml),
            _ => None,
        }
    }

    /// An empty doc in this format — for files that need not exist.
    pub fn empty_doc(self) -> Doc {
        Doc {
            format: self,
            root: serde_json::json!({}),
        }
    }

    pub fn parse(self, text: &str) -> anyhow::Result<Doc> {
        let root = match self {
            Format::Env => parse_env(text),
            Format::Json => json5::from_str(text).map_err(|e| anyhow!("bad JSON5: {e}"))?,
            Format::Toml => toml::from_str(text).map_err(|e| anyhow!("bad TOML: {e}"))?,
            Format::Yaml => serde_yaml::from_str(text).map_err(|e| anyhow!("bad YAML: {e}"))?,
        };

        if !root.is_object() {
            return Err(anyhow!("top level must be an object"));
        }

        Ok(Doc { format: self, root })
    }
}

#[derive(Debug)]
pub struct Doc {
    format: Format,
    root: serde_json::Value,
}

impl Doc {
    fn root_map(&mut self) -> &mut serde_json::Map<String, serde_json::Value> {
        self.root.as_object_mut().expect("root is an object")
    }

    pub fn get(&self, key_path: &str) -> Option<&serde_json::Value> {
        let mut cur = &self.root;
        for seg in key_path.split(PATH_SEP) {
            cur = cur.get(seg)?;
        }

        Some(cur)
    }

    pub fn set(&mut self, key_path: &str, new: serde_json::Value) -> anyhow::Result<()> {
        let mut segs = key_path.split(PATH_SEP);
        let last = segs.next_back().expect("non-empty key path");
        let mut cur = self.root_map();
        for seg in segs {
            if !cur.contains_key(seg) {
                cur.insert(seg.to_string(), serde_json::json!({}));
            }

            let Some(next) = cur.get_mut(seg).and_then(serde_json::Value::as_object_mut) else {
                return Err(anyhow!("cannot set {key_path}: '{seg}' must be an object"));
            };

            cur = next;
        }

        cur.insert(last.to_string(), new);

        Ok(())
    }

    pub fn remove(&mut self, key_path: &str) -> bool {
        let mut segs = key_path.split(PATH_SEP);
        let last = segs.next_back().expect("non-empty key path");
        let mut cur = self.root_map();
        for seg in segs {
            let Some(next) = cur.get_mut(seg).and_then(serde_json::Value::as_object_mut) else {
                return false;
            };
            cur = next;
        }

        cur.remove(last).is_some()
    }

    pub fn trim_empty(&mut self, key_path: &str) -> bool {
        let mut segs = key_path.split(PATH_SEP);
        let last = segs.next_back().expect("non-empty key path");
        let mut cur = self.root_map();
        for seg in segs {
            let Some(next) = cur.get_mut(seg).and_then(serde_json::Value::as_object_mut) else {
                return false;
            };

            cur = next;
        }

        let empty = cur
            .get(last)
            .and_then(serde_json::Value::as_object)
            .is_some_and(serde_json::Map::is_empty);
        if empty {
            cur.remove(last);
        }

        empty
    }

    pub fn to_text(&self) -> anyhow::Result<String> {
        let text = match self.format {
            Format::Env => env_lines(&self.root)?,
            Format::Json => serde_json::to_string_pretty(&self.root)
                .map_err(|e| anyhow!("serialize JSON: {e}"))?,
            Format::Yaml => {
                serde_yaml::to_string(&self.root).map_err(|e| anyhow!("serialize YAML: {e}"))?
            }
            Format::Toml => {
                let value =
                    toml::Value::try_from(&self.root).map_err(|e| anyhow!("to TOML value: {e}"))?;
                toml::to_string(&value).map_err(|e| anyhow!("serialize TOML: {e}"))?
            }
        };

        Ok(text)
    }
}

fn parse_env(text: &str) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((k, v)) = line.split_once('=') {
            map.insert(
                k.trim().to_string(),
                serde_json::Value::String(v.trim().to_string()),
            );
        }
    }

    serde_json::Value::Object(map)
}

fn env_lines(value: &serde_json::Value) -> anyhow::Result<String> {
    let Some(obj) = value.as_object() else {
        return Err(anyhow!("env doc must be an object"));
    };

    let mut lines = String::new();
    for (k, v) in obj {
        let v = v
            .as_str()
            .ok_or_else(|| anyhow!("env value for '{k}' must be a string"))?;
        lines.push_str(&format!("{k}={v}\n"));
    }

    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> serde_json::Value {
        serde_json::Value::String(v.to_string())
    }

    #[test]
    fn format_from_path_infers_by_name() {
        assert_eq!(Format::from_path(Path::new(".env")), Some(Format::Env));
        assert_eq!(Format::from_path(Path::new("a/.env")), Some(Format::Env));
        assert_eq!(Format::from_path(Path::new("cfg.json")), Some(Format::Json));
        assert_eq!(Format::from_path(Path::new("cfg.toml")), Some(Format::Toml));
        assert_eq!(Format::from_path(Path::new("cfg.yaml")), Some(Format::Yaml));
        assert_eq!(Format::from_path(Path::new("cfg.yml")), Some(Format::Yaml));
        assert_eq!(Format::from_path(Path::new("cfg.txt")), None);
        assert_eq!(Format::from_path(Path::new("noext")), None);
    }

    #[test]
    fn get_set_remove_round_trip_json() {
        let mut doc = Format::Json.parse(r#"{"env":{"MODEL":"x"}}"#).unwrap();
        doc.set("env.ANTHROPIC_BASE_URL", s("http://gw")).unwrap();
        assert_eq!(doc.get("env.ANTHROPIC_BASE_URL"), Some(&s("http://gw")));
        assert_eq!(doc.get("env.MODEL"), Some(&s("x")));
        assert!(doc.remove("env.ANTHROPIC_BASE_URL"));
        assert!(!doc.remove("env.ANTHROPIC_BASE_URL"));
        assert_eq!(doc.get("env.ANTHROPIC_BASE_URL"), None);
    }

    #[test]
    fn get_set_remove_round_trip_toml() {
        let mut doc = Format::Toml
            .parse("model = \"x\"\n[model_providers.custom]\nname = \"d\"\n")
            .unwrap();
        doc.set(
            "model_providers.awitch",
            serde_json::json!({"base_url": "http://gw", "env_key": "K"}),
        )
        .unwrap();
        assert_eq!(
            doc.get("model_providers.awitch"),
            Some(&serde_json::json!({"base_url": "http://gw", "env_key": "K"}))
        );
        assert_eq!(doc.get("model_providers.custom.name"), Some(&s("d")));
        assert_eq!(doc.get("model"), Some(&s("x")));
        assert!(doc.remove("model_providers.awitch"));
        assert_eq!(doc.get("model_providers.awitch"), None);
    }

    #[test]
    fn get_set_remove_round_trip_env() {
        let mut doc = Format::Env.parse("A=1\nB=2\n").unwrap();
        doc.set("C", s("3")).unwrap();
        assert_eq!(doc.get("A"), Some(&s("1")));
        assert_eq!(doc.get("C"), Some(&s("3")));
        assert!(doc.remove("B"));
        assert_eq!(doc.get("B"), None);
        assert!(!doc.remove("B"));
    }

    #[test]
    fn get_set_round_trip_yaml() {
        let mut doc = Format::Yaml.parse("model:\n  default: deep\n").unwrap();
        doc.set("model.provider", s("awitch")).unwrap();
        assert_eq!(doc.get("model.provider"), Some(&s("awitch")));
        assert_eq!(doc.get("model.default"), Some(&s("deep")));
        assert!(doc.remove("model.provider"));
        assert_eq!(doc.get("model.provider"), None);
    }

    #[test]
    fn trim_empty_removes_only_empty_containers() {
        let mut doc = Format::Json.parse(r#"{"env":{},"other":{"x":1}}"#).unwrap();
        assert!(doc.trim_empty("env"));
        assert_eq!(doc.get("env"), None);
        assert_eq!(doc.get("other.x"), Some(&serde_json::json!(1)));

        let mut kept = Format::Json.parse(r#"{"env":{"x":1}}"#).unwrap();
        assert!(!kept.trim_empty("env"));
        assert_eq!(kept.get("env.x"), Some(&serde_json::json!(1)));
    }

    #[test]
    fn json_parses_leniently() {
        // Agent configs are JSON5: comments and trailing commas are legal.
        let doc = Format::Json
            .parse("{\n  // a comment\n  \"models\": {\"providers\": {}},\n}\n")
            .unwrap();
        assert_eq!(doc.get("models.providers"), Some(&serde_json::json!({})));
    }

    #[test]
    fn parse_rejects_a_non_object_root() {
        let err = Format::Json.parse("[1,2]").unwrap_err();
        assert_eq!(err.to_string(), "top level must be an object");
    }

    #[test]
    fn set_through_scalar_container_errors_and_keeps_original() {
        // Claude settings where `env` is a scalar (not an object): pointing
        // into it must error, never replace the scalar with an object.
        let mut doc = Format::Json
            .parse(r#"{"env": "ANTHROPIC_AUTH_TOKEN=tok"}"#)
            .unwrap();
        let err = doc
            .set("env.ANTHROPIC_BASE_URL", s("http://gw"))
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "cannot set env.ANTHROPIC_BASE_URL: 'env' must be an object"
        );
        assert_eq!(doc.get("env"), Some(&s("ANTHROPIC_AUTH_TOKEN=tok")));
    }

    #[test]
    fn set_toml_through_scalar_container_errors() {
        let mut doc = Format::Toml.parse("model = \"x\"\n").unwrap();
        let err = doc.set("model.custom", s("d")).unwrap_err();
        assert!(
            err.to_string().contains("'model' must be an object"),
            "{err}"
        );
    }

    #[test]
    fn toml_round_trip_preserves_key_order() {
        let mut doc = Format::Toml
            .parse("zebra = \"1\"\nalpha = \"2\"\n")
            .unwrap();
        doc.set("middle", s("3")).unwrap();
        let raw = doc.to_text().unwrap();
        assert!(
            raw.find("zebra").unwrap() < raw.find("alpha").unwrap(),
            "{raw}"
        );
    }
}
