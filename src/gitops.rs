use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

pub fn ensure_git_repo(repo: &PathBuf) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .output()
        .with_context(|| format!("failed to run git in {}", repo.display()))?;
    if !output.status.success() {
        bail!("{} is not a git repository", repo.display());
    }
    Ok(())
}

pub fn git_diff(repo: &PathBuf, rev: &str) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("diff")
        .arg(rev)
        .output()
        .with_context(|| format!("failed to get git diff for {}", rev))?;
    if !output.status.success() {
        bail!("git diff failed for {}", rev);
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn git_changed_files(repo: &PathBuf, rev: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("diff")
        .arg("--name-only")
        .arg(rev)
        .output()
        .with_context(|| format!("failed to list changed files for {}", rev))?;
    if !output.status.success() {
        bail!("git diff --name-only failed for {}", rev);
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}
