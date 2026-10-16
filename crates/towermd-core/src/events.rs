use sha2::{Digest, Sha256};
pub fn derived_eid(commit: &str, kind: &str) -> Result<String, String> {
    validate_commit(commit)?;
    validate_token("type", kind)?;
    let mut hash = Sha256::new();
    hash.update(commit.as_bytes());
    hash.update([0]);
    hash.update(kind.as_bytes());
    Ok(format!("git:{}", hex::encode(hash.finalize())))
}

fn validate_commit(commit: &str) -> Result<(), String> {
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("commit must be a full 40 or 64 character hexadecimal hash".into());
    }
    Ok(())
}
fn validate_token(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("{label} must be a nonempty token"));
    }
    Ok(())
}
