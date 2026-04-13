use crate::cli::{OutputFormat, PromptArgs, ReviewMode};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub jira: JiraConfig,
    #[serde(default)]
    pub review: ReviewConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JiraConfig {
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewConfig {
    pub mode: Option<ReviewMode>,
    pub include_context: Option<bool>,
    pub context_budget_bytes: Option<usize>,
    pub context_file_max_bytes: Option<usize>,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            mode: Some(ReviewMode::Standard),
            include_context: Some(false),
            context_budget_bytes: Some(48_000),
            context_file_max_bytes: Some(12_000),
        }
    }
}

pub fn load_config() -> Result<AppConfig> {
    let path = default_config_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read config {}", path.display()))?;
    let cfg = toml::from_str::<AppConfig>(&text)
        .with_context(|| format!("failed to parse config {}", path.display()))?;
    Ok(cfg)
}

pub fn default_config_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".config/code-review/config.toml"))
}

pub fn save_config(cfg: &AppConfig) -> Result<PathBuf> {
    let path = default_config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config dir {}", parent.display()))?;
    }
    let text = toml::to_string_pretty(cfg).context("failed to serialize config")?;
    fs::write(&path, text).with_context(|| format!("failed to write config {}", path.display()))?;
    Ok(path)
}

pub fn apply_config_defaults(args: &mut PromptArgs, cfg: &AppConfig) {
    if args.stack.is_none() {
        args.stack = None;
    }
    if args.jira_base_url.is_none() {
        args.jira_base_url = cfg.jira.base_url.clone();
    }
    if args.jira_provider == "native" {
        if let Some(provider) = &cfg.jira.provider {
            args.jira_provider = provider.clone();
        }
    }
    if args.jira_command.is_none() {
        args.jira_command = cfg.jira.command.clone();
    }
    if matches!(args.mode, ReviewMode::Standard) {
        if let Some(mode) = cfg.review.mode {
            args.mode = mode;
        }
    }
    if matches!(args.format, OutputFormat::Text) {
        args.format = OutputFormat::Text;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_config_toml() {
        let text = r#"
[llm]
provider = "copilot"
model = "gpt-5.4"
models = ["gpt-5.4", "opus"]

[jira]
provider = "native"
base_url = "https://jira.example.com"

[review]
mode = "critical"
include_context = true
context_budget_bytes = 32000
"#;
        let cfg: AppConfig = toml::from_str(text).unwrap();
        assert_eq!(cfg.llm.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(cfg.jira.base_url.as_deref(), Some("https://jira.example.com"));
        assert!(matches!(cfg.review.mode, Some(ReviewMode::Critical)));
    }
}
