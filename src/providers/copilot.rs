use crate::conversation::MessageRole;
use crate::providers::{ChatRequest, ChatResponse, LlmProvider};
use crate::session::SessionStore;
use anyhow::Result;

pub struct CopilotCliProvider {
    store: SessionStore,
}

impl CopilotCliProvider {
    pub fn new(store: SessionStore) -> Self {
        Self { store }
    }

    fn render_messages_as_prompt(&self, request: &ChatRequest) -> String {
        let mut out = String::new();
        for msg in &request.messages {
            let role = match msg.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
            };
            out.push_str(&format!("[{role}]\n{}\n\n", msg.content));
        }
        out
    }
}

impl LlmProvider for CopilotCliProvider {
    fn name(&self) -> &str {
        "copilot-cli"
    }

    fn chat(&self, request: &ChatRequest) -> Result<ChatResponse> {
        let prompt = self.render_messages_as_prompt(request);
        let content = crate::copilot::run_review(&self.store, &prompt, Some(request.model.as_str()))?;
        Ok(ChatResponse {
            content: content.clone(),
            finish_reason: Some("stop".into()),
            usage: None,
            raw: Some(content),
            request_id: None,
        })
    }
}
