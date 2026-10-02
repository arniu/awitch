use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub key_path: String,
    pub value: serde_json::Value,
    pub old_value: Option<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEdits {
    pub path: String,
    pub edits: Vec<Edit>,
}

#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    #[serde(default)]
    checksum: u64,
    #[serde(default)]
    files: Vec<FileEdits>,
}

impl Record {
    const VERSION: u32 = 1;

    fn new(files: Vec<FileEdits>) -> Record {
        Record {
            version: Record::VERSION,
            checksum: checksum(&files),
            files,
        }
    }

    fn verify(&self) -> bool {
        self.version == Record::VERSION && self.checksum == checksum(&self.files)
    }
}

fn checksum(files: &[FileEdits]) -> u64 {
    let mut v = serde_json::to_value(files).unwrap_or_default();
    v.sort_all_objects(); // stable digest across writers' key order
    let s = serde_json::to_string(&v).unwrap_or_default();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    hash
}

/// Serialize files into the record text: versioned and checksummed.
pub(super) fn encode(files: &[FileEdits]) -> Result<String, toml::ser::Error> {
    toml::to_string(&Record::new(files.to_vec()))
}

/// Parse record text: a verified record's content, or None when broken.
pub(super) fn decode(text: &str) -> Option<Vec<FileEdits>> {
    match toml::from_str::<Record>(text) {
        Ok(r) if r.verify() => Some(r.files),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn edit(key_path: &str, value: serde_json::Value) -> Edit {
        Edit {
            key_path: key_path.into(),
            value,
            old_value: None,
        }
    }

    #[test]
    fn two_files_may_share_a_key_path() {
        let files = vec![
            FileEdits {
                path: "a.json".into(),
                edits: vec![edit("k", json!({"src": "a"}))],
            },
            FileEdits {
                path: "b.json".into(),
                edits: vec![edit("k", json!({"src": "b"}))],
            },
        ];

        let text = encode(&files).unwrap();
        match decode(&text) {
            Some(got) => assert_eq!(got, files),
            None => panic!("expected a verified record, got corrupt"),
        }
    }

    #[test]
    fn tampered_content_reads_as_corrupt() {
        let files = [FileEdits {
            path: "a.json".into(),
            edits: vec![edit("k", json!("v"))],
        }];

        let text = encode(&files).unwrap();
        let tampered = text.replace("key_path = \"k\"", "key_path = \"k \"");
        assert_ne!(tampered, text);
        assert!(decode(&tampered).is_none());
    }
}
