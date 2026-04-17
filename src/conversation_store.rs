use crate::conversation::{ReviewFinding, ReviewMessage, ReviewSession, ReviewTurn};
use anyhow::{Context, Result};
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
}
