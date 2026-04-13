use crate::cli::PromptArgs;
use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct JiraEnrichment {
    pub issue_key: Option<String>,
    pub source: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub acceptance: Vec<String>,
    pub comments: Vec<String>,
    pub labels: Vec<String>,
    pub components: Vec<String>,
    pub issue_type: Option<String>,
    pub priority: Option<String>,
    pub linked_titles: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct JiraIssueResponse {
    key: Option<String>,
    fields: JiraFields,
}

#[derive(Debug, Deserialize)]
struct JiraFields {
    summary: Option<String>,
    description: Option<String>,
    labels: Option<Vec<String>>,
    priority: Option<JiraNamedField>,
    issuetype: Option<JiraNamedField>,
    components: Option<Vec<JiraNamedField>>,
    comment: Option<JiraComments>,
}

#[derive(Debug, Deserialize)]
struct JiraNamedField {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JiraComments {
    comments: Vec<JiraComment>,
}

#[derive(Debug, Deserialize)]
struct JiraComment {
    body: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExternalIssue {
    key: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    acceptance: Option<Vec<String>>,
    comments: Option<Vec<String>>,
    labels: Option<Vec<String>>,
    components: Option<Vec<String>>,
    issue_type: Option<String>,
    priority: Option<String>,
    linked_titles: Option<Vec<String>>,
}

pub fn enrich_prompt_args(args: &mut PromptArgs, repo_files: &[String]) -> Result<()> {
    if args.jira.is_none() {
        return Ok(());
    }

    let jira = fetch_issue(args)?;
    apply_jira_defaults(args, &jira);
    infer_from_jira(args, &jira, repo_files);
    Ok(())
}

fn fetch_issue(args: &PromptArgs) -> Result<JiraEnrichment> {
    match args.jira_provider.as_str() {
        "native" => fetch_issue_native(args),
        "command" => fetch_issue_via_command(args),
        other => bail!("unsupported jira provider: {}", other),
    }
}

fn fetch_issue_native(args: &PromptArgs) -> Result<JiraEnrichment> {
    let issue_key = args
        .jira
        .as_deref()
        .ok_or_else(|| anyhow!("missing jira issue key"))?;
    let base = args
        .jira_base_url
        .clone()
        .or_else(|| std::env::var("JIRA_BASE_URL").ok())
        .ok_or_else(|| anyhow!("missing jira base url; pass --jira-base-url or set JIRA_BASE_URL"))?;
    let token = std::env::var("JIRA_TOKEN").ok();
    let user = std::env::var("JIRA_USER").ok();

    let client = Client::builder().build()?;
    let url = format!("{}/rest/api/2/issue/{}", base.trim_end_matches('/'), issue_key);
    let mut req = client.get(url);
    if let Some(token) = token {
        if let Some(user) = user {
            req = req.basic_auth(user, Some(token));
        } else {
            req = req.bearer_auth(token);
        }
    }

    let resp = req.send().context("failed to request jira issue")?;
    if !resp.status().is_success() {
        bail!("jira request failed with status {}", resp.status());
    }
    let issue: JiraIssueResponse = resp.json().context("failed to parse jira response")?;
    let description = issue.fields.description.map(clean_text_block);
    let acceptance = extract_acceptance(description.as_deref().unwrap_or(""));
    let comments = issue
        .fields
        .comment
        .map(|v| {
            v.comments
                .into_iter()
                .filter_map(|c| c.body)
                .map(clean_text_block)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(JiraEnrichment {
        issue_key: issue.key,
        source: "native".into(),
        summary: issue.fields.summary.map(clean_text_block),
        description,
        acceptance,
        comments,
        labels: issue.fields.labels.unwrap_or_default(),
        components: issue
            .fields
            .components
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.name)
            .collect(),
        issue_type: issue.fields.issuetype.and_then(|v| v.name),
        priority: issue.fields.priority.and_then(|v| v.name),
        linked_titles: vec![],
    })
}

fn fetch_issue_via_command(args: &PromptArgs) -> Result<JiraEnrichment> {
    let issue_key = args
        .jira
        .as_deref()
        .ok_or_else(|| anyhow!("missing jira issue key"))?;
    let template = args
        .jira_command
        .clone()
        .or_else(|| std::env::var("CODE_REVIEW_JIRA_COMMAND").ok())
        .ok_or_else(|| anyhow!("missing jira command; pass --jira-command or set CODE_REVIEW_JIRA_COMMAND"))?;
    let command = template.replace("{issue}", issue_key);
    let output = Command::new("sh")
        .arg("-lc")
        .arg(&command)
        .output()
        .with_context(|| format!("failed to run jira command: {}", command))?;
    if !output.status.success() {
        bail!("jira command failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let issue: ExternalIssue = serde_json::from_str(&text)
        .context("jira command must output JSON matching ExternalIssue shape")?;
    Ok(JiraEnrichment {
        issue_key: issue.key,
        source: "command".into(),
        summary: issue.summary.map(clean_text_block),
        description: issue.description.map(clean_text_block),
        acceptance: issue
            .acceptance
            .unwrap_or_default()
            .into_iter()
            .map(clean_text_block)
            .collect(),
        comments: issue
            .comments
            .unwrap_or_default()
            .into_iter()
            .map(clean_text_block)
            .collect(),
        labels: issue.labels.unwrap_or_default(),
        components: issue.components.unwrap_or_default(),
        issue_type: issue.issue_type,
        priority: issue.priority,
        linked_titles: issue.linked_titles.unwrap_or_default(),
    })
}

fn apply_jira_defaults(args: &mut PromptArgs, jira: &JiraEnrichment) {
    if args.goal.is_none() {
        args.goal = jira.summary.clone();
    }
    if args.issue.is_none() {
        args.issue = jira.description.clone().or_else(|| jira.summary.clone());
    }
    if args.why.is_none() {
        args.why = jira.summary.clone();
    }
    if args.rules.is_empty() {
        args.rules = jira.acceptance.iter().take(5).cloned().collect();
    }
    if args.test_results.is_empty() {
        args.test_results = extract_test_results(jira);
    }
}

fn infer_from_jira(args: &mut PromptArgs, jira: &JiraEnrichment, repo_files: &[String]) {
    if args.change_type.is_none() {
        args.change_type = infer_change_type(jira, repo_files);
    }
    if args.risks.is_empty() {
        args.risks = infer_risks(jira);
    }
    if args.expected_normal.is_none() || args.expected_error.is_none() || args.expected_edge.is_none() {
        let inferred = infer_expected_behavior(jira);
        if args.expected_normal.is_none() {
            args.expected_normal = inferred.0;
        }
        if args.expected_error.is_none() {
            args.expected_error = inferred.1;
        }
        if args.expected_edge.is_none() {
            args.expected_edge = inferred.2;
        }
    }
    if args.focus.is_empty() {
        args.focus = infer_focus(jira);
    }
}

fn infer_change_type(jira: &JiraEnrichment, repo_files: &[String]) -> Option<String> {
    let corpus = format!(
        "{} {} {} {} {}",
        jira.summary.clone().unwrap_or_default(),
        jira.description.clone().unwrap_or_default(),
        jira.labels.join(" "),
        jira.components.join(" "),
        jira.issue_type.clone().unwrap_or_default(),
    )
    .to_lowercase();

    let has_file = |suffixes: &[&str]| repo_files.iter().any(|f| suffixes.iter().any(|s| f.ends_with(s)));

    if corpus.contains("sql")
        || corpus.contains("migration")
        || corpus.contains("database")
        || corpus.contains("db")
        || has_file(&[".sql", "schema.prisma"])
    {
        return Some("db".into());
    }
    if corpus.contains("frontend")
        || corpus.contains("react")
        || corpus.contains("vue")
        || corpus.contains("ui")
        || has_file(&[".tsx", ".jsx", ".vue", ".css", ".scss"])
    {
        return Some("frontend".into());
    }
    if corpus.contains("ci")
        || corpus.contains("deploy")
        || corpus.contains("docker")
        || corpus.contains("workflow")
        || has_file(&["Dockerfile", ".yml", ".yaml", ".sh"])
    {
        return Some("infra".into());
    }
    Some("server".into())
}

fn infer_risks(jira: &JiraEnrichment) -> Vec<String> {
    let mut risks = Vec::new();
    let corpus = format!(
        "{} {} {} {} {} {}",
        jira.summary.clone().unwrap_or_default(),
        jira.description.clone().unwrap_or_default(),
        jira.comments.join(" "),
        jira.labels.join(" "),
        jira.components.join(" "),
        jira.priority.clone().unwrap_or_default(),
    )
    .to_lowercase();

    for (kw, risk) in [
        ("concurrency", "并发"),
        ("race", "并发"),
        ("transaction", "事务一致性"),
        ("permission", "权限"),
        ("auth", "权限"),
        ("security", "安全"),
        ("xss", "安全"),
        ("csrf", "安全"),
        ("performance", "性能"),
        ("slow", "性能"),
        ("compat", "兼容性"),
        ("rollback", "回滚风险"),
        ("migration", "数据库迁移风险"),
        ("api", "API 契约变化"),
        ("retry", "幂等"),
        ("duplicate", "幂等"),
    ] {
        if corpus.contains(kw) && !risks.iter().any(|v| v == risk) {
            risks.push(risk.to_string());
        }
    }

    if risks.is_empty() {
        risks.push("业务逻辑".into());
        risks.push("边界条件".into());
    }
    risks
}

fn infer_expected_behavior(jira: &JiraEnrichment) -> (Option<String>, Option<String>, Option<String>) {
    let normal = jira.acceptance.first().cloned().or_else(|| jira.summary.clone());
    let error = jira
        .comments
        .iter()
        .find(|c| c.contains("失败") || c.contains("error") || c.contains("异常"))
        .cloned()
        .or_else(|| Some("异常输入或非法状态应返回明确错误，且不得破坏已有数据。".into()));
    let edge = jira
        .acceptance
        .iter()
        .find(|c| c.contains("边界") || c.contains("重复") || c.contains("重试") || c.contains("并发"))
        .cloned()
        .or_else(|| Some("边界输入、重复提交和重试场景下行为应保持正确且幂等。".into()));
    (normal, error, edge)
}

fn infer_focus(jira: &JiraEnrichment) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(priority) = &jira.priority {
        if priority.to_lowercase().contains("high") || priority.contains('高') {
            out.push("优先检查高风险回归和线上影响面".into());
        }
    }
    if jira.issue_type.as_deref().unwrap_or_default().to_lowercase().contains("bug") {
        out.push("重点确认修复是否覆盖根因，而不是仅覆盖表象".into());
    }
    out
}

fn extract_test_results(jira: &JiraEnrichment) -> Vec<String> {
    let mut out = Vec::new();
    let patterns = ["test", "测试", "qa", "验证", "passed", "failed", "通过", "失败"];
    for text in jira.comments.iter().chain(jira.linked_titles.iter()) {
        let lower = text.to_lowercase();
        if patterns.iter().any(|p| lower.contains(&p.to_lowercase())) {
            out.push(text.clone());
        }
    }
    out.truncate(5);
    out
}

fn extract_acceptance(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let re = Regex::new(r"(?m)^\s*(?:[-*]|\d+[.)])\s+(.+)$").unwrap();
    for cap in re.captures_iter(text) {
        let item = clean_text_block(cap.get(1).map(|m| m.as_str()).unwrap_or_default());
        if item.len() > 6 {
            out.push(item);
        }
    }
    if out.is_empty() {
        for line in text.lines().take(5) {
            let line = clean_text_block(line);
            if line.len() > 12 {
                out.push(line);
            }
        }
    }
    out.truncate(5);
    out
}

fn clean_text_block(input: impl AsRef<str>) -> String {
    let s = input.as_ref().replace("\r\n", "\n").replace('\r', "\n");
    let lines = s
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    lines.join("\n")
}

pub fn infer_context_files_from_repo(repo_files: &[String], change_type: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    match change_type.unwrap_or("server") {
        "db" => collect_matching(repo_files, &["migration", "schema", ".sql"], &mut out),
        "frontend" => collect_matching(repo_files, &["routes", "router", "store", "page", "component"], &mut out),
        "infra" => collect_matching(repo_files, &["docker", ".github", "workflow", "deploy", "script"], &mut out),
        _ => collect_matching(repo_files, &["service", "handler", "controller", "api", "mod.rs"], &mut out),
    }
    out.truncate(5);
    out
}

fn collect_matching(repo_files: &[String], needles: &[&str], out: &mut Vec<String>) {
    for file in repo_files {
        let lower = file.to_lowercase();
        if needles.iter().any(|n| lower.contains(&n.to_lowercase())) && !out.contains(file) {
            out.push(file.clone());
        }
    }
}

pub fn maybe_expand_context_files(args: &mut PromptArgs, repo_files: &[String]) {
    if !args.context_files.is_empty() {
        return;
    }
    let inferred = infer_context_files_from_repo(repo_files, args.change_type.as_deref());
    args.context_files = inferred
        .into_iter()
        .map(|f| Path::new(&f).to_path_buf())
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acceptance_extraction_works() {
        let text = "1. 首次提交成功\n2. 重复提交返回冲突\n3. 网络重试不双写";
        let items = extract_acceptance(text);
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn infer_change_type_prefers_frontend_files() {
        let jira = JiraEnrichment::default();
        let ty = infer_change_type(&jira, &["src/app/page.tsx".into()]);
        assert_eq!(ty.as_deref(), Some("frontend"));
    }

    #[test]
    fn clean_text_compacts_blank_lines() {
        let cleaned = clean_text_block("a\n\n b \n");
        assert_eq!(cleaned, "a\nb");
    }
}
