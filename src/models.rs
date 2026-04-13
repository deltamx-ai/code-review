use crate::cli::OutputFormat;
use crate::config::AppConfig;
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ModelList {
    pub provider: String,
    pub models: Vec<String>,
    pub default_model: Option<String>,
}

impl ModelList {
    pub fn print(&self, format: OutputFormat) -> Result<()> {
        match format {
            OutputFormat::Text => {
                println!("provider: {}", self.provider);
                if let Some(default_model) = &self.default_model {
                    println!("default_model: {}", default_model);
                }
                println!("models:");
                for model in &self.models {
                    println!("- {}", model);
                }
            }
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(self)?),
        }
        Ok(())
    }
}

pub fn list_models(cfg: &AppConfig) -> ModelList {
    let provider = cfg.llm.provider.clone().unwrap_or_else(|| "copilot".into());
    let mut models = if cfg.llm.models.is_empty() {
        vec!["gpt-5".into(), "gpt-5.4".into(), "opus".into()]
    } else {
        cfg.llm.models.clone()
    };
    if let Some(default_model) = &cfg.llm.model {
        if !models.contains(default_model) {
            models.insert(0, default_model.clone());
        }
    }
    ModelList {
        provider,
        models,
        default_model: cfg.llm.model.clone(),
    }
}
