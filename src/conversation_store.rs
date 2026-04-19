use crate::conversation::{
    FindingPatch, FindingStatus, ReviewArtifact, ReviewFinding, ReviewMessage, ReviewSession,
    ReviewTurn, SessionListFilter, SessionSummary,
};
use anyhow::{anyhow, bail, Context, Result};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ConversationStore {
    root: PathBuf,
}

impl ConversationStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn default_root() -> Result<PathBuf> {
        let home = std::env::var("HOME").context("HOME is not set")?;
        Ok(Path::new(&home).join(".alma").join("review-sessions"))
    }

    pub fn new_default() -> Result<Self> {
        Ok(Self::new(Self::default_root()?))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn ensure_session_dirs(&self, session_id: &str) -> Result<PathBuf> {
        let dir = self.root.join(session_id);
        fs::create_dir_all(dir.join("turns"))?;
        fs::create_dir_all(dir.join("artifacts"))?;
        fs::create_dir_all(dir.join("checkpoints"))?;
        Ok(dir)
    }

    pub fn session_dir(&self, session_id: &str) -> PathBuf {
        self.root.join(session_id)
    }

    pub fn save_session(&self, session: &ReviewSession) -> Result<()> {
        let dir = self.ensure_session_dirs(&session.id)?;
        let path = dir.join("session.json");
        fs::write(&path, serde_json::to_vec_pretty(session)?)?;
        Ok(())
    }

    pub fn load_session(&self, session_id: &str) -> Result<ReviewSession> {
        let path = self.root.join(session_id).join("session.json");
        let bytes = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn session_exists(&self, session_id: &str) -> bool {
        self.session_dir(session_id).join("session.json").exists()
    }

    pub fn save_turn(&self, turn: &ReviewTurn) -> Result<()> {
        let dir = self.ensure_session_dirs(&turn.session_id)?;
        let path = dir.join("turns").join(format!("{:04}.json", turn.turn_no));
        fs::write(&path, serde_json::to_vec_pretty(turn)?)?;
        Ok(())
    }

    pub fn load_turns(&self, session_id: &str) -> Result<Vec<ReviewTurn>> {
        let dir = self.session_dir(session_id).join("turns");
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut paths = fs::read_dir(&dir)?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        paths.sort();
        let mut turns = Vec::new();
        for path in paths {
            let bytes = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
            turns.push(serde_json::from_slice(&bytes)?);
        }
        Ok(turns)
    }

    pub fn append_message(&self, msg: &ReviewMessage) -> Result<()> {
        let dir = self.ensure_session_dirs(&msg.session_id)?;
        let path = dir.join("messages.jsonl");
        let mut line = serde_json::to_string(msg)?;
        line.push('\n');
        let mut f = fs::OpenOptions::new().create(true).append(true).open(&path)?;
        f.write_all(line.as_bytes())?;
        Ok(())
    }

    pub fn load_messages(&self, session_id: &str) -> Result<Vec<ReviewMessage>> {
        let path = self.session_dir(session_id).join("messages.jsonl");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
        let reader = BufReader::new(file);
        let mut messages = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            messages.push(serde_json::from_str(&line)?);
        }
        Ok(messages)
    }

    pub fn next_message_seq(&self, session_id: &str) -> Result<u64> {
        Ok(self.load_messages(session_id)?.last().map(|m| m.seq_no + 1).unwrap_or(1))
    }

    pub fn save_findings(&self, session_id: &str, findings: &[ReviewFinding]) -> Result<()> {
        let dir = self.ensure_session_dirs(session_id)?;
        let path = dir.join("findings.json");
        fs::write(&path, serde_json::to_vec_pretty(findings)?)?;
        Ok(())
    }

    pub fn load_findings(&self, session_id: &str) -> Result<Vec<ReviewFinding>> {
        let path = self.session_dir(session_id).join("findings.json");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn list_sessions(&self, filter: &SessionListFilter) -> Result<Vec<SessionSummary>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut summaries: Vec<SessionSummary> = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let id = match path.file_name().and_then(|s| s.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };
            let session = match self.load_session(&id) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if let Some(repo) = &filter.repo {
                if !session.repo_root.display().to_string().contains(repo) {
                    continue;
                }
            }
            if let Some(status) = &filter.status {
                let want = status.to_lowercase();
                let current = format!("{:?}", session.status).to_lowercase();
                if current != want {
                    continue;
                }
            }
            if let Some(mode) = &filter.mode {
                let want = mode.to_lowercase();
                let current = match session.review_mode {
                    crate::cli::ReviewMode::Lite => "lite",
                    crate::cli::ReviewMode::Standard => "standard",
                    crate::cli::ReviewMode::Critical => "critical",
                };
                if current != want {
                    continue;
                }
            }
            let findings = self.load_findings(&id).unwrap_or_default();
            summaries.push(SessionSummary::from_session(&session, &findings));
        }
        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        let offset = filter.offset.unwrap_or(0);
        if offset > 0 {
            summaries.drain(0..offset.min(summaries.len()));
        }
        if let Some(limit) = filter.limit {
            summaries.truncate(limit);
        }
        Ok(summaries)
    }

    pub fn count_sessions(&self, filter: &SessionListFilter) -> Result<usize> {
        let mut count_filter = filter.clone();
        count_filter.limit = None;
        count_filter.offset = None;
        Ok(self.list_sessions(&count_filter)?.len())
    }

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        validate_session_id(session_id)?;
        let dir = self.session_dir(session_id);
        if !dir.exists() {
            bail!("session not found: {}", session_id);
        }
        fs::remove_dir_all(&dir).with_context(|| format!("failed to remove {}", dir.display()))?;
        Ok(())
    }

    pub fn update_finding(
        &self,
        session_id: &str,
        finding_id: &str,
        patch: &FindingPatch,
        now: &str,
    ) -> Result<ReviewFinding> {
        let mut findings = self.load_findings(session_id)?;
        let idx = findings
            .iter()
            .position(|f| f.id == finding_id)
            .ok_or_else(|| anyhow!("finding not found: {}", finding_id))?;
        let current_status = findings[idx].status.clone();
        if let Some(new_status) = patch.status.clone() {
            validate_status_transition(&current_status, &new_status)?;
            findings[idx].status = new_status.clone();
            match new_status {
                FindingStatus::Fixed | FindingStatus::AcceptedRisk => {
                    if findings[idx].resolved_at.is_none() {
                        findings[idx].resolved_at = Some(now.to_string());
                    }
                }
                _ => {
                    findings[idx].resolved_at = None;
                }
            }
        }
        if let Some(owner) = patch.owner.clone() {
            findings[idx].owner = Some(owner);
        }
        if let Some(tags) = patch.tags.clone() {
            findings[idx].tags = tags;
        }
        findings[idx].updated_at = now.to_string();
        let updated = findings[idx].clone();
        self.save_findings(session_id, &findings)?;
        Ok(updated)
    }

    pub fn save_artifact(&self, artifact: &ReviewArtifact) -> Result<()> {
        let dir = self.ensure_session_dirs(&artifact.session_id)?;
        let path = dir.join("artifacts").join(format!("{}.json", artifact.id));
        fs::write(&path, serde_json::to_vec_pretty(artifact)?)?;
        Ok(())
    }

    pub fn list_artifacts(&self, session_id: &str) -> Result<Vec<ReviewArtifact>> {
        let dir = self.session_dir(session_id).join("artifacts");
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut paths = fs::read_dir(&dir)?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        paths.sort();
        let mut out = Vec::new();
        for path in paths {
            let bytes = match fs::read(&path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            if let Ok(artifact) = serde_json::from_slice::<ReviewArtifact>(&bytes) {
                out.push(artifact);
            }
        }
        Ok(out)
    }
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.is_empty() {
        bail!("session id is empty");
    }
    if session_id.contains('/') || session_id.contains('\\') || session_id.contains("..") {
        bail!("invalid session id: {}", session_id);
    }
    Ok(())
}

fn validate_status_transition(current: &FindingStatus, next: &FindingStatus) -> Result<()> {
    use FindingStatus::*;
    if current == next {
        return Ok(());
    }
    let ok = matches!(
        (current, next),
        (_, Suspected)
            | (_, Confirmed)
            | (_, Dismissed)
            | (Suspected, Fixed)
            | (Suspected, AcceptedRisk)
            | (Confirmed, Fixed)
            | (Confirmed, AcceptedRisk)
            | (Fixed, AcceptedRisk)
            | (AcceptedRisk, Fixed)
    );
    if !ok {
        bail!("invalid status transition: {:?} -> {:?}", current, next);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::ReviewMode;
    use crate::conversation::{
        CodeLocation, ConversationStatus, FindingCategory, FindingEvidence, FindingSeverity,
        ReviewSession,
    };
    use tempfile::tempdir;

    fn sample_finding(id: &str, session_id: &str) -> ReviewFinding {
        ReviewFinding {
            id: id.into(),
            code: None,
            session_id: session_id.into(),
            source_turn_id: None,
            severity: FindingSeverity::High,
            category: FindingCategory::Logic,
            status: FindingStatus::Suspected,
            title: "t".into(),
            description: "d".into(),
            rationale: None,
            suggestion: None,
            confidence: None,
            owner: None,
            location: Some(CodeLocation {
                file_path: "src/x.rs".into(),
                line_start: None,
                line_end: None,
                symbol: None,
            }),
            evidence: vec![FindingEvidence {
                kind: "model_reasoning".into(),
                summary: "s".into(),
                content: None,
                artifact_id: None,
            }],
            related_files: vec![],
            tags: vec![],
            last_seen_turn: None,
            created_at: "1".into(),
            updated_at: "1".into(),
            resolved_at: None,
        }
    }

    #[test]
    fn status_transition_dismissed_to_fixed_is_blocked() {
        let err = validate_status_transition(&FindingStatus::Dismissed, &FindingStatus::Fixed)
            .unwrap_err();
        assert!(err.to_string().contains("invalid status transition"));
    }

    #[test]
    fn status_transition_common_paths_allowed() {
        assert!(validate_status_transition(&FindingStatus::Suspected, &FindingStatus::Confirmed).is_ok());
        assert!(validate_status_transition(&FindingStatus::Confirmed, &FindingStatus::Fixed).is_ok());
        assert!(validate_status_transition(&FindingStatus::Dismissed, &FindingStatus::Suspected).is_ok());
        assert!(validate_status_transition(&FindingStatus::Fixed, &FindingStatus::AcceptedRisk).is_ok());
    }

    #[test]
    fn update_finding_sets_resolved_at_for_fixed() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path().to_path_buf());
        let session = ReviewSession::new(
            "rs-test".into(),
            ReviewMode::Standard,
            "conversation",
            dir.path().to_path_buf(),
            "copilot-cli",
            "gpt-5.4",
            "1".into(),
        );
        store.save_session(&session).unwrap();
        let f = sample_finding("f1", "rs-test");
        store.save_findings("rs-test", &vec![f]).unwrap();

        let patch = FindingPatch {
            status: Some(FindingStatus::Fixed),
            owner: Some("alice".into()),
            tags: Some(vec!["ready".into()]),
        };
        let updated = store.update_finding("rs-test", "f1", &patch, "2").unwrap();
        assert_eq!(updated.status, FindingStatus::Fixed);
        assert_eq!(updated.owner.as_deref(), Some("alice"));
        assert_eq!(updated.resolved_at.as_deref(), Some("2"));
        assert_eq!(updated.tags, vec!["ready".to_string()]);

        // Flipping back to Suspected clears resolved_at.
        let patch2 = FindingPatch { status: Some(FindingStatus::Suspected), owner: None, tags: None };
        let back = store.update_finding("rs-test", "f1", &patch2, "3").unwrap();
        assert_eq!(back.resolved_at, None);
    }

    #[test]
    fn delete_session_rejects_path_traversal() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path().to_path_buf());
        assert!(store.delete_session("..").is_err());
        assert!(store.delete_session("a/b").is_err());
        assert!(store.delete_session("").is_err());
    }

    #[test]
    fn list_sessions_sorted_by_updated_at_desc() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path().to_path_buf());
        let mut a = ReviewSession::new(
            "rs-a".into(),
            ReviewMode::Standard,
            "conversation",
            dir.path().to_path_buf(),
            "copilot-cli",
            "m",
            "1".into(),
        );
        a.updated_at = "100".into();
        let mut b = ReviewSession::new(
            "rs-b".into(),
            ReviewMode::Standard,
            "conversation",
            dir.path().to_path_buf(),
            "copilot-cli",
            "m",
            "1".into(),
        );
        b.updated_at = "200".into();
        store.save_session(&a).unwrap();
        store.save_session(&b).unwrap();
        let filter = SessionListFilter::default();
        let items = store.list_sessions(&filter).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "rs-b");
        assert_eq!(items[1].id, "rs-a");
    }
}
