use crate::review_schema::{MissingTestCase, ReviewIssue, ReviewResult};

pub fn render_review_result_text(result: &ReviewResult) -> String {
    let mut out = String::new();
    out.push_str(&format!("mode: {}\n", result.mode));
    out.push_str(&format!("input_ok: {}\n", result.input_ok));
    out.push_str(&format!("input_level: {}\n", result.input_level));
    out.push_str(&format!("input_score: {}\n", result.input_score));
    out.push_str(&format!("confidence: {}\n\n", result.confidence));

    render_issue_block(&mut out, "1. 高风险问题", &result.high_risk);
    render_issue_block(&mut out, "2. 中风险问题", &result.medium_risk);
    render_issue_block(&mut out, "3. 低风险优化建议", &result.low_risk);
    render_tests_block(&mut out, "4. 缺失的测试场景", &result.missing_tests);

    if !result.impact_scope.is_empty() {
        out.push_str("5. 风险影响面\n");
        for item in &result.impact_scope {
            out.push_str(&format!("- {}\n", item));
        }
        out.push('\n');
    }

    if !result.risk_hints.is_empty() {
        out.push_str("6. 程序级风险提示\n");
        for hint in &result.risk_hints {
            out.push_str(&format!("- {}\n  detail: {}\n  source: {}\n", hint.title, hint.detail, hint.source));
        }
        out.push('\n');
    }

    if !result.release_checks.is_empty() {
        out.push_str("7. 发布建议 / 人工确认项\n");
        for item in &result.release_checks {
            out.push_str(&format!("- {}\n", item));
        }
        out.push('\n');
    }

    out.push_str("8. 总结结论\n");
    out.push_str(&format!("{}\n", result.summary));
    out
}

fn render_issue_block(out: &mut String, title: &str, issues: &[ReviewIssue]) {
    out.push_str(title);
    out.push('\n');
    if issues.is_empty() {
        out.push_str("- 无\n\n");
        return;
    }
    for issue in issues {
        out.push_str(&format!("- {}\n", issue.title));
        if let Some(v) = &issue.file { out.push_str(&format!("  file: {}\n", v)); }
        if let Some(v) = &issue.location { out.push_str(&format!("  location: {}\n", v)); }
        if let Some(v) = &issue.reason { out.push_str(&format!("  reason: {}\n", v)); }
        if let Some(v) = &issue.trigger { out.push_str(&format!("  trigger: {}\n", v)); }
        if let Some(v) = &issue.impact { out.push_str(&format!("  impact: {}\n", v)); }
        if let Some(v) = &issue.suggestion { out.push_str(&format!("  suggestion: {}\n", v)); }
    }
    out.push('\n');
}

fn render_tests_block(out: &mut String, title: &str, tests: &[MissingTestCase]) {
    out.push_str(title);
    out.push('\n');
    if tests.is_empty() {
        out.push_str("- 无\n\n");
        return;
    }
    for test in tests {
        out.push_str(&format!("- {}\n", test.title));
        if let Some(v) = &test.scenario { out.push_str(&format!("  scenario: {}\n", v)); }
    }
    out.push('\n');
}
