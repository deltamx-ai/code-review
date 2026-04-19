pub mod admission;
pub mod api;
pub mod cli;
pub mod config;
pub mod context;
pub mod copilot;
pub mod expand;
pub mod gitops;
pub mod jira;
pub mod conversation;
pub mod conversation_store;
pub mod models;
pub mod orchestrator;
pub mod prompt;
pub mod review_layers;
pub mod review_parser;
pub mod review_render;
pub mod review_schema;
pub mod review_validate;
pub mod risk;
pub mod services;
pub mod session;
pub mod providers;

use anyhow::{bail, Result};
use clap::Parser;
use cli::{AuthCommand, Cli, Commands, ReviewSessionCommand};
use config::load_config;
use conversation::{FindingPatch, FindingStatus, SessionListFilter};
use conversation_store::ConversationStore;
use orchestrator::{
    continue_session, start_session, ContinueReviewTurnRequest, StartReviewSessionRequest,
};
use prompt::print_template;
use providers::copilot::CopilotCliProvider;
use services::review_service::{
    execute_analyze, execute_assemble, execute_deep_review, execute_prompt, execute_review, execute_run,
    execute_validate, render_analyze_execution, render_assemble_execution, render_deep_review_execution,
    render_prompt_execution, render_review_execution, render_validate_execution,
};
use session::SessionStore;

fn apply_context_config(include_context: &mut bool, cfg: &config::AppConfig) {
    if let Some(v) = cfg.review.include_context {
        if !*include_context {
            *include_context = v;
        }
    }
}

fn resolve_run_context(args: &mut cli::RunArgs, cfg: &config::AppConfig) {
    apply_context_config(&mut args.include_context, cfg);
    args.context_budget_bytes = Some(config::resolve_context_budget_bytes(args.context_budget_bytes, cfg));
    args.context_file_max_bytes = Some(config::resolve_context_file_max_bytes(args.context_file_max_bytes, cfg));
}

fn resolve_analyze_context(args: &mut cli::AnalyzeArgs, cfg: &config::AppConfig) {
    apply_context_config(&mut args.include_context, cfg);
    args.context_budget_bytes = Some(config::resolve_context_budget_bytes(args.context_budget_bytes, cfg));
    args.context_file_max_bytes = Some(config::resolve_context_file_max_bytes(args.context_file_max_bytes, cfg));
}

fn resolve_deep_review_context(args: &mut cli::DeepReviewArgs, cfg: &config::AppConfig) {
    apply_context_config(&mut args.include_context, cfg);
    args.context_budget_bytes = Some(config::resolve_context_budget_bytes(args.context_budget_bytes, cfg));
    args.context_file_max_bytes = Some(config::resolve_context_file_max_bytes(args.context_file_max_bytes, cfg));
}

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
            resolve_run_context(&mut args, &cfg);
            let execution = execute_run(&args)?;
            render_prompt_execution(args.prompt.format, &execution)?;
            return Ok(execution.exit_code);
        }
        Commands::Analyze(mut args) => {
            config::apply_config_defaults(&mut args.prompt, &cfg);
            if args.model.is_none() {
                args.model = cfg.llm.model.clone();
            }
            resolve_analyze_context(&mut args, &cfg);
            let execution = execute_analyze(&store, cfg.llm.model.clone(), &args)?;
            render_analyze_execution(args.prompt.format, &execution)?;
            return Ok(execution.exit_code);
        }
        Commands::DeepReview(mut args) => {
            config::apply_config_defaults(&mut args.prompt, &cfg);
            if args.model.is_none() {
                args.model = cfg.llm.model.clone();
            }
            resolve_deep_review_context(&mut args, &cfg);
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
        Commands::ReviewSession { command } => {
            return dispatch_review_session(&cfg, &store, command);
        }
        Commands::Session { .. } => {
            bail!("the legacy `session` CLI command has been replaced by `review-session`");
        }
    }

    Ok(0)
}

fn dispatch_review_session(
    cfg: &config::AppConfig,
    store: &SessionStore,
    command: ReviewSessionCommand,
) -> Result<i32> {
    let convo_store = ConversationStore::new_default()?;
    match command {
        ReviewSessionCommand::Start(args) => {
            let mut prompt = args.prompt.clone();
            config::apply_config_defaults(&mut prompt, cfg);
            let provider = CopilotCliProvider::new(store.clone());
            let result = start_session(
                &convo_store,
                &provider,
                StartReviewSessionRequest {
                    repo_root: args.repo.clone(),
                    review_mode: prompt.mode,
                    provider: args.provider.clone(),
                    model: args.model.clone().or_else(|| cfg.llm.model.clone()),
                    base_ref: args.base_ref.clone(),
                    head_ref: args.head_ref.clone(),
                    diff_text: args.diff_text.clone(),
                    prompt_args: prompt,
                    initial_instruction: args.initial_instruction.clone(),
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "session": result.session,
                "turn": result.turn,
                "new_findings": result.new_findings,
            }))?);
            Ok(0)
        }
        ReviewSessionCommand::Continue(args) => {
            let provider = CopilotCliProvider::new(store.clone());
            let result = continue_session(
                &convo_store,
                &provider,
                ContinueReviewTurnRequest {
                    session_id: args.session_id.clone(),
                    instruction: args.instruction.clone(),
                    attached_files: args.attached_files.clone(),
                    extra_context: args.extra_context.clone(),
                    focus_finding_ids: args.focus_finding_ids.clone(),
                    generate_final_report: args.finalize,
                    model: args.model.clone(),
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&result.session)?);
            Ok(0)
        }
        ReviewSessionCommand::Show(args) => {
            let session = convo_store.load_session(&args.session_id)?;
            let turns = convo_store.load_turns(&args.session_id)?;
            let messages = convo_store.load_messages(&args.session_id)?;
            let findings = convo_store.load_findings(&args.session_id)?;
            let artifacts = convo_store.list_artifacts(&args.session_id)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "session": session,
                    "turns": turns,
                    "messages": messages,
                    "findings": findings,
                    "artifacts": artifacts,
                }))?
            );
            Ok(0)
        }
        ReviewSessionCommand::List(args) => {
            let filter = SessionListFilter {
                repo: args.repo.clone(),
                status: args.status.clone(),
                mode: args.mode.clone(),
                limit: Some(args.limit),
                offset: Some(args.offset),
            };
            let sessions = convo_store.list_sessions(&filter)?;
            let total = convo_store.count_sessions(&SessionListFilter {
                repo: args.repo.clone(),
                status: args.status.clone(),
                mode: args.mode.clone(),
                limit: None,
                offset: None,
            })?;
            match args.format {
                cli::OutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "items": sessions,
                            "total": total,
                            "limit": args.limit,
                            "offset": args.offset,
                        }))?
                    );
                }
                cli::OutputFormat::Text => {
                    println!("total: {}", total);
                    for s in sessions {
                        println!(
                            "{} [{:?}] mode={:?} turns={}/{} findings={} updated={}",
                            s.id,
                            s.status,
                            s.review_mode,
                            s.current_turn,
                            s.total_turns,
                            s.finding_counts.total,
                            s.updated_at,
                        );
                    }
                }
            }
            Ok(0)
        }
        ReviewSessionCommand::Delete(args) => {
            convo_store.delete_session(&args.session_id)?;
            println!("deleted: {}", args.session_id);
            Ok(0)
        }
        ReviewSessionCommand::Finding(args) => {
            let parsed_status = match args.status.as_deref() {
                None => None,
                Some("suspected") => Some(FindingStatus::Suspected),
                Some("confirmed") => Some(FindingStatus::Confirmed),
                Some("dismissed") => Some(FindingStatus::Dismissed),
                Some("fixed") => Some(FindingStatus::Fixed),
                Some("accepted_risk") | Some("accepted-risk") => Some(FindingStatus::AcceptedRisk),
                Some(other) => bail!("unknown status: {}", other),
            };
            let patch = FindingPatch {
                status: parsed_status,
                owner: args.owner.clone(),
                tags: if args.tags.is_empty() { None } else { Some(args.tags.clone()) },
            };
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let updated = convo_store.update_finding(
                &args.session_id,
                &args.finding_id,
                &patch,
                &secs.to_string(),
            )?;
            match args.format {
                cli::OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&updated)?),
                cli::OutputFormat::Text => {
                    println!("updated: {} status={:?}", updated.id, updated.status);
                }
            }
            Ok(0)
        }
    }
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
