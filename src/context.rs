use anyhow::{Context, Result};
use serde::Serialize;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

const BINARY_SNIFF_BYTES: usize = 1024;

#[derive(Debug, Clone, Serialize, Default)]
pub struct ContextFile {
    pub path: String,
    pub content: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ContextCollection {
    pub files: Vec<ContextFile>,
    pub skipped: Vec<String>,
    pub truncated: Vec<String>,
}

pub fn read_repo_context_with_budget(
    repo: &PathBuf,
    files: &[String],
    budget_bytes: usize,
    file_max_bytes: usize,
) -> Result<ContextCollection> {
    let mut out = ContextCollection::default();
    let mut used = 0usize;

    for rel in files {
        let path = repo.join(rel);
        if !path.exists() || !path.is_file() {
            out.skipped.push(format!("{} (missing)", rel));
            continue;
        }

        let remaining_budget = budget_bytes.saturating_sub(used);
        if remaining_budget == 0 {
            out.skipped.push(format!("{} (budget exceeded)", rel));
            continue;
        }

        match read_text_prefix(&path, file_max_bytes.min(remaining_budget))? {
            ReadOutcome::Binary => {
                out.skipped.push(format!("{} (binary)", rel));
            }
            ReadOutcome::NonUtf8 => {
                out.skipped.push(format!("{} (non-utf8)", rel));
            }
            ReadOutcome::Text { content, truncated } => {
                used += content.len();
                if truncated {
                    out.truncated.push(rel.clone());
                }
                out.files.push(ContextFile {
                    path: rel.clone(),
                    content,
                    truncated,
                });
            }
        }
    }

    Ok(out)
}

enum ReadOutcome {
    Binary,
    NonUtf8,
    Text { content: String, truncated: bool },
}

fn read_text_prefix(path: &Path, max_bytes: usize) -> Result<ReadOutcome> {
    let metadata = path
        .metadata()
        .with_context(|| format!("failed to stat {}", path.display()))?;
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;

    let sniff_len = BINARY_SNIFF_BYTES.min(max_bytes.max(1));
    let mut sniff = vec![0u8; sniff_len];
    let sniff_read = file
        .read(&mut sniff)
        .with_context(|| format!("failed to read {}", path.display()))?;
    sniff.truncate(sniff_read);

    if sniff.iter().any(|b| *b == 0) {
        return Ok(ReadOutcome::Binary);
    }

    let mut bytes = sniff;
    if max_bytes > bytes.len() {
        let to_read = max_bytes - bytes.len();
        let mut rest = vec![0u8; to_read];
        let n = file
            .read(&mut rest)
            .with_context(|| format!("failed to read {}", path.display()))?;
        rest.truncate(n);
        bytes.extend_from_slice(&rest);
    }

    let metadata_len = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    let mut truncated = metadata_len > bytes.len();

    let valid_len = utf8_safe_prefix_len(&bytes);
    if valid_len == 0 && !bytes.is_empty() && std::str::from_utf8(&bytes).is_err() {
        return Ok(ReadOutcome::NonUtf8);
    }
    if valid_len < bytes.len() {
        truncated = true;
        bytes.truncate(valid_len);
    }

    let content = String::from_utf8(bytes)
        .with_context(|| format!("failed to decode {} as utf-8", path.display()))?;
    Ok(ReadOutcome::Text { content, truncated })
}

fn utf8_safe_prefix_len(bytes: &[u8]) -> usize {
    for len in (0..=bytes.len()).rev() {
        if std::str::from_utf8(&bytes[..len]).is_ok() {
            return len;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn respects_budget_and_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        let mut f = fs::File::create(&file).unwrap();
        write!(f, "1234567890").unwrap();

        let result =
            read_repo_context_with_budget(&dir.path().to_path_buf(), &["a.txt".to_string()], 5, 10)
                .unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].content, "12345");
        assert!(result.files[0].truncated);
    }

    #[test]
    fn keeps_utf8_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("utf8.txt");
        fs::write(&file, "你好世界").unwrap();

        let result = read_repo_context_with_budget(
            &dir.path().to_path_buf(),
            &["utf8.txt".to_string()],
            5,
            5,
        )
        .unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].content, "你");
        assert!(result.files[0].truncated);
    }

    #[test]
    fn skips_binary_without_full_read() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bin.dat");
        fs::write(&file, [0u8, 159, 146, 150]).unwrap();

        let result = read_repo_context_with_budget(
            &dir.path().to_path_buf(),
            &["bin.dat".to_string()],
            100,
            100,
        )
        .unwrap();
        assert!(result.files.is_empty());
        assert!(result.skipped[0].contains("binary"));
    }
}
