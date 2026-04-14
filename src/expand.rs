use std::collections::BTreeSet;

#[derive(Debug, Clone, Default)]
pub struct DependencyExpansion {
    pub symbol_files: Vec<String>,
    pub reference_files: Vec<String>,
    pub route_chain_files: Vec<String>,
}

impl DependencyExpansion {
    pub fn all_files(&self) -> Vec<String> {
        let mut set = BTreeSet::new();
        for file in self
            .symbol_files
            .iter()
            .chain(self.reference_files.iter())
            .chain(self.route_chain_files.iter())
        {
            set.insert(file.clone());
        }
        set.into_iter().collect()
    }
}

pub fn expand_related_files(changed_files: &[String], repo_files: &[String]) -> Vec<String> {
    let mut out = BTreeSet::new();
    let repo = repo_files.iter().cloned().collect::<Vec<_>>();

    for file in changed_files {
        let lower = file.to_lowercase();
        let stem = file_stem_name(file);
        let dir = file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");

        for candidate in &repo {
            if candidate == file {
                continue;
            }
            let c_lower = candidate.to_lowercase();
            let same_dir = !dir.is_empty() && candidate.starts_with(&(dir.to_string() + "/"));
            let same_stem = !stem.is_empty() && c_lower.contains(&stem.to_lowercase());

            if is_related_test(file, candidate)
                || is_related_schema(candidate)
                || is_related_contract(candidate)
                || (same_dir && is_high_value_neighbor(candidate))
                || (same_stem && is_context_pair(&lower, &c_lower))
            {
                out.insert(candidate.clone());
            }
        }
    }

    out.into_iter().take(12).collect()
}

pub fn expand_dependency_files(
    changed_files: &[String],
    repo_files: &[String],
    file_contents: &[(String, String)],
) -> DependencyExpansion {
    let mut symbol_files = BTreeSet::new();
    let mut reference_files = BTreeSet::new();
    let mut route_chain_files = BTreeSet::new();
    let changed_set = changed_files.iter().cloned().collect::<BTreeSet<_>>();

    let repo_lookup = repo_files.iter().cloned().collect::<BTreeSet<_>>();
    let extracted_symbols = file_contents
        .iter()
        .flat_map(|(_, content)| extract_symbols(content))
        .collect::<BTreeSet<_>>();

    for symbol in &extracted_symbols {
        for (path, content) in file_contents {
            if changed_set.contains(path) {
                continue;
            }
            if defines_symbol(path, content, symbol) {
                symbol_files.insert(path.clone());
            }
        }

        for candidate in repo_files {
            if changed_set.contains(candidate) {
                continue;
            }
            if candidate.to_lowercase().contains(&symbol.to_lowercase()) {
                symbol_files.insert(candidate.clone());
            }
        }
    }

    for symbol in &extracted_symbols {
        for (path, content) in file_contents {
            if changed_set.contains(path) {
                continue;
            }
            if references_symbol(content, symbol) {
                reference_files.insert(path.clone());
            }
        }
    }

    for changed in changed_files {
        let lower = changed.to_lowercase();
        let base = file_stem_name(changed).to_lowercase();
        if lower.contains("handler") || lower.contains("controller") || lower.contains("route") || lower.contains("router") {
            for candidate in repo_files {
                let c = candidate.to_lowercase();
                if changed_set.contains(candidate) {
                    continue;
                }
                if (c.contains("service") || c.contains("repo") || c.contains("store") || c.contains("dto") || c.contains("api"))
                    && (base.is_empty() || c.contains(&base))
                {
                    route_chain_files.insert(candidate.clone());
                }
            }
        }
        if lower.contains("service") || lower.contains("repo") || lower.contains("store") {
            for candidate in repo_files {
                let c = candidate.to_lowercase();
                if changed_set.contains(candidate) {
                    continue;
                }
                if (c.contains("handler") || c.contains("controller") || c.contains("route") || c.contains("router") || c.contains("dto") || c.contains("api"))
                    && (base.is_empty() || c.contains(&base))
                {
                    route_chain_files.insert(candidate.clone());
                }
            }
        }
    }

    DependencyExpansion {
        symbol_files: filter_existing(symbol_files, &repo_lookup, 10),
        reference_files: filter_existing(reference_files, &repo_lookup, 10),
        route_chain_files: filter_existing(route_chain_files, &repo_lookup, 10),
    }
}

fn filter_existing(set: BTreeSet<String>, repo_lookup: &BTreeSet<String>, limit: usize) -> Vec<String> {
    set.into_iter().filter(|f| repo_lookup.contains(f)).take(limit).collect()
}

fn file_stem_name(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let name = name.split('.').next().unwrap_or(name);
    name.replace("_test", "")
        .replace(".test", "")
        .replace(".spec", "")
        .replace("Test", "")
}

fn is_related_test(changed: &str, candidate: &str) -> bool {
    let stem = file_stem_name(changed).to_lowercase();
    let c = candidate.to_lowercase();
    !stem.is_empty() && c.contains(&stem) && (c.contains("test") || c.contains("spec"))
}

fn is_related_schema(candidate: &str) -> bool {
    let c = candidate.to_lowercase();
    ["schema", "dto", "model", "entity", "migration", "prisma", ".sql"]
        .iter()
        .any(|k| c.contains(k))
}

fn is_related_contract(candidate: &str) -> bool {
    let c = candidate.to_lowercase();
    ["interface", "trait", "types", "contract", "api", "proto", "openapi", "graphql"]
        .iter()
        .any(|k| c.contains(k))
}

fn is_high_value_neighbor(candidate: &str) -> bool {
    let c = candidate.to_lowercase();
    ["mod.rs", "lib.rs", "service", "handler", "controller", "router", "config", "test", "spec"]
        .iter()
        .any(|k| c.contains(k))
}

fn is_context_pair(changed: &str, candidate: &str) -> bool {
    let pairs = [
        ("service", "handler"),
        ("controller", "service"),
        ("api", "dto"),
        ("page", "store"),
        ("component", "store"),
        ("model", "schema"),
    ];
    pairs.iter().any(|(a, b)| {
        (changed.contains(a) && candidate.contains(b)) || (changed.contains(b) && candidate.contains(a))
    })
}

fn extract_symbols(content: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    for line in content.lines() {
        let l = line.trim();
        for prefix in ["fn ", "pub fn ", "async fn ", "pub async fn ", "struct ", "pub struct ", "enum ", "pub enum ", "trait ", "pub trait ", "interface ", "class ", "type ", "pub type "] {
            if let Some(rest) = l.strip_prefix(prefix) {
                let name = rest
                    .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or("")
                    .trim();
                if name.len() >= 3 {
                    out.insert(name.to_string());
                }
            }
        }
    }
    out.into_iter().collect()
}

fn defines_symbol(path: &str, content: &str, symbol: &str) -> bool {
    let path_hit = path.to_lowercase().contains(&symbol.to_lowercase());
    if path_hit {
        return true;
    }
    content.lines().any(|line| {
        let l = line.trim();
        l.starts_with("fn ")
            || l.starts_with("pub fn ")
            || l.starts_with("async fn ")
            || l.starts_with("pub async fn ")
            || l.starts_with("struct ")
            || l.starts_with("pub struct ")
            || l.starts_with("enum ")
            || l.starts_with("pub enum ")
            || l.starts_with("trait ")
            || l.starts_with("pub trait ")
            || l.starts_with("interface ")
            || l.starts_with("class ")
    }) && content.contains(symbol)
}

fn references_symbol(content: &str, symbol: &str) -> bool {
    let exact_call = format!("{}(", symbol);
    let exact_ref = format!(" {} ", symbol);
    content.contains(&exact_call) || content.contains(symbol) || content.contains(&exact_ref)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_related_test_and_schema_files() {
        let changed = vec!["src/order/service.rs".to_string()];
        let repo = vec![
            "src/order/service.rs".to_string(),
            "src/order/service_test.rs".to_string(),
            "src/order/model.rs".to_string(),
            "src/order/dto.rs".to_string(),
            "src/order/mod.rs".to_string(),
        ];
        let expanded = expand_related_files(&changed, &repo);
        assert!(expanded.iter().any(|f| f.ends_with("service_test.rs")));
        assert!(expanded.iter().any(|f| f.ends_with("dto.rs")) || expanded.iter().any(|f| f.ends_with("model.rs")));
    }

    #[test]
    fn expands_symbol_definition_and_references() {
        let changed = vec!["src/order/service.rs".to_string()];
        let repo = vec![
            "src/order/service.rs".to_string(),
            "src/order/handler.rs".to_string(),
            "src/order/repo.rs".to_string(),
            "src/order/dto.rs".to_string(),
        ];
        let contents = vec![
            ("src/order/service.rs".to_string(), "pub fn create_payment() { save_order(); } struct PaymentDto {}".to_string()),
            ("src/order/handler.rs".to_string(), "fn handle() { create_payment(); }".to_string()),
            ("src/order/repo.rs".to_string(), "pub fn save_order() {}".to_string()),
            ("src/order/dto.rs".to_string(), "pub struct PaymentDto { id: String }".to_string()),
        ];
        let expanded = expand_dependency_files(&changed, &repo, &contents);
        let all = expanded.all_files();
        assert!(all.iter().any(|f| f.ends_with("handler.rs")));
        assert!(all.iter().any(|f| f.ends_with("repo.rs")) || all.iter().any(|f| f.ends_with("dto.rs")));
    }
}
