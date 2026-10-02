use super::value_spec::ValueSpec;

#[derive(Debug, Clone, Copy)]
pub struct App {
    pub name: &'static str,
    pub base_dir: Dir,
    pub files: &'static [File],
}

#[derive(Debug, Clone, Copy)]
pub struct Dir {
    pub default: &'static str,
    pub env_override: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub struct File {
    pub path: &'static str,
    pub patches: &'static [Patch],
    pub trim_empty: &'static [&'static str],
}

/// What pointing patches at one managed key, when pointed: the key path and
/// the value template to install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Patch {
    pub key_path: &'static str,
    pub value: ValueSpec,
}

pub struct KeyPath(&'static str);

pub const fn key_path(key_path: &'static str) -> KeyPath {
    assert!(!key_path.is_empty(), "a key path cannot be empty");
    KeyPath(key_path)
}

impl KeyPath {
    pub const fn set_url(self) -> Patch {
        Patch {
            key_path: self.0,
            value: ValueSpec::Url,
        }
    }

    pub const fn set_key(self) -> Patch {
        Patch {
            key_path: self.0,
            value: ValueSpec::Key,
        }
    }

    pub const fn set_pointer(self, pointer: &'static str) -> Patch {
        Patch {
            key_path: self.0,
            value: ValueSpec::Pointer(pointer),
        }
    }

    pub const fn set_entry(self, template: &'static str) -> Patch {
        Patch {
            key_path: self.0,
            value: ValueSpec::Entry(template),
        }
    }
}

pub const fn dir(default: &'static str) -> Dir {
    Dir {
        default,
        env_override: None,
    }
}

impl Dir {
    pub const fn env_override(mut self, env: &'static str) -> Dir {
        self.env_override = Some(env);
        self
    }
}

pub const fn file(path: &'static str, patches: &'static [Patch]) -> File {
    assert!(!path.is_empty(), "a file path cannot be empty");
    File {
        path,
        patches,
        trim_empty: &[],
    }
}

impl File {
    pub const fn trim_empty(mut self, trim_empty: &'static [&'static str]) -> File {
        self.trim_empty = trim_empty;
        self
    }
}
