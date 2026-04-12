use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::NamedTempFile;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub provider_source: String,
    pub host: String,
    pub user: String,
    pub access_token: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_probe_at: Option<String>,
    pub last_error: Option<String>,
    pub last_device_code: Option<String>,
}

impl SessionRecord {
    pub fn sanitized(mut self) -> Self {
        self.access_token = sanitize_token_field(&self.access_token);
        self
    }
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    pub fn new_default() -> Result<Self> {
        let home = home_dir()?;
        let dir = home.join(".config/code-review");
        fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("failed to set permissions on {}", dir.display()))?;
        Ok(Self { path: dir.join("session.json") })
    }

    pub fn from_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Option<SessionRecord>> {
        if !self.path.exists() {
            return Ok(None);
        }

        let lock = self.lock_file()?;
        lock.lock_shared()
            .with_context(|| format!("failed to acquire shared lock for {}", self.path.display()))?;

        let content = fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        let record: SessionRecord = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", self.path.display()))?;
        Ok(Some(record.sanitized()))
    }

    pub fn save(&self, record: &SessionRecord) -> Result<()> {
        let parent = self.parent_dir()?;
        fs::create_dir_all(&parent)?;
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("failed to set permissions on {}", parent.display()))?;

        let lock = self.lock_file()?;
        lock.lock_exclusive()
            .with_context(|| format!("failed to acquire exclusive lock for {}", self.path.display()))?;

        let record = record.clone().sanitized();
        let content = serde_json::to_vec_pretty(&record)?;

        let mut tmp = NamedTempFile::new_in(&parent)
            .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
        tmp.as_file_mut()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to set temp permissions in {}", parent.display()))?;
        use std::io::Write;
        tmp.write_all(&content)
            .with_context(|| format!("failed to write temp session file for {}", self.path.display()))?;
        tmp.as_file_mut().sync_all()
            .with_context(|| format!("failed to sync temp session file for {}", self.path.display()))?;
        tmp.persist(&self.path)
            .map_err(|e| anyhow::anyhow!("failed to persist temp session file to {}: {}", self.path.display(), e.error))?;
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to set permissions on {}", self.path.display()))?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        let lock = self.lock_file()?;
        lock.lock_exclusive()
            .with_context(|| format!("failed to acquire exclusive lock for {}", self.path.display()))?;
        if self.path.exists() {
            fs::remove_file(&self.path)
                .with_context(|| format!("failed to remove {}", self.path.display()))?;
        }
        Ok(())
    }

    fn parent_dir(&self) -> Result<PathBuf> {
        self.path
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| anyhow::anyhow!("session path has no parent: {}", self.path.display()))
    }

    fn lock_file(&self) -> Result<File> {
        let parent = self.parent_dir()?;
        fs::create_dir_all(&parent)?;
        let lock_path = parent.join("session.lock");
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(&lock_path)
            .with_context(|| format!("failed to open lock file {}", lock_path.display()))?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to set permissions on {}", lock_path.display()))?;
        Ok(file)
    }
}

pub fn now_string() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    secs.to_string()
}

pub fn mask_token(token: &str) -> String {
    if token.is_empty() {
        return "<empty>".into();
    }
    if token == "configured" {
        return "conf...ured".into();
    }
    if token.len() <= 8 {
        return "****".into();
    }
    format!("{}...{}", &token[..4], &token[token.len() - 4..])
}

pub fn sanitize_token_field(token: &str) -> String {
    let trimmed = token.trim();
    if trimmed.is_empty() || trimmed == "configured" {
        return "configured".into();
    }
    mask_token(trimmed)
}

pub fn home_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn mask_works() {
        assert_eq!(mask_token("abcdef"), "****");
        assert_eq!(mask_token("abcdefghijkl"), "abcd...ijkl");
    }

    #[test]
    fn sanitize_never_keeps_raw_secret() {
        assert_eq!(sanitize_token_field("sk-1234567890abcdef"), "sk-1...cdef");
        assert_eq!(sanitize_token_field(""), "configured");
    }

    #[test]
    fn save_is_sanitized() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let store = SessionStore::from_path(dir.path().join("session.json"));
        let rec = SessionRecord {
            provider_source: "copilot-cli".into(),
            host: "https://github.com".into(),
            user: "delta".into(),
            access_token: "super-secret-token-value".into(),
            created_at: now_string(),
            updated_at: now_string(),
            last_probe_at: None,
            last_error: None,
            last_device_code: None,
        };
        store.save(&rec).expect("save session");
        let raw = fs::read_to_string(store.path()).expect("read session file");
        assert!(!raw.contains("super-secret-token-value"));
    }

    #[test]
    fn concurrent_saves_do_not_corrupt_file() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("session.json");
        let mut handles = Vec::new();
        for i in 0..8 {
            let store = SessionStore::from_path(path.clone());
            handles.push(thread::spawn(move || {
                let rec = SessionRecord {
                    provider_source: "copilot-cli".into(),
                    host: "https://github.com".into(),
                    user: format!("user{}", i),
                    access_token: format!("secret-token-{}", i),
                    created_at: now_string(),
                    updated_at: now_string(),
                    last_probe_at: None,
                    last_error: None,
                    last_device_code: None,
                };
                store.save(&rec).expect("concurrent save");
            }));
        }
        for h in handles {
            h.join().expect("thread join");
        }
        let store = SessionStore::from_path(path);
        let loaded = store.load().expect("load after concurrent saves").expect("session exists");
        assert!(loaded.user.starts_with("user"));
        assert!(!loaded.access_token.contains("secret-token"));
    }
}
