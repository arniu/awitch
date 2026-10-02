use std::path::Path;

pub(crate) fn read_or_create(path: &Path) -> anyhow::Result<String> {
    if let Ok(existing) = std::fs::read_to_string(path) {
        let t = existing.trim().to_string();
        if !t.is_empty() {
            return Ok(t);
        }
    }

    let token = crate::utils::random_hex(32);
    crate::utils::write_private_file_atomic(path, token.as_bytes())?;
    Ok(token)
}
