pub mod admission;
pub mod api;
pub mod cli;
pub mod config;
pub mod context;
pub mod copilot;
pub mod expand;
pub mod gitops;
pub mod jira;
pub mod models;
pub mod prompt;
pub mod review_layers;
pub mod review_parser;
pub mod review_render;
pub mod review_schema;
pub mod review_validate;
pub mod risk;
pub mod services;
pub mod session;

use anyhow::{bail, Result};
use clap::Parser;
use cli::{AuthCommand, Cli, Commands};
use config::load_config;
use prompt::print_template;
use services::review_service::{
    execute_analyze, execute_assemble, execute_deep_review, execute_prompt, execute_review, execute_run,
    execute_validate, render_analyze_execution, render_assemble_execution, render_deep_review_execution,
    render_prompt_execution, render_review_execution, render_validate_execution,
};
use session::SessionStore;

pub fn run() -> Result<i32> {
    let cli = Cli::parse();
    let cfg = load_config()?;
    let store = SessionStore::new_default()?;

    match cli.command {
        Commands::Prompt(mut args) => {
            config::apply_config_defaults(&mut args, &cfg);
            let execution = execute_prompt(&args)?;
            render_prompt_execution(args.format, &execution)?;
            return Ok(execution.exit_code);
        }
        Commands::Assemble(mut args) => {
            config::apply_config_defaults(&mut args, &cfg);
            let execution = execute_assemble(&args)?;
            render_assemble_execution(&execution)?;
        }
        Commands::Run(mut args) => {
            config::apply_config_defaults(&mut args.prompt, &cfg);
            if let Some(include_context) = cfg.review.include_context {
                if !args.include_context {
                    args.include_context = include_context;
                }
            }
            if args.context_budget_bytes == 48_000 {
                if let Some(v) = cfg.review.context_budget_bytes { args.context_budget_bytes = v; }
            }
            if args.context_file_max_bytes == 12_000 {
                if let Some(v) = cfg.review.context_file_max_bytes { args.context_file_max_bytes = v; }
            }
            let execution = execute_run(&args)?;
            render_prompt_execution(args.prompt.format, &execution)?;
            return Ok(execution.exit_code);
        }
        Commands::Analyze(mut args) => {
            config::apply_config_defaults(&mut args.prompt, &cfg);
            if args.model.is_none() {
                args.model = cfg.llm.model.clone();
            }
            if let Some(include_context) = cfg.review.include_context {
                if !args.include_context {
                    args.include_context = include_context;
                }
            }
            if args.context_budget_bytes == 48_000 {
                if let Some(v) = cfg.review.context_budget_bytes { args.context_budget_bytes = v; }
            }
            if args.context_file_max_bytes == 12_000 {
                if let Some(v) = cfg.review.context_file_max_bytes { args.context_file_max_bytes = v; }
            }
            let execution = execute_analyze(&store, cfg.llm.model.clone(), &args)?;
            render_analyze_execution(args.prompt.format, &execution)?;
            return Ok(execution.exit_code);
        }
        Commands::DeepReview(mut args) => {
            config::apply_config_defaults(&mut args.prompt, &cfg);
            if args.model.is_none() {
                args.model = cfg.llm.model.clone();
            }
            if let Some(include_context) = cfg.review.include_context {
                if !args.include_context {
                    args.include_context = include_context;
                }
            }
            if args.context_budget_bytes == 48_000 {
                if let Some(v) = cfg.review.context_budget_bytes { args.context_budget_bytes = v; }
            }
            if args.context_file_max_bytes == 12_000 {
                if let Some(v) = cfg.review.context_file_max_bytes { args.context_file_max_bytes = v; }
            }
            let execution = execute_deep_review(&store, &args)?;
            render_deep_review_execution(args.prompt.format, &execution)?;
            return Ok(execution.exit_code);
        }
        Commands::Serve(args) => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(api::serve(&args.bind))?;
            return Ok(0);
        }
        Commands::Models { format } => {
            let models = models::list_models(&cfg)?;
            models.print(format)?;
        }
        Commands::Template { format } => print_template(format)?,
        Commands::Validate(mut args) => {
            config::apply_config_defaults(&mut args, &cfg);
            let execution = execute_validate(&args)?;
            render_validate_execution(args.format, &execution)?;
            return Ok(execution.exit_code);
        }
        Commands::Auth { command } => match command {
            AuthCommand::Init { format } => {
                let status = copilot::status(&store)?;
                if !status.logged_in {
                    let args = cli::LoginArgs { host: "https://github.com".into(), no_open: false };
                    let _ = copilot::login(&args, &store)?;
                }
                let models = models::list_models(&cfg)?;
                println!("Config path: {}", config::default_config_path()?.display());
                models.print(cli::OutputFormat::Text)?;
                println!("Run `code-review auth select-model --index <n>` to save your default model.");
                let status = copilot::status(&store)?;
                status.print(format)?;
            }
            AuthCommand::Login(args) => {
                let record = copilot::login(&args, &store)?;
                println!("Logged in via {}.", record.provider_source);
                println!("Session saved at {}", store.path().display());
            }
            AuthCommand::Models { format } => {
                let models = models::list_models(&cfg)?;
                models.print(format)?;
            }
            AuthCommand::SelectModel { model, index, format } => {
                let mut cfg = cfg.clone();
                let models = models::list_models(&cfg)?;
                let chosen = if let Some(model) = model {
                    model
                } else if let Some(index) = index {
                    if index == 0 || index > models.models.len() {
                        bail!("model index out of range");
                    }
                    models.models[index - 1].clone()
                } else {
                    bail!("provide --model or --index");
                };
                cfg.llm.model = Some(chosen.clone());
                if cfg.llm.provider.is_none() {
                    cfg.llm.provider = Some("copilot".into());
                }
                let path = config::save_config(&cfg)?;
                match format {
                    cli::OutputFormat::Text => {
                        println!("selected_model: {}", chosen);
                        println!("config: {}", path.display());
                    }
                    cli::OutputFormat::Json => {
                        println!("{}", serde_json::json!({"selected_model": chosen, "config": path.display().to_string() }));
                    }
                }
            }
            AuthCommand::Status { format } => {
                let status = copilot::status(&store)?;
                status.print(format)?;
                if let Some(model) = &cfg.llm.model {
                    println!("default_model: {}", model);
                }
                println!("config: {}", config::default_config_path()?.display());
            }
            AuthCommand::Logout { clear_remote } => {
                copilot::logout(&store, clear_remote)?;
                println!("Logged out. Local session removed.");
            }
            AuthCommand::Refresh { format } => {
                let status = copilot::refresh(&store)?;
                status.print(format)?;
            }
            AuthCommand::Whoami { format } => {
                let info = copilot::whoami(&store)?;
                info.print(format)?;
            }
        },
        Commands::Review(mut args) => {
            let execution = execute_review(&store, cfg.llm.model.clone(), &mut args)?;
            render_review_execution(args.prompt_args.format, &execution)?;
            return Ok(execution.exit_code);
        }
    }

    Ok(0)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_stage2_files_and_hints() {
        let stage1 = r#"
高风险问题
- src/order/service.rs: 幂等处理可能失效
- 不确定：OrderService.createPayment 在重试时是否有事务保护
- src/order/dto.rs: 契约可能变化
"#;
        let (files, hints) = crate::services::review_service::extract_stage2_focus(stage1);
        assert!(files.iter().any(|f| f == "src/order/service.rs"));
        assert!(files.iter().any(|f| f == "src/order/dto.rs"));
        assert!(hints.iter().any(|h| h.contains("不确定")));
    }

    #[test]
    fn repair_prompt_contains_required_sections() {
        let prompt = crate::services::review_service::build_repair_prompt("raw output", cli::ReviewMode::Critical);
        assert!(prompt.contains("高风险问题"));
        assert!(prompt.contains("发布建议 / 人工确认项"));
    }

    #[test]
    fn auto_expand_context_adds_related_files() {
        let mut args = cli::PromptArgs {
            mode: cli::ReviewMode::Standard,
            stack: None,
            goal: None,
            why: None,
            rules: vec![],
            risks: vec![],
            expected_normal: None,
            expected_error: None,
            expected_edge: None,
            issue: None,
            test_results: vec![],
            jira: None,
            jira_base_url: None,
            jira_provider: "native".into(),
            jira_command: None,
            diff_file: None,
            context_files: vec![],
            files: vec!["src/order/service.rs".into()],
            focus: vec![],
            baseline_files: vec![],
            incident_files: vec![],
            change_type: None,
            format: cli::OutputFormat::Text,
        };
        let repo_files = vec![
            "src/order/service.rs".into(),
            "src/order/service_test.rs".into(),
            "src/order/dto.rs".into(),
        ];
        let changed_files = vec!["src/order/service.rs".into()];
        crate::services::review_service::auto_expand_context_paths(&mut args, &repo_files, &changed_files);
        let collected = args.context_files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>();
        assert!(collected.iter().any(|f| f.ends_with("service_test.rs")));
    }
}
