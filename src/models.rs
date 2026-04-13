use crate::cli::OutputFormat;
use crate::config::AppConfig;
use anyhow::{Context, Result};
use regex::Regex;
use reqwest::blocking::Client;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ModelList {
    pub provider: String,
    pub models: Vec<String>,
    pub default_model: Option<String>,
    pub source: String,
}

impl ModelList {
    pub fn print(&self, format: OutputFormat) -> Result<()> {
        match format {
            OutputFormat::Text => {
                println!("provider: {}", self.provider);
                println!("source: {}", self.source);
                if let Some(default_model) = &self.default_model {
                    println!("default_model: {}", default_model);
                }
                println!("models:");
                for (idx, model) in self.models.iter().enumerate() {
                    println!("{}. {}", idx + 1, model);
                }
            }
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(self)?),
        }
        Ok(())
    }
}

pub fn list_models(cfg: &AppConfig) -> Result<ModelList> {
    let provider = cfg.llm.provider.clone().unwrap_or_else(|| "copilot".into());
    let models = fetch_copilot_models().unwrap_or_else(|_| fallback_models());
    Ok(ModelList {
        provider,
        models,
        default_model: cfg.llm.model.clone(),
        source: "github-docs".into(),
    })
}

fn fetch_copilot_models() -> Result<Vec<String>> {
    let client = Client::builder().build()?;
    let html = client
        .get("https://docs.github.com/en/copilot/reference/ai-models/supported-models")
        .send()
        .context("failed to request GitHub supported models page")?
        .text()
        .context("failed to read supported models page")?;

    let re = Regex::new(r#"(?i)>(gpt-[0-9][^<]{0,20}|claude[^<]{0,30}|opus[^<]{0,20}|sonnet[^<]{0,20}|gemini[^<]{0,20}|o[0-9][^<]{0,20})<"#).unwrap();
    let mut out = Vec::new();
    for cap in re.captures_iter(&html) {
        let model = cap[1]
            .replace("&nbsp;", " ")
            .replace("&#39;", "'")
            .trim()
            .to_string();
        if !out.iter().any(|m: &String| m.eq_ignore_ascii_case(&model)) {
            out.push(model);
        }
    }
    if out.is_empty() {
        anyhow::bail!("no models parsed from docs page");
    }
    Ok(out)
}

fn fallback_models() -> Vec<String> {
    vec!["gpt-5".into(), "gpt-5.4".into(), "opus".into()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_has_models() {
        assert!(!fallback_models().is_empty());
    }
}
