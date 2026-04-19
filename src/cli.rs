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
    Analyze(AnalyzeArgs),
    DeepReview(DeepReviewArgs),
    ReviewSession {
        #[command(subcommand)]
        command: ReviewSessionCommand,
    },
    Serve(ServeArgs),
    Models {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
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
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum ReviewSessionCommand {
    Start(ReviewSessionStartArgs),
    Continue(ReviewSessionContinueArgs),
    Show(ReviewSessionShowArgs),
}

#[derive(Args, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReviewSessionStartArgs {
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long)]
    pub provider: Option<String>,
    #[arg(long)]
    pub base_ref: Option<String>,
    #[arg(long)]
    pub head_ref: Option<String>,
    #[arg(long)]
    pub diff_text: Option<String>,
    #[arg(long)]
    pub initial_instruction: Option<String>,
    #[command(flatten)]
    pub prompt: PromptArgs,
}

#[derive(Args, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReviewSessionContinueArgs {
    #[arg(long)]
    pub session_id: String,
    #[arg(long)]
    pub instruction: Option<String>,
    #[arg(long = "attached-file")]
    pub attached_files: Vec<String>,
    #[arg(long = "extra-context")]
    pub extra_context: Vec<String>,
    #[arg(long = "focus-finding")]
    pub focus_finding_ids: Vec<String>,
    #[arg(long, default_value_t = false)]
    pub finalize: bool,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

#[derive(Args, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReviewSessionShowArgs {
    #[arg(long)]
    pub session_id: String,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

#[derive(Args, Debug, Clone, serde::Deserialize)]
pub struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1:3000")]
    pub bind: String,
}

#[derive(Subcommand, Debug)]
pub enum AuthCommand {
    Init {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    Login(LoginArgs),
    Models {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    SelectModel {
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        index: Option<usize>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
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

#[derive(Subcommand, Debug)]
pub enum SessionCommand {
    Start(SessionStartArgs),
    Continue(SessionContinueArgs),
    Show(SessionShowArgs),
}

#[derive(Args, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionStartArgs {
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long)]
    pub diff_text: Option<String>,
    #[command(flatten)]
    pub prompt: PromptArgs,
    #[arg(long)]
    pub instruction: Option<String>,
}

#[derive(Args, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionContinueArgs {
    #[arg(long)]
    pub session_id: String,
    #[arg(long)]
    pub instruction: Option<String>,
    #[arg(long = "attach")]
    pub attached_files: Vec<String>,
    #[arg(long = "context")]
    pub extra_context: Vec<String>,
    #[arg(long = "focus-finding")]
    pub focus_finding_ids: Vec<String>,
    #[arg(long, default_value_t = false)]
    pub finalize: bool,
}

#[derive(Args, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionShowArgs {
    #[arg(long)]
    pub session_id: String,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

#[derive(Args, Debug, Clone)]
pub struct LoginArgs {
    #[arg(long, default_value = "https://github.com")]
    pub host: String,
    #[arg(long, default_value_t = false)]
    pub no_open: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewMode {
    Lite,
    Standard,
    Critical,
}

#[derive(Args, Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    #[arg(long = "incident-file")]
    pub incident_files: Vec<PathBuf>,
    #[arg(long = "type", help = "Change type: server, db, frontend, infra, contract, api")]
    pub change_type: Option<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

#[derive(Args, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunArgs {
    #[arg(long)]
    pub git: String,
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    #[command(flatten)]
    pub prompt: PromptArgs,
    #[arg(long, default_value_t = false)]
    pub include_context: bool,
    #[arg(long)]
    pub context_budget_bytes: Option<usize>,
    #[arg(long)]
    pub context_file_max_bytes: Option<usize>,
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
            incident_files: self.prompt.incident_files.clone(),
            change_type: self.prompt.change_type.clone(),
            format: self.prompt.format,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnalyzeStrategy {
    Standard,
    Deep,
}

#[derive(Args, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnalyzeArgs {
    #[arg(long)]
    pub git: String,
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long, value_enum, default_value_t = AnalyzeStrategy::Deep)]
    pub strategy: AnalyzeStrategy,
    #[command(flatten)]
    pub prompt: PromptArgs,
    #[arg(long, default_value_t = true)]
    pub include_context: bool,
    #[arg(long)]
    pub context_budget_bytes: Option<usize>,
    #[arg(long)]
    pub context_file_max_bytes: Option<usize>,
}

impl AnalyzeArgs {
    pub fn to_run_args(&self) -> RunArgs {
        RunArgs {
            git: self.git.clone(),
            repo: self.repo.clone(),
            prompt: self.prompt.clone(),
            include_context: self.include_context,
            context_budget_bytes: self.context_budget_bytes,
            context_file_max_bytes: self.context_file_max_bytes,
        }
    }

    pub fn to_deep_review_args(&self) -> DeepReviewArgs {
        DeepReviewArgs {
            git: self.git.clone(),
            repo: self.repo.clone(),
            model: self.model.clone(),
            prompt: self.prompt.clone(),
            include_context: self.include_context,
            context_budget_bytes: self.context_budget_bytes,
            context_file_max_bytes: self.context_file_max_bytes,
        }
    }
}

#[derive(Args, Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    #[arg(long)]
    pub context_budget_bytes: Option<usize>,
    #[arg(long)]
    pub context_file_max_bytes: Option<usize>,
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
            incident_files: self.prompt.incident_files.clone(),
            change_type: self.prompt.change_type.clone(),
            format: self.prompt.format,
        }
    }
}

#[derive(Args, Debug, Clone, serde::Deserialize)]
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
            || !self.prompt_args.baseline_files.is_empty()
            || !self.prompt_args.incident_files.is_empty();
        if has_fields {
            Some(self.prompt_args.clone())
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Text,
    Json,
}
