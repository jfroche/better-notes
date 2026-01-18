use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use walkdir::WalkDir;

use crate::forge::Forge;

/// A git commit
#[derive(Debug, Clone)]
pub struct Commit {
    pub hash: String,
    pub short_hash: String,
    pub subject: String,
    pub body: Option<String>,
    #[allow(dead_code)]
    pub author: String,
    pub date: DateTime<Utc>,
}

impl Commit {
    /// Get the full commit message (subject + body) for LLM context
    pub fn full_message(&self) -> String {
        match &self.body {
            Some(body) if !body.is_empty() => format!("{}\n\n{}", self.subject, body),
            _ => self.subject.clone(),
        }
    }
}

/// A git repository with its detected forge
#[derive(Debug, Clone)]
pub struct Repository {
    pub path: PathBuf,
    pub forge: Option<Forge>,
}

/// Parse a date string into a DateTime
pub fn parse_date(date_str: &Option<String>) -> Result<DateTime<Utc>> {
    match date_str {
        None => Ok(Utc::now()),
        Some(s) => {
            let s = s.to_lowercase();

            // Handle relative dates
            if s == "today" {
                return Ok(Utc::now());
            }
            if s == "yesterday" {
                return Ok(Utc::now() - Duration::days(1));
            }

            // Handle "N days ago"
            if s.ends_with(" days ago") || s.ends_with(" day ago") {
                let parts: Vec<&str> = s.split_whitespace().collect();
                if let Ok(n) = parts[0].parse::<i64>() {
                    return Ok(Utc::now() - Duration::days(n));
                }
            }

            // Try to parse as YYYY-MM-DD
            if let Ok(date) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
                let datetime = date.and_hms_opt(23, 59, 59).unwrap();
                return Ok(DateTime::from_naive_utc_and_offset(datetime, Utc));
            }

            Err(anyhow!("Unable to parse date: {}", s))
        }
    }
}

/// Discover all git repositories under a directory
pub fn discover_repositories(root: &Path) -> Result<Vec<Repository>> {
    let mut repos = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .max_depth(10)
        .into_iter()
        .filter_entry(|e| !is_hidden(e))
    {
        // Skip entries we can't read (permission denied, etc.)
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!("Skipping inaccessible path: {}", e);
                continue;
            }
        };
        let path = entry.path();

        // Check for .git directory or file (worktree)
        if path.file_name() == Some(std::ffi::OsStr::new(".git")) {
            let repo_path = path.parent().unwrap().to_path_buf();

            // Skip bare repositories
            if is_bare_repository(&repo_path) {
                continue;
            }

            let forge = detect_forge(&repo_path);
            repos.push(Repository {
                path: repo_path,
                forge,
            });
        }
    }

    Ok(repos)
}

fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.') && s != ".git")
        .unwrap_or(false)
}

fn is_bare_repository(path: &Path) -> bool {
    let output = Command::new("git")
        .args(["rev-parse", "--is-bare-repository"])
        .current_dir(path)
        .output();

    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim() == "true",
        Err(_) => false,
    }
}

/// Detect the forge from a repository's remote
fn detect_forge(repo_path: &Path) -> Option<Forge> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Forge::from_remote_url(&url)
}

/// Get commits from a repository within a date range
pub fn get_commits(
    repo: &Repository,
    since: &DateTime<Utc>,
    until: &DateTime<Utc>,
) -> Result<Vec<Commit>> {
    let author = get_git_author()?;

    let since_str = since.format("%Y-%m-%d").to_string();
    let until_str = until.format("%Y-%m-%d").to_string();

    // Use record separator (%x00) between commits and field separator (%x1f) between fields
    // Format: hash<US>short_hash<US>subject<US>body<US>author<US>date<RS>
    let output = Command::new("git")
        .args([
            "log",
            "--all",
            "--no-merges",
            &format!("--author={author}"),
            &format!("--since={since_str}"),
            &format!("--until={until_str} 23:59:59"),
            "--pretty=format:%H%x1f%h%x1f%s%x1f%b%x1f%an%x1f%aI%x00",
        ])
        .current_dir(&repo.path)
        .output()
        .context("Failed to run git log")?;

    if !output.status.success() {
        return Err(anyhow!(
            "git log failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut commits = Vec::new();

    // Split by record separator (null byte)
    for record in stdout.split('\0') {
        if record.trim().is_empty() {
            continue;
        }

        // Split by unit separator
        let parts: Vec<&str> = record.split('\x1f').collect();
        if parts.len() < 6 {
            continue;
        }

        let date = DateTime::parse_from_rfc3339(parts[5].trim())
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        let body = parts[3].trim();
        let body = if body.is_empty() {
            None
        } else {
            Some(body.to_string())
        };

        commits.push(Commit {
            hash: parts[0].to_string(),
            short_hash: parts[1].to_string(),
            subject: parts[2].to_string(),
            body,
            author: parts[4].to_string(),
            date,
        });
    }

    Ok(commits)
}

/// Get the current git user's name
fn get_git_author() -> Result<String> {
    let output = Command::new("git")
        .args(["config", "user.name"])
        .output()
        .context("Failed to get git user.name")?;

    if !output.status.success() {
        return Err(anyhow!("git config user.name failed"));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Deduplicate commits and group by forge
pub fn deduplicate_and_group(
    commits: Vec<(Repository, Commit)>,
    projects_root: &Path,
) -> Vec<(Forge, Vec<Commit>)> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut grouped: HashMap<String, (Forge, Vec<Commit>)> = HashMap::new();

    for (repo, commit) in commits {
        // Skip if we've already seen this commit hash
        if seen.contains(&commit.hash) {
            continue;
        }
        seen.insert(commit.hash.clone());

        // Group by forge display name (or path if no forge)
        let key = repo
            .forge
            .as_ref()
            .map(|f| f.to_string())
            .unwrap_or_else(|| {
                repo.path
                    .strip_prefix(projects_root)
                    .unwrap_or(&repo.path)
                    .to_string_lossy()
                    .to_string()
            });

        let forge = repo.forge.clone().unwrap_or_else(|| Forge::Gitea {
            host: "unknown".to_string(),
            owner: "unknown".to_string(),
            repo: repo.path.file_name().unwrap().to_string_lossy().to_string(),
        });

        grouped
            .entry(key)
            .or_insert_with(|| (forge, Vec::new()))
            .1
            .push(commit);
    }

    // Sort commits by date within each group
    let mut result: Vec<_> = grouped.into_values().collect();
    for (_, commits) in &mut result {
        commits.sort_by(|a, b| b.date.cmp(&a.date));
    }

    // Sort groups by forge name
    result.sort_by(|a, b| a.0.to_string().cmp(&b.0.to_string()));

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_date_yesterday() {
        let result = parse_date(&Some("yesterday".to_string())).unwrap();
        let expected = Utc::now() - Duration::days(1);
        // Check that dates are within a second of each other
        assert!((result - expected).num_seconds().abs() < 2);
    }

    #[test]
    fn test_parse_date_iso() {
        let result = parse_date(&Some("2024-01-15".to_string())).unwrap();
        assert_eq!(result.format("%Y-%m-%d").to_string(), "2024-01-15");
    }

    #[test]
    fn test_parse_date_days_ago() {
        let result = parse_date(&Some("3 days ago".to_string())).unwrap();
        let expected = Utc::now() - Duration::days(3);
        assert!((result - expected).num_seconds().abs() < 2);
    }
}
