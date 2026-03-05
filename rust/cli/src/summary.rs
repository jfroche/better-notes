use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::conversation::ConversationEntry;
use crate::git::Commit;
use crate::pr::PullRequest;

/// LLM-based summarizer using Claude API
pub struct Summarizer {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<Message>,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    text: Option<String>,
}

impl Summarizer {
    /// Create a new summarizer from environment
    pub fn new() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .context("ANTHROPIC_API_KEY environment variable not set")?;

        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            model: "claude-3-haiku-20240307".to_string(),
        })
    }

    /// Check if the summarizer is available (API key is set)
    #[allow(dead_code)]
    pub fn is_available() -> bool {
        std::env::var("ANTHROPIC_API_KEY").is_ok()
    }

    /// Generate a narrative summary from commit messages, PR descriptions, and conversation context
    pub async fn summarize(
        &self,
        commits: &[Commit],
        prs: &[PullRequest],
        conversations: &[&ConversationEntry],
    ) -> Result<String> {
        if commits.is_empty() && prs.is_empty() && conversations.is_empty() {
            return Ok(String::new());
        }

        // Use full commit messages (subject + body) for better context
        let commit_list: String = commits
            .iter()
            .map(|c| format!("- {}", c.full_message()))
            .collect::<Vec<_>>()
            .join("\n");

        // Include PR titles and descriptions
        let pr_list: String = prs
            .iter()
            .map(|pr| {
                let desc = pr
                    .description
                    .as_ref()
                    .filter(|d| !d.is_empty())
                    .map(|d| format!("\n  {}", d.lines().take(5).collect::<Vec<_>>().join("\n  ")))
                    .unwrap_or_default();
                format!("- PR #{}: {}{}", pr.number, pr.title, desc)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let conversation_list: String = conversations
            .iter()
            .map(|e| format!("- {}", e.display))
            .collect::<Vec<_>>()
            .join("\n");

        let mut context = String::new();
        if !commits.is_empty() {
            context.push_str("Commits:\n");
            context.push_str(&commit_list);
        }
        if !prs.is_empty() {
            if !context.is_empty() {
                context.push_str("\n\n");
            }
            context.push_str("Pull Requests:\n");
            context.push_str(&pr_list);
        }
        if !conversations.is_empty() {
            if !context.is_empty() {
                context.push_str("\n\n");
            }
            context.push_str("Conversation topics (developer's questions and intent during this work):\n");
            context.push_str(&conversation_list);
        }

        let prompt = format!(
            r#"Summarize this git activity in 1-2 sentences, focusing on what was accomplished and the value delivered. Be concise and use past tense. Write in first person (e.g., "I implemented...", "I fixed...").

{context}

Write a summary suitable for daily standup notes and timesheet entries to report work to clients. Focus on the "what" and "why" rather than technical implementation details. Use conversation topics to understand intent behind the commits. Do not use bullet points, just a short paragraph in first person."#
        );

        let request = AnthropicRequest {
            model: self.model.clone(),
            max_tokens: 300,
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt,
            }],
        };

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .context("Failed to send request to Anthropic API")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API error: {} - {}", status, body);
        }

        let result: AnthropicResponse = response
            .json()
            .await
            .context("Failed to parse Anthropic API response")?;

        Ok(result
            .content
            .first()
            .and_then(|c| c.text.clone())
            .unwrap_or_default()
            .trim()
            .to_string())
    }
}
