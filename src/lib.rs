pub mod cli;
pub mod context;
pub mod copilot;
pub mod gitops;
pub mod prompt;
pub mod session;

use anyhow::{bail, Result};
use clap::Parser;
use cli::{AuthCommand, Cli, Commands};
use prompt::{
    build_prompt, build_prompt_from_sources, print_template, validate_args, PromptOutput,
    PromptSummary,
};
use session::SessionStore;

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let store = SessionStore::new_default()?;

    match cli.command {
        Commands::Prompt(args) => {
            let validation = validate_args(
                &args,
                args.diff_file.is_some(),
                !args.context_files.is_empty() || !args.files.is_empty(),
            );
            let prompt = build_prompt(&args)?;
            output_prompt(
                args.format,
                validation.score,
                validation.ok,
                prompt,
                PromptSummary::from_prompt_args(&args),
            )?;
        }
        Commands::Run(args) => {
            gitops::ensure_git_repo(&args.repo)?;
            let diff = gitops::git_diff(&args.repo, &args.git)?;
            if diff.trim().is_empty() {
                bail!("git diff is empty for range {}", args.git);
            }
            let files = gitops::git_changed_files(&args.repo, &args.git)?;
            let contexts = if args.include_context {
                context::read_repo_context_with_budget(
                    &args.repo,
                    &files,
                    args.context_budget_bytes,
                    args.context_file_max_bytes,
                )?
            } else {
                context::ContextCollection::default()
            };
            let prompt_args = args.to_prompt_args(files.clone());
            let validation = validate_args(
                &prompt_args,
                true,
                !contexts.files.is_empty() || !files.is_empty(),
            );
            let prompt = build_prompt_from_sources(&prompt_args, Some(diff), contexts)?;
            output_prompt(
                prompt_args.format,
                validation.score,
                validation.ok,
                prompt,
                PromptSummary::from_prompt_args(&prompt_args),
            )?;
        }
        Commands::Template { format } => print_template(format)?,
        Commands::Validate(args) => {
            let validation = validate_args(
                &args,
                args.diff_file.is_some(),
                !args.context_files.is_empty() || !args.files.is_empty(),
            );
            validation.print(args.format)?;
        }
        Commands::Auth { command } => match command {
            AuthCommand::Login(args) => {
                let record = copilot::login(&args, &store)?;
                println!("Logged in via {}.", record.provider_source);
                println!("Session saved at {}", store.path().display());
            }
            AuthCommand::Status { format } => {
                let status = copilot::status(&store)?;
                status.print(format)?;
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
        Commands::Review(args) => {
            let status = copilot::status(&store)?;
            if !status.logged_in {
                bail!("Copilot is not authenticated. Run `code-review auth login` first.");
            }
            let prompt = if let Some(prompt) = args.prompt.clone() {
                prompt
            } else if let Some(prompt_args) = args.to_prompt_args() {
                let validation = validate_args(
                    &prompt_args,
                    prompt_args.diff_file.is_some(),
                    !prompt_args.context_files.is_empty() || !prompt_args.files.is_empty(),
                );
                if validation.score < 40 {
                    bail!("review input is too thin; add more context or use --prompt directly");
                }
                build_prompt(&prompt_args)?
            } else {
                bail!("provide --prompt or enough prompt-building flags");
            };
            let response = copilot::run_review(&store, &prompt, args.model.as_deref())?;
            println!("{}", response);
        }
    }

    Ok(())
}

fn output_prompt(
    format: cli::OutputFormat,
    score: u8,
    ok: bool,
    prompt: String,
    summary: PromptSummary,
) -> Result<()> {
    match format {
        cli::OutputFormat::Text => {
            println!("# Review Readiness Score: {}/100", score);
            if !ok {
                println!("# Warning: context is incomplete; AI review quality may be limited.\n");
            }
            println!("{}", prompt);
        }
        cli::OutputFormat::Json => {
            let output = PromptOutput {
                ok,
                score,
                prompt,
                summary,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }
    Ok(())
}
