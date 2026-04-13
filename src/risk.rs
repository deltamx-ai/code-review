use crate::cli::PromptArgs;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
pub struct RiskHint {
    pub title: String,
    pub detail: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RiskAnalysis {
    pub impact_scope: Vec<String>,
    pub release_checks: Vec<String>,
    pub hints: Vec<RiskHint>,
}

pub fn analyze_risks(args: &PromptArgs, changed_files: &[String], diff_text: Option<&str>) -> RiskAnalysis {
    let mut out = RiskAnalysis::default();

    let files_lower = changed_files.iter().map(|f| f.to_lowercase()).collect::<Vec<_>>();
    let diff_lower = diff_text.unwrap_or_default().to_lowercase();

    if has_any(&files_lower, &[".sql", "migration", "schema.prisma"]) || diff_lower.contains("alter table") {
        out.hints.push(RiskHint {
            title: "数据库迁移风险".into(),
            detail: "检测到 SQL / migration / schema 相关改动，需确认在线迁移、索引影响和回滚方案。".into(),
            source: "file-or-diff".into(),
        });
        push_unique(&mut out.impact_scope, "数据库结构或数据迁移可能影响历史数据兼容性与线上写入稳定性");
        push_unique(&mut out.release_checks, "确认是否需要在线迁移、灰度发布和可执行回滚方案");
    }

    if has_any(&files_lower, &["api", "dto", "proto", "openapi", "graphql", "contract"]) {
        out.hints.push(RiskHint {
            title: "API / 契约变更风险".into(),
            detail: "检测到接口、DTO、契约或 schema 相关文件变更，需确认调用方兼容性。".into(),
            source: "file-path".into(),
        });
        push_unique(&mut out.impact_scope, "接口字段或契约变更可能影响上下游调用方、联调和向后兼容");
        push_unique(&mut out.release_checks, "确认调用方兼容策略、版本管理和联调窗口");
    }

    if has_any(&files_lower, &["auth", "permission", "role", "policy"]) || diff_lower.contains("permission") {
        out.hints.push(RiskHint {
            title: "权限 / 安全风险".into(),
            detail: "检测到权限、鉴权、角色或策略相关改动，需重点验证越权和漏校验。".into(),
            source: "file-or-diff".into(),
        });
        push_unique(&mut out.impact_scope, "权限控制改动可能引入越权访问、漏鉴权或角色边界错误");
        push_unique(&mut out.release_checks, "发布前确认权限回归测试、管理员场景和越权防护场景");
    }

    if has_any(&files_lower, &["handler", "controller", "service", "model", "repository"]) && changed_files.len() >= 3 {
        out.hints.push(RiskHint {
            title: "跨层联动风险".into(),
            detail: "检测到 handler/service/model 等多层同时修改，需关注模块影响面扩大。".into(),
            source: "file-path".into(),
        });
        push_unique(&mut out.impact_scope, "多层同时修改可能导致行为变化跨模块扩散，增加联调和回归范围");
    }

    match args.change_type.as_deref() {
        Some("server") => {
            push_unique(&mut out.release_checks, "确认事务边界、异常响应结构和并发场景回归测试");
        }
        Some("db") => {
            push_unique(&mut out.release_checks, "确认迁移执行顺序、锁表风险和数据回填策略");
        }
        Some("frontend") => {
            push_unique(&mut out.release_checks, "确认交互回归、路由权限、状态兼容和埋点正确性");
        }
        Some("infra") => {
            push_unique(&mut out.release_checks, "确认执行环境、触发条件、权限边界和失败回退策略");
        }
        _ => {}
    }

    if matches!(args.mode, crate::cli::ReviewMode::Critical) {
        push_unique(&mut out.release_checks, "critical 变更需人工确认发布策略、灰度范围和回滚负责人");
    }

    out
}

fn has_any(files_lower: &[String], keywords: &[&str]) -> bool {
    files_lower.iter().any(|f| keywords.iter().any(|k| f.contains(k)))
}

fn push_unique(target: &mut Vec<String>, value: &str) {
    if !target.iter().any(|v| v == value) {
        target.push(value.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{OutputFormat, PromptArgs, ReviewMode};

    fn base_args(change_type: Option<&str>, mode: ReviewMode) -> PromptArgs {
        PromptArgs {
            mode,
            stack: Some("Rust".into()),
            goal: Some("ship".into()),
            why: None,
            rules: vec!["rule".into()],
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
            files: vec![],
            focus: vec![],
            baseline_files: vec![],
            change_type: change_type.map(|s| s.to_string()),
            format: OutputFormat::Text,
        }
    }

    #[test]
    fn detects_db_migration_risk() {
        let args = base_args(Some("db"), ReviewMode::Standard);
        let files = vec!["migrations/001_add_order.sql".into()];
        let analysis = analyze_risks(&args, &files, None);
        assert!(analysis.hints.iter().any(|h| h.title.contains("数据库迁移")));
        assert!(!analysis.release_checks.is_empty());
    }

    #[test]
    fn detects_contract_risk() {
        let args = base_args(None, ReviewMode::Standard);
        let files = vec!["src/order/dto.rs".into(), "src/order/api.rs".into()];
        let analysis = analyze_risks(&args, &files, None);
        assert!(analysis.impact_scope.iter().any(|v| v.contains("调用方") || v.contains("兼容")));
    }
}
