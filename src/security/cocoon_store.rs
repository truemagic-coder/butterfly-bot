use std::fs::File;
use std::path::Path;

use cocoon::Cocoon;

use crate::error::{ButterflyBotError, Result};

const BLOB_MAGIC: &[u8; 4] = b"BBC1";
const BLOB_VERSION: u8 = 1;
const CIPHER_CHA_CHA20_POLY1305: u8 = 1;

fn wrap_envelope(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + payload.len());
    out.extend_from_slice(BLOB_MAGIC);
    out.push(BLOB_VERSION);
    out.push(CIPHER_CHA_CHA20_POLY1305);
    out.extend_from_slice(payload);
    out
}

fn unwrap_envelope(raw: Vec<u8>, path: &Path) -> Result<Vec<u8>> {
    if raw.len() < 6 {
        return Err(ButterflyBotError::SecurityStorage(format!(
            "encrypted secret {} has invalid envelope length",
            path.to_string_lossy()
        )));
    }

    if &raw[0..4] != BLOB_MAGIC {
        return Err(ButterflyBotError::SecurityStorage(format!(
            "encrypted secret {} has invalid envelope magic",
            path.to_string_lossy()
        )));
    }

    let version = raw[4];
    if version != BLOB_VERSION {
        return Err(ButterflyBotError::SecurityStorage(format!(
            "encrypted secret {} has unsupported envelope version {}",
            path.to_string_lossy(),
            version
        )));
    }

    let cipher = raw[5];
    if cipher != CIPHER_CHA_CHA20_POLY1305 {
        return Err(ButterflyBotError::SecurityStorage(format!(
            "encrypted secret {} has unsupported cipher id {}",
            path.to_string_lossy(),
            cipher
        )));
    }

    Ok(raw[6..].to_vec())
}

pub fn load_secret(path: &Path, passphrase: &str) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let mut file = File::open(path).map_err(|e| {
        ButterflyBotError::SecurityStorage(format!(
            "failed to open encrypted secret {}: {e}",
            path.to_string_lossy()
        ))
    })?;

    let cocoon = Cocoon::new(passphrase.as_bytes());
    let decoded = cocoon.parse(&mut file).map_err(|e| {
        ButterflyBotError::SecurityStorage(format!(
            "failed to decrypt encrypted secret {}: {e:?}",
            path.to_string_lossy()
        ))
    })?;

    let payload = unwrap_envelope(decoded, path)?;

    let value = String::from_utf8(payload).map_err(|e| {
        ButterflyBotError::SecurityStorage(format!(
            "invalid utf8 in encrypted secret {}: {e}",
            path.to_string_lossy()
        ))
    })?;

    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ButterflyBotError::SecurityStorage(format!(
            "encrypted secret {} is empty",
            path.to_string_lossy()
        )));
    }

    Ok(Some(trimmed.to_string()))
}

pub fn persist_secret(path: &Path, passphrase: &str, value: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ButterflyBotError::SecurityStorage(format!(
                "failed to create encrypted secret directory {}: {e}",
                parent.to_string_lossy()
            ))
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }

    let mut file = File::create(path).map_err(|e| {
        ButterflyBotError::SecurityStorage(format!(
            "failed to create encrypted secret {}: {e}",
            path.to_string_lossy()
        ))
    })?;

    let mut cocoon = Cocoon::new(passphrase.as_bytes());
    let payload = wrap_envelope(value.as_bytes());
    cocoon
        .dump(payload, &mut file)
        .map_err(|e| {
            ButterflyBotError::SecurityStorage(format!(
                "failed to write encrypted secret {}: {e:?}",
                path.to_string_lossy()
            ))
        })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cocoon_roundtrip_secret() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.cocoon");

        persist_secret(&path, "test-passphrase", "super-secret").unwrap();
        let loaded = load_secret(&path, "test-passphrase").unwrap();

        assert_eq!(loaded.as_deref(), Some("super-secret"));
    }

    #[test]
    fn cocoon_wrong_passphrase_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.cocoon");

        persist_secret(&path, "pass-a", "super-secret").unwrap();
        let err = load_secret(&path, "pass-b").unwrap_err();

        assert!(format!("{err}").contains("failed to decrypt encrypted secret"));
    }

    #[test]
    fn cocoon_invalid_magic_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.cocoon");

        persist_secret(&path, "test-passphrase", "super-secret").unwrap();
        let mut file = File::open(&path).unwrap();
        let cocoon = Cocoon::new("test-passphrase".as_bytes());
        let mut decoded = cocoon.parse(&mut file).unwrap();
        decoded[0] = b'X';

        let mut out = File::create(&path).unwrap();
        let mut cocoon = Cocoon::new("test-passphrase".as_bytes());
        cocoon.dump(decoded, &mut out).unwrap();

        let err = load_secret(&path, "test-passphrase").unwrap_err();
        assert!(format!("{err}").contains("invalid envelope magic"));
    }

    #[test]
    fn cocoon_unsupported_version_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.cocoon");

        persist_secret(&path, "test-passphrase", "super-secret").unwrap();
        let mut file = File::open(&path).unwrap();
        let cocoon = Cocoon::new("test-passphrase".as_bytes());
        let mut decoded = cocoon.parse(&mut file).unwrap();
        decoded[4] = 99;

        let mut out = File::create(&path).unwrap();
        let mut cocoon = Cocoon::new("test-passphrase".as_bytes());
        cocoon.dump(decoded, &mut out).unwrap();

        let err = load_secret(&path, "test-passphrase").unwrap_err();
        assert!(format!("{err}").contains("unsupported envelope version"));
    }
}
