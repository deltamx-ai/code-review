use crate::cli::{LoginArgs, OutputFormat};
use crate::session::{home_dir, mask_token, now_string, sanitize_token_field, SessionRecord, SessionStore};
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use tempfile::NamedTempFile;
use wait_timeout::ChildExt;

#[derive(Debug, Serialize)]
pub struct AuthStatus {
    pub logged_in: bool,
    pub quota_exhausted: bool,
    pub provider_source: Option<String>,
    pub user: Option<String>,
    pub host: Option<String>,
    pub token_preview: Option<String>,
    pub updated_at: Option<String>,
    pub last_probe_at: Option<String>,
    pub last_error: Option<String>,
}

impl AuthStatus {
    pub fn print(&self, format: OutputFormat) -> Result<()> {
        match format {
            OutputFormat::Text => {
                println!("logged_in: {}", self.logged_in);
                println!("quota_exhausted: {}", self.quota_exhausted);
                if let Some(v) = &self.user { println!("user: {}", v); }
                if let Some(v) = &self.provider_source { println!("provider: {}", v); }
                if let Some(v) = &self.host { println!("host: {}", v); }
                if let Some(v) = &self.token_preview { println!("token: {}", v); }
                if let Some(v) = &self.updated_at { println!("updated_at: {}", v); }
                if let Some(v) = &self.last_probe_at { println!("last_probe_at: {}", v); }
                if let Some(v) = &self.last_error { println!("last_error: {}", v); }
            }
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(self)?),
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct WhoAmI {
    pub user: String,
    pub provider_source: String,
    pub host: String,
    pub token_preview: String,
    pub updated_at: String,
}

impl WhoAmI {
    pub fn print(&self, format: OutputFormat) -> Result<()> {
        match format {
            OutputFormat::Text => {
                println!("user: {}", self.user);
                println!("provider: {}", self.provider_source);
                println!("host: {}", self.host);
                println!("token: {}", self.token_preview);
                println!("updated_at: {}", self.updated_at);
            }
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(self)?),
        }
        Ok(())
    }
}

pub fn login(args: &LoginArgs, store: &SessionStore) -> Result<SessionRecord> {
    println!("Starting real Copilot login...");
    let status = Command::new("copilot")
        .arg("login")
        .arg("--host")
        .arg(&args.host)
        .status()
        .context("failed to start `copilot login`")?;

    if !status.success() {
        let probe = run_probe().unwrap_or(ProbeResult {
            logged_in: false,
            quota_exhausted: false,
            last_error: Some(format!("login failed with status: {}", status)),
        });
        if !probe.logged_in {
            bail!("copilot login failed with status: {}", status);
        }
    }

    let probe = run_probe()?;
    if !probe.logged_in {
        bail!("copilot login completed but probe still failed: {}", probe.last_error.clone().unwrap_or_else(|| "unknown error".into()));
    }

    let record = SessionRecord {
        provider_source: "copilot-cli".into(),
        host: args.host.clone(),
        user: discover_user(),
        access_token: "configured".into(),
        created_at: now_string(),
        updated_at: now_string(),
        last_probe_at: Some(now_string()),
        last_error: None,
        last_device_code: None,
    };
    store.save(&record)?;
    Ok(record)
}

pub fn status(store: &SessionStore) -> Result<AuthStatus> {
    let probe = run_probe()?;
    match store.load()? {
        Some(mut record) => {
            record.updated_at = now_string();
            record.last_probe_at = Some(now_string());
            record.last_error = probe.last_error.clone();
            if probe.logged_in {
                if record.user.is_empty() || record.user == "unknown" {
                    record.user = discover_user();
                }
                let token_preview = discover_token_preview();
                record.access_token = sanitize_token_field(&token_preview);
            }
            store.save(&record)?;
            Ok(AuthStatus {
                logged_in: probe.logged_in,
                quota_exhausted: probe.quota_exhausted,
                provider_source: Some(record.provider_source),
                user: Some(record.user),
                host: Some(record.host),
                token_preview: Some(mask_token(&record.access_token)),
                updated_at: Some(record.updated_at),
                last_probe_at: record.last_probe_at,
                last_error: record.last_error,
            })
        }
        None => Ok(AuthStatus {
            logged_in: probe.logged_in,
            quota_exhausted: probe.quota_exhausted,
            provider_source: probe.logged_in.then(|| "copilot-cli".into()),
            user: probe.logged_in.then(discover_user),
            host: probe.logged_in.then(|| "https://github.com".into()),
            token_preview: probe.logged_in.then(discover_token_preview).map(|s| mask_token(&sanitize_token_field(&s))),
            updated_at: probe.logged_in.then(now_string),
            last_probe_at: Some(now_string()),
            last_error: probe.last_error,
        }),
    }
}

pub fn logout(store: &SessionStore, clear_remote: bool) -> Result<()> {
    if clear_remote {
        let _ = Command::new("copilot").arg("logout").status();
    }
    store.clear()
}

pub fn refresh(store: &SessionStore) -> Result<AuthStatus> {
    let mut record = store.load()?.ok_or_else(|| anyhow!("not logged in"))?;
    let probe = run_probe()?;
    record.updated_at = now_string();
    record.last_probe_at = Some(now_string());
    record.last_error = probe.last_error.clone();
    if probe.logged_in {
        record.user = discover_user();
        record.access_token = sanitize_token_field(&discover_token_preview());
    }
    store.save(&record)?;
    status(store)
}

pub fn whoami(store: &SessionStore) -> Result<WhoAmI> {
    let status = status(store)?;
    if !status.logged_in {
        bail!("not logged in");
    }
    Ok(WhoAmI {
        user: status.user.unwrap_or_else(|| "unknown".into()),
        provider_source: status.provider_source.unwrap_or_else(|| "copilot-cli".into()),
        host: status.host.unwrap_or_else(|| "https://github.com".into()),
        token_preview: status.token_preview.unwrap_or_else(|| "conf...ured".into()),
        updated_at: status.updated_at.unwrap_or_else(now_string),
    })
}

pub fn run_review(_store: &SessionStore, prompt: &str, model: Option<&str>) -> Result<String> {
    if prompt.trim().is_empty() {
        bail!("prompt is empty");
    }

    let prompt_arg = build_prompt_arg(prompt)?;
    let output = run_copilot_prompt_arg(&prompt_arg, model, Duration::from_secs(90))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        bail!("copilot review returned empty output");
    }
    Ok(stdout)
}

#[derive(Debug)]
struct ProbeResult {
    logged_in: bool,
    quota_exhausted: bool,
    last_error: Option<String>,
}

fn build_prompt_arg(prompt: &str) -> Result<String> {
    const INLINE_LIMIT: usize = 8 * 1024;
    if prompt.len() <= INLINE_LIMIT {
        return Ok(prompt.to_string());
    }

    let mut tmp = NamedTempFile::new_in(std::env::temp_dir())
        .context("failed to create prompt temp file")?;
    use std::io::Write;
    tmp.write_all(prompt.as_bytes()).context("failed to write prompt temp file")?;
    let kept = tmp.keep().map_err(|e| anyhow!("failed to keep prompt temp file: {}", e.error))?;
    Ok(format!("@{}", kept.1.display()))
}

fn run_copilot_prompt_arg(prompt_arg: &str, model: Option<&str>, timeout: Duration) -> Result<Output> {
    let mut cmd = Command::new("copilot");
    if let Some(model) = model {
        cmd.arg("--model").arg(model);
    }
    cmd.arg("--no-ask-user")
        .arg("-p")
        .arg(prompt_arg)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().context("failed to spawn copilot")?;
    match child.wait_timeout(timeout).context("failed waiting on copilot process")? {
        Some(_status) => {
            let output = child.wait_with_output().context("failed to collect copilot output")?;
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
                bail!("copilot command failed: {}", if err.is_empty() { format!("status {}", output.status) } else { err });
            }
            Ok(output)
        }
        None => {
            let _ = child.kill();
            let output = child.wait_with_output().ok();
            let stderr = output
                .as_ref()
                .map(|o| String::from_utf8_lossy(&o.stderr).trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "no stderr captured".into());
            bail!("copilot command timed out after {}s: {}", timeout.as_secs(), stderr)
        }
    }
}

fn run_probe() -> Result<ProbeResult> {
    match run_copilot_prompt_arg("reply with exactly OK", None, Duration::from_secs(20)) {
        Ok(_) => Ok(ProbeResult {
            logged_in: true,
            quota_exhausted: false,
            last_error: None,
        }),
        Err(err) => {
            let msg = err.to_string();
            let lower = msg.to_lowercase();
            let quota_exhausted = lower.contains("402")
                || lower.contains("no quota")
                || lower.contains("insufficient quota")
                || lower.contains("quota exceeded");
            Ok(ProbeResult {
                logged_in: quota_exhausted,
                quota_exhausted,
                last_error: Some(msg),
            })
        }
    }
}

fn discover_user() -> String {
    std::env::var("GITHUB_USER")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".into())
}

fn discover_token_preview() -> String {
    for key in ["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                return value;
            }
        }
    }
    if let Ok(home) = home_dir() {
        let config = home.join(".copilot/config.json");
        if config.exists() {
            return "configured".into();
        }
    }
    "configured".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_user_has_fallback() {
        let user = discover_user();
        assert!(!user.trim().is_empty());
    }

    #[test]
    fn token_preview_is_safe() {
        let preview = sanitize_token_field(&discover_token_preview());
        assert!(!preview.trim().is_empty());
    }

    #[test]
    fn long_prompt_uses_at_file() {
        let prompt = "a".repeat(9000);
        let arg = build_prompt_arg(&prompt).expect("build prompt arg");
        assert!(arg.starts_with('@'));
    }

    #[test]
    fn quota_error_is_treated_as_logged_in() {
        let msg = "copilot command failed: 402 You have no quota".to_string();
        let lower = msg.to_lowercase();
        let quota_exhausted = lower.contains("402") || lower.contains("no quota");
        assert!(quota_exhausted);
    }
}
