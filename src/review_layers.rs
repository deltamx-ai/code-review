use crate::cli::{PromptArgs, ReviewMode};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewLayerKind {
    Basic,
    Engineering,
    Business,
    Risk,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewLayer {
    pub kind: ReviewLayerKind,
    pub title: &'static str,
    pub goal: &'static str,
    pub checks: Vec<String>,
}

pub fn build_review_layers(args: &PromptArgs) -> Vec<ReviewLayer> {
    let mut layers = vec![basic_layer(), engineering_layer(), business_layer(args), risk_layer(args)];

    if matches!(args.mode, ReviewMode::Critical) {
        if let Some(layer) = layers.iter_mut().find(|l| l.kind == ReviewLayerKind::Risk) {
            layer.checks.push("评估发布、灰度、回滚和人工确认项".into());
            layer.checks.push("结合基线/红线文件检查安全、性能和变更边界".into());
        }
        if let Some(layer) = layers.iter_mut().find(|l| l.kind == ReviewLayerKind::Business) {
            layer.checks.push("优先验证支付、风控、权限、审批、计费等核心业务路径".into());
        }
    }

    layers
}

pub fn render_layers_prompt(layers: &[ReviewLayer]) -> String {
    let mut out = String::new();
    out.push_str("四层审查要求:\n");
    for layer in layers {
        out.push_str(&format!("- {}（{}）:\n", layer.title, layer.goal));
        for check in &layer.checks {
            out.push_str(&format!("  - {}\n", check));
        }
    }
    out
}

fn basic_layer() -> ReviewLayer {
    ReviewLayer {
        kind: ReviewLayerKind::Basic,
        title: "基础层",
        goal: "发现显性缺陷与边界错误",
        checks: vec![
            "检查空指针、None、unwrap、越界、空集合、空字符串等基础缺陷".into(),
            "检查异常处理、错误返回、资源释放、死代码和不可达逻辑".into(),
            "检查并发下的基础竞态、重复写、锁遗漏和幂等失效".into(),
        ],
    }
}

fn engineering_layer() -> ReviewLayer {
    ReviewLayer {
        kind: ReviewLayerKind::Engineering,
        title: "工程层",
        goal: "发现架构与工程实现问题",
        checks: vec![
            "检查架构分层是否被破坏，是否存在跨层直接调用或职责混乱".into(),
            "检查可维护性、配置使用、常量治理、依赖方向和模块边界是否合理".into(),
            "检查明显性能隐患、契约边界模糊和高成本实现".into(),
        ],
    }
}

fn business_layer(args: &PromptArgs) -> ReviewLayer {
    let mut checks = vec![
        "对照业务意图和业务规则，检查需求实现偏差与规则遗漏".into(),
        "检查权限校验、状态流转、幂等性、审批、计费、风控等关键逻辑".into(),
    ];
    if args.expected_normal.is_some() || args.expected_error.is_some() || args.expected_edge.is_some() {
        checks.push("结合正常/异常/边界预期，核对实现是否满足预期行为".into());
    } else {
        checks.push("如果预期行为不足，要明确指出证据不足和不确定点".into());
    }
    ReviewLayer {
        kind: ReviewLayerKind::Business,
        title: "业务层",
        goal: "检查需求与业务规则是否真正被正确实现",
        checks,
    }
}

fn risk_layer(args: &PromptArgs) -> ReviewLayer {
    let mut checks = vec![
        "评估模块影响面、向后兼容风险、联调风险和上下游依赖影响".into(),
        "检查 API 契约变化、数据结构变化和跨文件联动风险".into(),
    ];

    match args.change_type.as_deref() {
        Some("db") => {
            checks.push("重点检查数据库迁移风险、索引变更、在线迁移与回滚方案".into());
        }
        Some("server") => {
            checks.push("重点检查事务边界、异常响应结构、并发访问和依赖破坏风险".into());
        }
        Some("frontend") => {
            checks.push("重点检查交互兼容性、状态管理、权限/路由和埋点影响".into());
        }
        Some("infra") => {
            checks.push("重点检查执行环境、触发条件、权限范围和失败回退策略".into());
        }
        _ => {
            checks.push("根据变更文件与上下文评估潜在发布和兼容性风险".into());
        }
    }

    ReviewLayer {
        kind: ReviewLayerKind::Risk,
        title: "风险层",
        goal: "评估影响面与发布风险",
        checks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::OutputFormat;

    fn base_args(mode: ReviewMode) -> PromptArgs {
        PromptArgs {
            mode,
            stack: Some("Rust".into()),
            goal: Some("fix order".into()),
            why: None,
            rules: vec!["rule".into()],
            risks: vec![],
            expected_normal: Some("ok".into()),
            expected_error: None,
            expected_edge: None,
            issue: Some("issue".into()),
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
            change_type: Some("db".into()),
            format: OutputFormat::Text,
        }
    }

    #[test]
    fn critical_adds_extra_risk_checks() {
        let args = base_args(ReviewMode::Critical);
        let layers = build_review_layers(&args);
        let risk = layers.iter().find(|l| l.kind == ReviewLayerKind::Risk).unwrap();
        assert!(risk.checks.iter().any(|c| c.contains("回滚")));
    }

    #[test]
    fn render_layers_contains_all_titles() {
        let args = base_args(ReviewMode::Standard);
        let text = render_layers_prompt(&build_review_layers(&args));
        assert!(text.contains("基础层"));
        assert!(text.contains("工程层"));
        assert!(text.contains("业务层"));
        assert!(text.contains("风险层"));
    }
}
