use std::collections::BTreeSet;

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
    !stem.is_empty()
        && c.contains(&stem)
        && (c.contains("test") || c.contains("spec"))
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
}
