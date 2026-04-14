use std::collections::BTreeSet;

#[derive(Debug, Clone, Default)]
pub struct DependencyExpansion {
    pub symbol_files: Vec<String>,
    pub reference_files: Vec<String>,
    pub route_chain_files: Vec<String>,
    pub import_chain_files: Vec<String>,
    pub frontend_chain_files: Vec<String>,
    pub backend_chain_files: Vec<String>,
}

impl DependencyExpansion {
    pub fn all_files(&self) -> Vec<String> {
        let mut set = BTreeSet::new();
        for file in self
            .symbol_files
            .iter()
            .chain(self.reference_files.iter())
            .chain(self.route_chain_files.iter())
            .chain(self.import_chain_files.iter())
            .chain(self.frontend_chain_files.iter())
            .chain(self.backend_chain_files.iter())
        {
            set.insert(file.clone());
        }
        set.into_iter().collect()
    }

    pub fn prioritized_groups(self) -> Vec<(&'static str, Vec<String>)> {
        vec![
            ("import-chain", self.import_chain_files),
            ("backend-chain", self.backend_chain_files),
            ("frontend-chain", self.frontend_chain_files),
            ("route-chain", self.route_chain_files),
            ("reference", self.reference_files),
            ("symbol", self.symbol_files),
        ]
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
    let mut import_chain_files = BTreeSet::new();
    let mut frontend_chain_files = BTreeSet::new();
    let mut backend_chain_files = BTreeSet::new();
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

    for (path, content) in file_contents {
        for import_path in extract_import_like_paths(content) {
            if repo_lookup.contains(&import_path) && !changed_set.contains(&import_path) {
                import_chain_files.insert(import_path);
            } else if let Some(found) = resolve_import_candidate(path, &import_path, repo_files) {
                if !changed_set.contains(&found) {
                    import_chain_files.insert(found);
                }
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

        if is_frontend_component_path(&lower) {
            for candidate in repo_files {
                let c = candidate.to_lowercase();
                if changed_set.contains(candidate) {
                    continue;
                }
                if (c.contains("store") || c.contains("hook") || c.contains("composable") || c.contains("service") || c.contains("api") || c.contains("query"))
                    && (base.is_empty() || c.contains(&base))
                {
                    frontend_chain_files.insert(candidate.clone());
                }
            }
        }

        if lower.contains("store") || lower.contains("hook") || lower.contains("composable") {
            for candidate in repo_files {
                let c = candidate.to_lowercase();
                if changed_set.contains(candidate) {
                    continue;
                }
                if (c.contains("component") || c.contains("page") || c.ends_with(".tsx") || c.ends_with(".jsx") || c.ends_with(".vue"))
                    && (base.is_empty() || c.contains(&base))
                {
                    frontend_chain_files.insert(candidate.clone());
                }
            }
        }

        if lower.contains("controller") || lower.contains("resource") {
            let class_base = base
                .replace("controller", "")
                .replace("resource", "")
                .trim()
                .to_string();
            for candidate in repo_files {
                let c = candidate.to_lowercase();
                if changed_set.contains(candidate) {
                    continue;
                }
                if (c.contains("service") || c.contains("repository") || c.contains("repo") || c.contains("entity") || c.contains("dto"))
                    && (base.is_empty() || c.contains(&base) || (!class_base.is_empty() && c.contains(&class_base)))
                {
                    backend_chain_files.insert(candidate.clone());
                }
            }
        }

        if lower.contains("service") || lower.contains("repository") || lower.contains("repo") {
            for candidate in repo_files {
                let c = candidate.to_lowercase();
                if changed_set.contains(candidate) {
                    continue;
                }
                if (c.contains("controller") || c.contains("resource") || c.contains("entity") || c.contains("dto"))
                    && (base.is_empty() || c.contains(&base))
                {
                    backend_chain_files.insert(candidate.clone());
                }
            }
        }
    }

    DependencyExpansion {
        symbol_files: filter_existing(symbol_files, &repo_lookup, 10),
        reference_files: filter_existing(reference_files, &repo_lookup, 10),
        route_chain_files: filter_existing(route_chain_files, &repo_lookup, 10),
        import_chain_files: filter_existing(import_chain_files, &repo_lookup, 12),
        frontend_chain_files: filter_existing(frontend_chain_files, &repo_lookup, 10),
        backend_chain_files: filter_existing(backend_chain_files, &repo_lookup, 10),
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
        for prefix in [
            "fn ", "pub fn ", "async fn ", "pub async fn ",
            "struct ", "pub struct ", "enum ", "pub enum ",
            "trait ", "pub trait ", "interface ", "class ",
            "type ", "pub type ",
        ] {
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

fn extract_import_like_paths(content: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    for line in content.lines() {
        let l = line.trim();
        for marker in ["from '", "from \"", "import '", "import \"", "require(\"", "require('", "use "] {
            if let Some(idx) = l.find(marker) {
                let rest = &l[idx + marker.len()..];
                let path = rest
                    .split(['\'', '"', ';', ')', ' '])
                    .next()
                    .unwrap_or("")
                    .trim();
                if !path.is_empty() && !path.starts_with('@') {
                    out.insert(path.to_string());
                }
            }
        }
    }
    out.into_iter().collect()
}

fn resolve_import_candidate(base_file: &str, import_path: &str, repo_files: &[String]) -> Option<String> {
    let base_dir = base_file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let normalized = import_path.trim_start_matches("./").trim_start_matches("../");

    let mut candidates = vec![
        format!("{}/{}", base_dir, normalized),
        normalized.to_string(),
        format!("{}.ts", normalized),
        format!("{}.tsx", normalized),
        format!("{}.js", normalized),
        format!("{}.jsx", normalized),
        format!("{}.vue", normalized),
        format!("{}.java", normalized),
        format!("{}.kt", normalized),
        format!("{}/index.ts", normalized),
        format!("{}/index.tsx", normalized),
        format!("{}/index.js", normalized),
    ];

    if base_dir.is_empty() {
        candidates.push(normalized.to_string());
    } else {
        candidates.push(format!("{}/{}.ts", base_dir, normalized));
        candidates.push(format!("{}/{}.tsx", base_dir, normalized));
        candidates.push(format!("{}/{}.js", base_dir, normalized));
        candidates.push(format!("{}/{}.jsx", base_dir, normalized));
        candidates.push(format!("{}/{}.vue", base_dir, normalized));
        candidates.push(format!("{}/{}.java", base_dir, normalized));
    }

    repo_files.iter().find(|f| candidates.iter().any(|c| normalize_path(c) == normalize_path(f))).cloned()
}

fn normalize_path(path: &str) -> String {
    path.replace("//", "/").trim_start_matches("./").to_string()
}

fn is_frontend_component_path(lower: &str) -> bool {
    lower.contains("component")
        || lower.contains("page")
        || lower.ends_with(".tsx")
        || lower.ends_with(".jsx")
        || lower.ends_with(".vue")
        || lower.contains("container")
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

    #[test]
    fn expands_import_chain_for_ts_and_frontend_patterns() {
        let changed = vec!["src/pages/orders.tsx".to_string()];
        let repo = vec![
            "src/pages/orders.tsx".to_string(),
            "src/services/orderService.ts".to_string(),
            "src/store/orderStore.ts".to_string(),
            "src/api/orders.ts".to_string(),
        ];
        let contents = vec![
            ("src/pages/orders.tsx".to_string(), "import { listOrders } from '../services/orderService'; import { useOrderStore } from '../store/orderStore';".to_string()),
            ("src/services/orderService.ts".to_string(), "export function listOrders() {}".to_string()),
            ("src/store/orderStore.ts".to_string(), "export function useOrderStore() {}".to_string()),
            ("src/api/orders.ts".to_string(), "export const ordersApi = {}".to_string()),
        ];
        let expanded = expand_dependency_files(&changed, &repo, &contents);
        let all = expanded.all_files();
        assert!(all.iter().any(|f| f.ends_with("orderService.ts")));
        assert!(all.iter().any(|f| f.ends_with("orderStore.ts")));
    }

    #[test]
    fn expands_java_backend_chain() {
        let changed = vec!["src/main/java/com/acme/order/OrderController.java".to_string()];
        let repo = vec![
            "src/main/java/com/acme/order/OrderController.java".to_string(),
            "src/main/java/com/acme/order/OrderService.java".to_string(),
            "src/main/java/com/acme/order/OrderRepository.java".to_string(),
            "src/main/java/com/acme/order/OrderDto.java".to_string(),
        ];
        let contents = vec![("src/main/java/com/acme/order/OrderController.java".to_string(), "class OrderController { OrderService service; }".to_string())];
        let expanded = expand_dependency_files(&changed, &repo, &contents);
        let all = expanded.all_files();
        assert!(all.iter().any(|f| f.ends_with("OrderService.java")));
        assert!(all.iter().any(|f| f.ends_with("OrderRepository.java")) || all.iter().any(|f| f.ends_with("OrderDto.java")));
    }
}
