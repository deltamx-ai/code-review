use crate::cli::ReviewMode;
use crate::review_schema::{MissingTestCase, ReviewIssue, ReviewResult};
use regex::Regex;

pub fn parse_review_text(mode: ReviewMode, raw_text: &str, used_rules: Vec<String>) -> ReviewResult {
    let mut result = ReviewResult::new(mode, raw_text.to_string());
    result.used_rules = used_rules;

    let mut current = Section::None;
    for raw_line in raw_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(section) = detect_section(line) {
            current = section;
            continue;
        }
        match current {
            Section::High => push_issue(&mut result.high_risk, line),
            Section::Medium => push_issue(&mut result.medium_risk, line),
            Section::Low => push_issue(&mut result.low_risk, line),
            Section::MissingTests => push_test(&mut result.missing_tests, line),
            Section::Summary => append_summary(&mut result.summary, line),
            Section::ImpactScope => result.impact_scope.push(clean_bullet(line)),
            Section::ReleaseChecks => result.release_checks.push(clean_bullet(line)),
            Section::None => {
                if result.summary.is_empty() && !looks_like_issue(line) {
                    append_summary(&mut result.summary, line);
                }
            }
        }
    }

    if matches!(mode, ReviewMode::Critical) {
        backfill_critical_sections(raw_text, &mut result);
    }
    result.finalize();
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    High,
    Medium,
    Low,
    MissingTests,
    Summary,
    ImpactScope,
    ReleaseChecks,
}

fn detect_section(line: &str) -> Option<Section> {
    let lower = line.to_lowercase();
    let normalized = clean_bullet(line)
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')' || c == '、' || c.is_whitespace())
        .trim()
        .to_string();
    let normalized_lower = normalized.to_lowercase();

    match normalized_lower.as_str() {
        "高风险问题" | "high risk" | "high risk issues" => return Some(Section::High),
        "中风险问题" | "medium risk" | "medium risk issues" => return Some(Section::Medium),
        "低风险优化建议" | "低风险问题" | "优化建议" | "low risk" | "low risk issues" => return Some(Section::Low),
        "缺失的测试场景" | "缺失的测试" | "missing tests" | "missing test cases" => return Some(Section::MissingTests),
        "总结结论" | "总结" | "结论" | "summary" => return Some(Section::Summary),
        "风险影响面" | "影响面" | "impact scope" => return Some(Section::ImpactScope),
        "发布建议 / 人工确认项" | "发布建议" | "人工确认项" | "release checks" | "release check" => return Some(Section::ReleaseChecks),
        _ => {}
    }

    if line.len() < 40 {
        if lower == "1." || lower.starts_with("1. ") { return Some(Section::High); }
        if lower == "2." || lower.starts_with("2. ") { return Some(Section::Medium); }
        if lower == "3." || lower.starts_with("3. ") { return Some(Section::Low); }
        if lower == "4." || lower.starts_with("4. ") { return Some(Section::MissingTests); }
        if lower == "5." || lower.starts_with("5. ") { return Some(Section::Summary); }
        if lower == "6." || lower.starts_with("6. ") { return Some(Section::ImpactScope); }
        if lower == "7." || lower.starts_with("7. ") { return Some(Section::ReleaseChecks); }
    }

    None
}

fn push_issue(target: &mut Vec<ReviewIssue>, line: &str) {
    if !looks_like_issue(line) {
        if let Some(last) = target.last_mut() {
            let extra = clean_bullet(line);
            if !extra.is_empty() {
                last.reason = merge_field(last.reason.take(), extra);
            }
        }
        return;
    }

    let cleaned = clean_bullet(line);
    let (file, location) = extract_file_and_location(&cleaned);
    let title = extract_title(&cleaned);
    let lower = cleaned.to_lowercase();
    let reason = find_after_keyword(&cleaned, &["原因", "because", "reason", "危险原因"])
        .or_else(|| lower.contains("可能") .then(|| cleaned.clone()));
    let trigger = find_after_keyword(&cleaned, &["触发", "条件", "当", "trigger"]);
    let impact = find_after_keyword(&cleaned, &["影响", "impact"]);
    let suggestion = find_after_keyword(&cleaned, &["建议", "修复", "suggest", "fix"]);

    target.push(ReviewIssue {
        title,
        file,
        location,
        reason,
        trigger,
        impact,
        suggestion,
    });
}

fn push_test(target: &mut Vec<MissingTestCase>, line: &str) {
    let cleaned = clean_bullet(line);
    if cleaned.is_empty() {
        return;
    }
    target.push(MissingTestCase {
        title: cleaned.clone(),
        scenario: extract_after_colon(&cleaned),
    });
}

fn append_summary(summary: &mut String, line: &str) {
    let cleaned = clean_bullet(line);
    if cleaned.is_empty() {
        return;
    }
    if !summary.is_empty() {
        summary.push('\n');
    }
    summary.push_str(&cleaned);
}

fn clean_bullet(line: &str) -> String {
    line.trim()
        .trim_start_matches('-')
        .trim_start_matches('*')
        .trim_start_matches('•')
        .trim()
        .to_string()
}

fn extract_file_and_location(line: &str) -> (Option<String>, Option<String>) {
    let file_re = Regex::new(r"([A-Za-z0-9_./-]+\.(rs|ts|tsx|js|jsx|java|go|py|sql|yml|yaml))").unwrap();
    let loc_re = Regex::new(r"([A-Za-z_][A-Za-z0-9_]{2,})\s*(?:\(|函数|method)").unwrap();
    let file = file_re.captures(line).map(|c| c[1].to_string());
    let location = loc_re.captures(line).map(|c| c[1].to_string())
        .or_else(|| extract_after_symbol(line, ':'));
    (file, location)
}

fn extract_title(line: &str) -> String {
    let cleaned = clean_bullet(line);
    if let Some((_, rest)) = cleaned.split_once(':') {
        return rest.trim().to_string();
    }
    cleaned
}

fn extract_after_colon(line: &str) -> Option<String> {
    line.split_once(':').map(|(_, v)| v.trim().to_string()).filter(|s| !s.is_empty())
}

fn extract_after_symbol(line: &str, sym: char) -> Option<String> {
    line.split_once(sym).map(|(_, v)| v.trim().to_string()).filter(|s| !s.is_empty())
}

fn find_after_keyword(line: &str, keywords: &[&str]) -> Option<String> {
    for keyword in keywords {
        if let Some(idx) = line.to_lowercase().find(&keyword.to_lowercase()) {
            let suffix = line[idx + keyword.len()..].trim().trim_start_matches(':').trim().to_string();
            if !suffix.is_empty() {
                return Some(suffix);
            }
        }
    }
    None
}

fn merge_field(existing: Option<String>, extra: String) -> Option<String> {
    match existing {
        Some(v) if !v.is_empty() => Some(format!("{}；{}", v, extra)),
        _ => Some(extra),
    }
}

fn looks_like_issue(line: &str) -> bool {
    line.starts_with('-')
        || line.starts_with('*')
        || line.starts_with('•')
        || line.contains(':')
        || line.contains("可能")
        || line.contains("风险")
}

fn backfill_critical_sections(raw_text: &str, result: &mut ReviewResult) {
    for line in raw_text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let lower = line.to_lowercase();
        if result.impact_scope.is_empty() && (lower.contains("兼容") || lower.contains("迁移") || lower.contains("联调") || lower.contains("上下游")) {
            result.impact_scope.push(clean_bullet(line));
        }
        if result.release_checks.is_empty() && (lower.contains("发布") || lower.contains("回滚") || lower.contains("人工确认") || lower.contains("灰度")) {
            result.release_checks.push(clean_bullet(line));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_review_sections() {
        let text = r#"
1. 高风险问题
- src/order/service.rs:create_order 可能重复下单 原因: 缺少幂等校验 触发: 并发重试 影响: 重复扣款 建议: 增加唯一约束
2. 中风险问题
- src/order/dto.rs: 契约字段变更可能影响调用方
3. 低风险优化建议
- 可以补充日志字段
4. 缺失的测试场景
- 并发重试场景
5. 总结结论
- 发现 1 个高风险问题
"#;
        let result = parse_review_text(ReviewMode::Standard, text, vec!["rule1".into()]);
        assert_eq!(result.high_risk.len(), 1);
        assert_eq!(result.medium_risk.len(), 1);
        assert_eq!(result.low_risk.len(), 1);
        assert_eq!(result.missing_tests.len(), 1);
        assert!(result.summary.contains("高风险"));
    }

    #[test]
    fn critical_backfills_impact_and_release_checks() {
        let text = r#"
高风险问题
- migration.sql: 数据迁移可能影响上下游兼容
总结结论
- 发布前需要人工确认并准备回滚方案
"#;
        let result = parse_review_text(ReviewMode::Critical, text, vec![]);
        assert!(!result.impact_scope.is_empty());
        assert!(!result.release_checks.is_empty());
    }
}
