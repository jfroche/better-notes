use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::forge::Forge;
use crate::git::Repository;

/// A single entry from Claude Code's history.jsonl
#[derive(Debug, Clone)]
pub struct ConversationEntry {
    pub display: String,
    pub timestamp: DateTime<Utc>,
    pub project: PathBuf,
    pub session_id: String,
}

#[derive(Deserialize)]
struct HistoryLine {
    display: Option<String>,
    timestamp: Option<i64>,
    project: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
}

/// Read Claude Code history entries within a date range
pub fn read_history(since: &DateTime<Utc>, until: &DateTime<Utc>) -> Result<Vec<ConversationEntry>> {
    let history_path = match dirs::home_dir() {
        Some(home) => home.join(".claude").join("history.jsonl"),
        None => return Ok(Vec::new()),
    };

    if !history_path.exists() {
        tracing::debug!("Claude history file not found at {:?}", history_path);
        return Ok(Vec::new());
    }

    let contents = std::fs::read_to_string(&history_path)
        .context("failed to read Claude history.jsonl")?;

    let since_millis = since.timestamp_millis();
    let until_millis = until.timestamp_millis();

    let mut entries = Vec::new();
    for line in contents.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let parsed: HistoryLine = match serde_json::from_str(line) {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!("skipping malformed history line: {}", e);
                continue;
            }
        };

        let timestamp = match parsed.timestamp {
            Some(ts) => ts,
            None => continue,
        };

        if timestamp < since_millis || timestamp > until_millis {
            continue;
        }

        let display = match parsed.display {
            Some(d) if !d.trim().is_empty() => d.trim().to_string(),
            _ => continue,
        };

        let session_id = match parsed.session_id {
            Some(id) if !id.is_empty() => id,
            _ => continue,
        };

        let project = match parsed.project {
            Some(p) if !p.is_empty() => PathBuf::from(p),
            _ => continue,
        };

        let dt = DateTime::from_timestamp_millis(timestamp)
            .unwrap_or(*since);

        entries.push(ConversationEntry {
            display,
            timestamp: dt,
            project,
            session_id,
        });
    }

    Ok(entries)
}

/// Match conversation entries to discovered repositories by longest path prefix
pub fn match_to_repos<'a>(
    entries: &'a [ConversationEntry],
    repos: &[Repository],
) -> Vec<(&'a ConversationEntry, Forge)> {
    let mut matched = Vec::new();

    for entry in entries {
        let mut best_match: Option<(&Forge, usize)> = None;

        for repo in repos {
            if let Some(forge) = &repo.forge {
                let repo_str = repo.path.to_string_lossy();
                let entry_str = entry.project.to_string_lossy();

                if entry_str.starts_with(repo_str.as_ref()) {
                    let len = repo_str.len();
                    if best_match.is_none_or(|(_, best_len)| len > best_len) {
                        best_match = Some((forge, len));
                    }
                }
            }
        }

        if let Some((forge, _)) = best_match {
            matched.push((entry, forge.clone()));
        }
    }

    matched
}

/// Construct the path to a session's jsonl file in Claude's project directory
pub fn session_file_path(project: &Path, session_id: &str) -> PathBuf {
    let encoded = project
        .to_string_lossy()
        .replace('/', "-");

    dirs::home_dir()
        .expect("could not determine home directory")
        .join(".claude")
        .join("projects")
        .join(encoded)
        .join(format!("{session_id}.jsonl"))
}

/// Convert a session jsonl to markdown using cclog
pub fn convert_session(session_jsonl: &Path, output_path: &Path) -> Result<bool> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .context("failed to create conversations output directory")?;
    }

    let output = Command::new("cclog")
        .arg(session_jsonl)
        .arg("-o")
        .arg(output_path)
        .output();

    match output {
        Ok(result) if result.status.success() => {
            tracing::debug!("converted session to {:?}", output_path);
            Ok(true)
        }
        Ok(result) => {
            tracing::warn!(
                "cclog failed: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            Ok(false)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!("cclog not found on PATH, skipping session conversion");
            Ok(false)
        }
        Err(e) => Err(e).context("failed to run cclog"),
    }
}

/// Check whether cclog is available on PATH
pub fn cclog_available() -> bool {
    Command::new("cclog")
        .arg("--help")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Generate a filename for a conversation markdown file
pub fn conversation_filename(forge: &Forge, first_prompt: &str) -> String {
    let (owner, repo) = forge.owner_repo();
    let slug = first_prompt
        .split_whitespace()
        .take(6)
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>();

    let slug = if slug.len() > 60 { &slug[..60] } else { &slug };

    format!("{owner}-{repo}--{slug}.md")
}

/// Convert matched conversations, deduplicating by session id.
/// Returns a map from forge display name to vec of (entry, optional markdown path).
pub fn convert_all_sessions(
    matched: &[(&ConversationEntry, Forge)],
    conversations_dir: &Path,
) -> std::collections::HashMap<String, Vec<(ConversationEntry, Option<PathBuf>)>> {
    let has_cclog = cclog_available();
    if !has_cclog {
        tracing::warn!("cclog not on PATH; conversation entries will appear without links");
    }

    let mut converted_sessions: HashSet<String> = HashSet::new();
    // Track session_id -> (forge, first_prompt) for filename generation
    let mut session_first_prompt: std::collections::HashMap<String, (Forge, String)> =
        std::collections::HashMap::new();
    // Track session_id -> output path
    let mut session_output_paths: std::collections::HashMap<String, Option<PathBuf>> =
        std::collections::HashMap::new();

    // First pass: identify first prompt per session for filename generation
    for (entry, forge) in matched {
        session_first_prompt
            .entry(entry.session_id.clone())
            .or_insert_with(|| (forge.clone(), entry.display.clone()));
    }

    // Second pass: convert unique sessions
    for (session_id, (forge, first_prompt)) in &session_first_prompt {
        if converted_sessions.contains(session_id) {
            continue;
        }
        converted_sessions.insert(session_id.clone());

        if !has_cclog {
            session_output_paths.insert(session_id.clone(), None);
            continue;
        }

        // Find any matching entry to get the project path
        let project = matched
            .iter()
            .find(|(e, _)| &e.session_id == session_id)
            .map(|(e, _)| &e.project);

        let project = match project {
            Some(p) => p,
            None => continue,
        };

        let session_path = session_file_path(project, session_id);
        if !session_path.exists() {
            tracing::debug!("session file not found: {:?}", session_path);
            session_output_paths.insert(session_id.clone(), None);
            continue;
        }

        let filename = conversation_filename(forge, first_prompt);
        let output_path = conversations_dir.join(&filename);

        match convert_session(&session_path, &output_path) {
            Ok(true) => {
                session_output_paths.insert(session_id.clone(), Some(output_path));
            }
            _ => {
                session_output_paths.insert(session_id.clone(), None);
            }
        }
    }

    // Build the result grouped by forge display name
    let mut result: std::collections::HashMap<String, Vec<(ConversationEntry, Option<PathBuf>)>> =
        std::collections::HashMap::new();

    for (entry, forge) in matched {
        let key = forge.display_name();
        let md_path = session_output_paths
            .get(&entry.session_id)
            .cloned()
            .flatten();
        result
            .entry(key)
            .or_default()
            .push(((*entry).clone(), md_path));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversation_filename() {
        let forge = Forge::GitHub {
            owner: "numtide".to_string(),
            repo: "system-manager".to_string(),
        };
        let filename = conversation_filename(&forge, "How hard would it be to integrate security.wrappers");
        assert_eq!(
            filename,
            "numtide-system-manager--how-hard-would-it-be-to.md"
        );
    }

    #[test]
    fn test_conversation_filename_long_prompt() {
        let forge = Forge::Gitea {
            host: "git.pyxel.lan".to_string(),
            owner: "jfroche".to_string(),
            repo: "notes".to_string(),
        };
        let prompt = "This is a very long prompt that has many words and should be truncated at six words maximum";
        let filename = conversation_filename(&forge, prompt);
        assert_eq!(
            filename,
            "jfroche-notes--this-is-a-very-long-prompt.md"
        );
    }

    #[test]
    fn test_conversation_filename_special_chars() {
        let forge = Forge::GitHub {
            owner: "org".to_string(),
            repo: "repo".to_string(),
        };
        let filename = conversation_filename(&forge, "Fix the bug! @file.rs #123");
        assert_eq!(filename, "org-repo--fix-the-bug-filers-123.md");
    }

    #[test]
    fn test_session_file_path() {
        let project = Path::new("/home/jfroche/projects/numtide/system-manager");
        let session_id = "abc-123-def";
        let path = session_file_path(project, session_id);
        assert!(path.to_string_lossy().contains("-home-jfroche-projects-numtide-system-manager"));
        assert!(path.to_string_lossy().ends_with("abc-123-def.jsonl"));
    }

    #[test]
    fn test_match_to_repos_longest_prefix() {
        let repos = vec![
            Repository {
                path: PathBuf::from("/home/user/projects/org"),
                forge: Some(Forge::GitHub {
                    owner: "org".to_string(),
                    repo: "parent".to_string(),
                }),
            },
            Repository {
                path: PathBuf::from("/home/user/projects/org/subrepo"),
                forge: Some(Forge::GitHub {
                    owner: "org".to_string(),
                    repo: "subrepo".to_string(),
                }),
            },
        ];

        let entries = vec![ConversationEntry {
            display: "test".to_string(),
            timestamp: Utc::now(),
            project: PathBuf::from("/home/user/projects/org/subrepo/subdir"),
            session_id: "sess-1".to_string(),
        }];

        let matched = match_to_repos(&entries, &repos);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].1.display_name(), "org/subrepo");
    }
}
