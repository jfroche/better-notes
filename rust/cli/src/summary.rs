use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::git::Commit;

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
    pub fn is_available() -> bool {
        std::env::var("ANTHROPIC_API_KEY").is_ok()
    }

    /// Generate a narrative summary from commit messages
    pub async fn summarize(&self, commits: &[Commit]) -> Result<String> {
        if commits.is_empty() {
            return Ok(String::new());
        }

        // Use full commit messages (subject + body) for better context
        let commit_list: String = commits
            .iter()
            .map(|c| format!("- {}", c.full_message()))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            r#"Summarize these git commits in 1-2 sentences, focusing on what was accomplished and the value delivered. Be concise and use past tense.

Commits:
{commit_list}

Write a summary suitable for daily standup notes and timesheet entries to report work to clients. Focus on the "what" and "why" rather than technical implementation details. Do not use bullet points, just a short paragraph."#
        );

        let request = AnthropicRequest {
            model: self.model.clone(),
            max_tokens: 150,
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
