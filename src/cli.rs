use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "code-review")]
#[command(version = "0.3.0")]
#[command(about = "Build structured AI code review prompts and run review flows")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Prompt(PromptArgs),
    Assemble(PromptArgs),
    Run(RunArgs),
    DeepReview(DeepReviewArgs),
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    Template {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    Validate(PromptArgs),
    Review(ReviewArgs),
}

#[derive(Subcommand, Debug)]
pub enum AuthCommand {
    Login(LoginArgs),
    Status {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    Logout {
        #[arg(long, default_value_t = false)]
        clear_remote: bool,
    },
    Refresh {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    Whoami {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
}

#[derive(Args, Debug, Clone)]
pub struct LoginArgs {
    #[arg(long, default_value = "https://github.com")]
    pub host: String,
    #[arg(long, default_value_t = false)]
    pub no_open: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewMode {
    Lite,
    Standard,
    Critical,
}

#[derive(Args, Debug, Clone, serde::Serialize)]
pub struct PromptArgs {
    #[arg(long, value_enum, default_value_t = ReviewMode::Standard)]
    pub mode: ReviewMode,
    #[arg(long)]
    pub stack: Option<String>,
    #[arg(long)]
    pub goal: Option<String>,
    #[arg(long)]
    pub why: Option<String>,
    #[arg(long = "rule")]
    pub rules: Vec<String>,
    #[arg(long = "risk")]
    pub risks: Vec<String>,
    #[arg(long)]
    pub expected_normal: Option<String>,
    #[arg(long)]
    pub expected_error: Option<String>,
    #[arg(long)]
    pub expected_edge: Option<String>,
    #[arg(long)]
    pub issue: Option<String>,
    #[arg(long = "test-result")]
    pub test_results: Vec<String>,
    #[arg(long)]
    pub jira: Option<String>,
    #[arg(long = "jira-base-url")]
    pub jira_base_url: Option<String>,
    #[arg(long = "jira-provider", default_value = "native")]
    pub jira_provider: String,
    #[arg(long = "jira-command")]
    pub jira_command: Option<String>,
    #[arg(long)]
    pub diff_file: Option<PathBuf>,
    #[arg(long = "context-file")]
    pub context_files: Vec<PathBuf>,
    #[arg(long = "file")]
    pub files: Vec<String>,
    #[arg(long = "focus")]
    pub focus: Vec<String>,
    #[arg(long = "baseline-file")]
    pub baseline_files: Vec<PathBuf>,
    #[arg(long = "type", help = "Change type: server, db, frontend, infra")]
    pub change_type: Option<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

#[derive(Args, Debug, Clone, serde::Serialize)]
pub struct RunArgs {
    #[arg(long)]
    pub git: String,
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    #[command(flatten)]
    pub prompt: PromptArgs,
    #[arg(long, default_value_t = false)]
    pub include_context: bool,
    #[arg(long, default_value_t = 48_000)]
    pub context_budget_bytes: usize,
    #[arg(long, default_value_t = 12_000)]
    pub context_file_max_bytes: usize,
}

impl RunArgs {
    pub fn to_prompt_args(&self, files: Vec<String>) -> PromptArgs {
        PromptArgs {
            mode: self.prompt.mode,
            stack: self.prompt.stack.clone(),
            goal: self.prompt.goal.clone(),
            why: self.prompt.why.clone(),
            rules: self.prompt.rules.clone(),
            risks: self.prompt.risks.clone(),
            expected_normal: self.prompt.expected_normal.clone(),
            expected_error: self.prompt.expected_error.clone(),
            expected_edge: self.prompt.expected_edge.clone(),
            issue: self.prompt.issue.clone(),
            test_results: self.prompt.test_results.clone(),
            jira: self.prompt.jira.clone(),
            jira_base_url: self.prompt.jira_base_url.clone(),
            jira_provider: self.prompt.jira_provider.clone(),
            jira_command: self.prompt.jira_command.clone(),
            diff_file: self.prompt.diff_file.clone(),
            context_files: self.prompt.context_files.clone(),
            files,
            focus: self.prompt.focus.clone(),
            baseline_files: self.prompt.baseline_files.clone(),
            change_type: self.prompt.change_type.clone(),
            format: self.prompt.format,
        }
    }
}

#[derive(Args, Debug, Clone, serde::Serialize)]
pub struct DeepReviewArgs {
    #[arg(long)]
    pub git: String,
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    #[arg(long)]
    pub model: Option<String>,
    #[command(flatten)]
    pub prompt: PromptArgs,
    #[arg(long, default_value_t = true)]
    pub include_context: bool,
    #[arg(long, default_value_t = 48_000)]
    pub context_budget_bytes: usize,
    #[arg(long, default_value_t = 12_000)]
    pub context_file_max_bytes: usize,
}

impl DeepReviewArgs {
    pub fn to_prompt_args(&self, files: Vec<String>) -> PromptArgs {
        PromptArgs {
            mode: self.prompt.mode,
            stack: self.prompt.stack.clone(),
            goal: self.prompt.goal.clone(),
            why: self.prompt.why.clone(),
            rules: self.prompt.rules.clone(),
            risks: self.prompt.risks.clone(),
            expected_normal: self.prompt.expected_normal.clone(),
            expected_error: self.prompt.expected_error.clone(),
            expected_edge: self.prompt.expected_edge.clone(),
            issue: self.prompt.issue.clone(),
            test_results: self.prompt.test_results.clone(),
            jira: self.prompt.jira.clone(),
            jira_base_url: self.prompt.jira_base_url.clone(),
            jira_provider: self.prompt.jira_provider.clone(),
            jira_command: self.prompt.jira_command.clone(),
            diff_file: self.prompt.diff_file.clone(),
            context_files: self.prompt.context_files.clone(),
            files,
            focus: self.prompt.focus.clone(),
            baseline_files: self.prompt.baseline_files.clone(),
            change_type: self.prompt.change_type.clone(),
            format: self.prompt.format,
        }
    }
}

#[derive(Args, Debug, Clone)]
pub struct ReviewArgs {
    #[arg(long)]
    pub prompt: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[command(flatten)]
    pub prompt_args: PromptArgs,
}

impl ReviewArgs {
    pub fn to_prompt_args(&self) -> Option<PromptArgs> {
        let has_fields = self.prompt_args.stack.is_some()
            || self.prompt_args.goal.is_some()
            || self.prompt_args.why.is_some()
            || !self.prompt_args.rules.is_empty()
            || !self.prompt_args.risks.is_empty()
            || self.prompt_args.expected_normal.is_some()
            || self.prompt_args.expected_error.is_some()
            || self.prompt_args.expected_edge.is_some()
            || self.prompt_args.issue.is_some()
            || !self.prompt_args.test_results.is_empty()
            || self.prompt_args.jira.is_some()
            || self.prompt_args.diff_file.is_some()
            || !self.prompt_args.context_files.is_empty()
            || !self.prompt_args.files.is_empty()
            || !self.prompt_args.focus.is_empty()
            || !self.prompt_args.baseline_files.is_empty();
        if has_fields {
            Some(self.prompt_args.clone())
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Text,
    Json,
}
