use anyhow::{Context, Result};
use serde::Deserialize;

use crate::forge::Forge;
use crate::git::Commit;

/// Read GitHub token from gh CLI config or environment
fn get_github_token() -> Option<String> {
    // First check environment variable
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.is_empty() {
            return Some(token);
        }
    }

    // Fall back to gh CLI config
    #[derive(Deserialize)]
    struct GhHost {
        oauth_token: Option<String>,
    }

    let config_path = dirs::config_dir()?.join("gh/hosts.yml");
    let content = std::fs::read_to_string(&config_path).ok()?;
    let hosts: std::collections::HashMap<String, GhHost> = serde_yaml::from_str(&content).ok()?;

    hosts.get("github.com").and_then(|h| h.oauth_token.clone())
}

/// Read GitLab token from glab CLI config or environment
fn get_gitlab_token(host: &str) -> Option<String> {
    // First check environment variable
    if let Ok(token) = std::env::var("GITLAB_TOKEN") {
        if !token.is_empty() {
            return Some(token);
        }
    }

    // Fall back to glab CLI config
    #[derive(Deserialize)]
    struct GlabConfig {
        hosts: std::collections::HashMap<String, GlabHost>,
    }

    #[derive(Deserialize)]
    struct GlabHost {
        token: Option<String>,
    }

    let config_path = dirs::config_dir()?.join("glab-cli/config.yml");
    let content = std::fs::read_to_string(&config_path).ok()?;
    let config: GlabConfig = serde_yaml::from_str(&content).ok()?;

    config.hosts.get(host).and_then(|h| h.token.clone())
}

/// Read Gitea token from tea config file
fn get_gitea_token(host: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct TeaConfig {
        logins: Vec<TeaLogin>,
    }

    #[derive(Deserialize)]
    struct TeaLogin {
        url: String,
        token: String,
    }

    let config_path = dirs::config_dir()?.join("tea/config.yml");
    let content = std::fs::read_to_string(&config_path).ok()?;
    let config: TeaConfig = serde_yaml::from_str(&content).ok()?;

    config
        .logins
        .iter()
        .find(|login| login.url.contains(host))
        .map(|login| login.token.clone())
}

/// Pull request / merge request status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrStatus {
    Open,
    Merged,
    Closed,
}

/// CI status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiStatus {
    Pending,
    Success,
    Failure,
    Unknown,
}

/// A pull request with its status
#[derive(Debug, Clone)]
pub struct PullRequest {
    pub number: u32,
    pub title: String,
    pub description: Option<String>,
    pub status: PrStatus,
    pub ci_status: CiStatus,
    pub has_conflicts: bool,
    pub url: String,
    /// Commit hashes associated with this PR
    pub commit_hashes: Vec<String>,
    /// Last updated date
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl std::fmt::Display for PrStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrStatus::Open => write!(f, "Open"),
            PrStatus::Merged => write!(f, "Merged"),
            PrStatus::Closed => write!(f, "Closed"),
        }
    }
}

impl std::fmt::Display for CiStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CiStatus::Pending => write!(f, "pending"),
            CiStatus::Success => write!(f, "success"),
            CiStatus::Failure => write!(f, "failure"),
            CiStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// Fetch PRs associated with the given commits
pub async fn fetch_prs_for_commits(forge: &Forge, _commits: &[Commit]) -> Result<Vec<PullRequest>> {
    // Get branches that contain the commits
    // For now, we'll fetch all open PRs for the repo and match by branch
    match forge {
        Forge::GitHub { owner, repo } => fetch_github_prs(owner, repo).await,
        Forge::Gitea { host, owner, repo } => fetch_gitea_prs(host, owner, repo).await,
        Forge::GitLab { host, owner, repo } => fetch_gitlab_prs(host, owner, repo).await,
    }
}

async fn fetch_github_prs(owner: &str, repo: &str) -> Result<Vec<PullRequest>> {
    let token = get_github_token();
    if token.is_none() {
        tracing::warn!("No GitHub token found, skipping GitHub PR fetch");
        return Ok(Vec::new());
    }
    let token = token.unwrap();

    // Get current user login
    let username = get_github_username(&token).await?;

    #[derive(Deserialize)]
    struct GhPr {
        number: u32,
        title: String,
        body: Option<String>,
        state: String,
        merged: Option<bool>,
        mergeable: Option<bool>,
        html_url: String,
        head: GhHead,
        updated_at: Option<String>,
    }

    #[derive(Deserialize)]
    struct GhHead {
        sha: String,
    }

    let url = format!(
        "https://api.github.com/repos/{owner}/{repo}/pulls?state=all&per_page=20&sort=updated&direction=desc"
    );

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "better-notes")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await;

    let all_prs: Vec<GhPr> = match response {
        Ok(resp) if resp.status().is_success() => resp.json().await.unwrap_or_default(),
        Ok(resp) => {
            tracing::warn!("GitHub API error: {}", resp.status());
            return Ok(Vec::new());
        }
        Err(e) => {
            tracing::warn!("GitHub API request failed: {}", e);
            return Ok(Vec::new());
        }
    };

    // Filter to only user's PRs
    let mut result = Vec::new();
    for pr in all_prs {
        // Fetch PR details to check author
        let pr_author = get_github_pr_author(&token, owner, repo, pr.number).await;
        if pr_author.as_deref() != Some(username.as_str()) {
            continue;
        }

        let status = if pr.merged == Some(true) {
            PrStatus::Merged
        } else {
            match pr.state.as_str() {
                "closed" => PrStatus::Closed,
                _ => PrStatus::Open,
            }
        };

        // Fetch CI status for open PRs
        let ci_status = if status == PrStatus::Open {
            fetch_github_ci_status(&token, owner, repo, &pr.head.sha).await
        } else {
            CiStatus::Unknown
        };

        let has_conflicts = pr.mergeable == Some(false);

        // Fetch commits for this PR
        let commit_hashes = fetch_github_pr_commits(&token, owner, repo, pr.number).await;

        let updated_at = pr
            .updated_at
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|d| d.with_timezone(&chrono::Utc));

        result.push(PullRequest {
            number: pr.number,
            title: pr.title,
            description: pr.body,
            status,
            ci_status,
            has_conflicts,
            url: pr.html_url,
            commit_hashes,
            updated_at,
        });
    }

    Ok(result)
}

/// Get GitHub username from token
async fn get_github_username(token: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct GhUser {
        login: String,
    }

    let client = reqwest::Client::new();
    let response = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "better-notes")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("Failed to fetch GitHub user")?;

    let user: GhUser = response
        .json()
        .await
        .context("Failed to parse GitHub user")?;
    Ok(user.login)
}

/// Get PR author from GitHub API
async fn get_github_pr_author(
    token: &str,
    owner: &str,
    repo: &str,
    pr_number: u32,
) -> Option<String> {
    #[derive(Deserialize)]
    struct GhPrDetail {
        user: GhUser,
    }

    #[derive(Deserialize)]
    struct GhUser {
        login: String,
    }

    let url = format!("https://api.github.com/repos/{owner}/{repo}/pulls/{pr_number}");
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "better-notes")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .ok()?;

    let pr: GhPrDetail = response.json().await.ok()?;
    Some(pr.user.login)
}

/// Fetch CI status for a GitHub commit
async fn fetch_github_ci_status(token: &str, owner: &str, repo: &str, sha: &str) -> CiStatus {
    #[derive(Deserialize)]
    struct CombinedStatus {
        state: String,
    }

    let url = format!("https://api.github.com/repos/{owner}/{repo}/commits/{sha}/status");
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "better-notes")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => match resp.json::<CombinedStatus>().await {
            Ok(status) => match status.state.as_str() {
                "success" => CiStatus::Success,
                "pending" => CiStatus::Pending,
                "failure" | "error" => CiStatus::Failure,
                _ => CiStatus::Unknown,
            },
            Err(_) => CiStatus::Unknown,
        },
        _ => CiStatus::Unknown,
    }
}

/// Fetch commits for a GitHub PR
async fn fetch_github_pr_commits(
    token: &str,
    owner: &str,
    repo: &str,
    pr_number: u32,
) -> Vec<String> {
    #[derive(Deserialize)]
    struct GhCommit {
        sha: String,
    }

    let url = format!(
        "https://api.github.com/repos/{owner}/{repo}/pulls/{pr_number}/commits?per_page=100"
    );
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "better-notes")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => resp
            .json::<Vec<GhCommit>>()
            .await
            .map(|commits| commits.into_iter().map(|c| c.sha).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

async fn fetch_gitea_prs(host: &str, owner: &str, repo: &str) -> Result<Vec<PullRequest>> {
    #[derive(Deserialize)]
    struct GiteaPr {
        number: u32,
        title: String,
        body: Option<String>,
        state: String,
        merged: Option<bool>,
        mergeable: Option<bool>,
        updated_at: Option<String>,
    }

    let scheme = "https";
    let url = format!("{scheme}://{host}/api/v1/repos/{owner}/{repo}/pulls?state=all&limit=20");

    let client = reqwest::Client::new();
    let mut request = client.get(&url);

    // Add authentication if token is available
    if let Some(token) = get_gitea_token(host) {
        request = request.header("Authorization", format!("token {token}"));
    }

    let response = request.send().await;

    let prs: Vec<GiteaPr> = match response {
        Ok(resp) if resp.status().is_success() => resp.json().await.unwrap_or_default(),
        _ => {
            tracing::warn!("Failed to fetch Gitea PRs from {url}");
            return Ok(Vec::new());
        }
    };

    let mut result = Vec::new();
    for pr in prs {
        let status = if pr.merged == Some(true) {
            PrStatus::Merged
        } else {
            match pr.state.as_str() {
                "closed" => PrStatus::Closed,
                _ => PrStatus::Open,
            }
        };

        // Fetch CI status for open PRs
        let ci_status = if status == PrStatus::Open {
            fetch_gitea_ci_status(host, owner, repo, pr.number).await
        } else {
            CiStatus::Unknown
        };

        // Fetch commits for this PR
        let commit_hashes = fetch_gitea_pr_commits(host, owner, repo, pr.number).await;

        let updated_at = pr
            .updated_at
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|d| d.with_timezone(&chrono::Utc));

        result.push(PullRequest {
            number: pr.number,
            title: pr.title,
            description: pr.body,
            status,
            ci_status,
            has_conflicts: pr.mergeable == Some(false),
            url: format!("{scheme}://{host}/{owner}/{repo}/pulls/{}", pr.number),
            commit_hashes,
            updated_at,
        });
    }

    Ok(result)
}

async fn fetch_gitea_pr_commits(
    host: &str,
    owner: &str,
    repo: &str,
    pr_number: u32,
) -> Vec<String> {
    #[derive(Deserialize)]
    struct GiteaCommit {
        sha: String,
    }

    let scheme = "https";
    let url = format!("{scheme}://{host}/api/v1/repos/{owner}/{repo}/pulls/{pr_number}/commits");

    let client = reqwest::Client::new();
    let mut request = client.get(&url);

    if let Some(token) = get_gitea_token(host) {
        request = request.header("Authorization", format!("token {token}"));
    }

    match request.send().await {
        Ok(resp) if resp.status().is_success() => resp
            .json::<Vec<GiteaCommit>>()
            .await
            .map(|commits| commits.into_iter().map(|c| c.sha).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

async fn fetch_gitea_ci_status(host: &str, owner: &str, repo: &str, pr_number: u32) -> CiStatus {
    #[derive(Deserialize)]
    struct CombinedStatus {
        state: String,
    }

    let scheme = "https";
    let token = get_gitea_token(host);

    // First get the PR to find the head commit
    let pr_url = format!("{scheme}://{host}/api/v1/repos/{owner}/{repo}/pulls/{pr_number}");
    let client = reqwest::Client::new();

    #[derive(Deserialize)]
    struct PrHead {
        head: Option<PrRef>,
    }

    #[derive(Deserialize)]
    struct PrRef {
        sha: Option<String>,
    }

    let mut request = client.get(&pr_url);
    if let Some(ref token) = token {
        request = request.header("Authorization", format!("token {token}"));
    }

    let sha = match request.send().await {
        Ok(resp) if resp.status().is_success() => resp
            .json::<PrHead>()
            .await
            .ok()
            .and_then(|pr| pr.head)
            .and_then(|h| h.sha),
        _ => return CiStatus::Unknown,
    };

    let Some(sha) = sha else {
        return CiStatus::Unknown;
    };

    // Fetch combined status for the commit
    let status_url = format!("{scheme}://{host}/api/v1/repos/{owner}/{repo}/commits/{sha}/status");
    let mut request = client.get(&status_url);
    if let Some(ref token) = token {
        request = request.header("Authorization", format!("token {token}"));
    }
    match request.send().await {
        Ok(resp) if resp.status().is_success() => match resp.json::<CombinedStatus>().await {
            Ok(status) => match status.state.as_str() {
                "success" => CiStatus::Success,
                "pending" => CiStatus::Pending,
                "failure" | "error" => CiStatus::Failure,
                _ => CiStatus::Unknown,
            },
            Err(_) => CiStatus::Unknown,
        },
        _ => CiStatus::Unknown,
    }
}

async fn fetch_gitlab_prs(host: &str, owner: &str, repo: &str) -> Result<Vec<PullRequest>> {
    let token = get_gitlab_token(host);
    if token.is_none() {
        tracing::warn!("No GitLab token found for {host}, skipping GitLab MR fetch");
        return Ok(Vec::new());
    }
    let token = token.unwrap();

    // Get current user ID
    let username = get_gitlab_username(host, &token).await?;

    #[derive(Deserialize)]
    struct GitLabMr {
        iid: u32,
        title: String,
        description: Option<String>,
        state: String,
        has_conflicts: Option<bool>,
        detailed_merge_status: Option<String>,
        sha: Option<String>,
        author: GitLabUser,
        updated_at: Option<String>,
    }

    #[derive(Deserialize)]
    struct GitLabUser {
        username: String,
    }

    // URL-encode the project path
    let project_path = format!("{owner}/{repo}").replace('/', "%2F");
    let url = format!(
        "https://{host}/api/v4/projects/{project_path}/merge_requests?state=all&per_page=20&order_by=updated_at"
    );

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("PRIVATE-TOKEN", &token)
        .header("User-Agent", "better-notes")
        .send()
        .await;

    let all_mrs: Vec<GitLabMr> = match response {
        Ok(resp) if resp.status().is_success() => resp.json().await.unwrap_or_default(),
        Ok(resp) => {
            tracing::warn!("GitLab API error: {}", resp.status());
            return Ok(Vec::new());
        }
        Err(e) => {
            tracing::warn!("GitLab API request failed: {}", e);
            return Ok(Vec::new());
        }
    };

    // Filter to only user's MRs
    let mut result = Vec::new();
    for mr in all_mrs {
        if mr.author.username != username {
            continue;
        }

        let status = match mr.state.as_str() {
            "merged" => PrStatus::Merged,
            "closed" => PrStatus::Closed,
            _ => PrStatus::Open,
        };

        // CI status from detailed_merge_status
        let ci_status = match mr.detailed_merge_status.as_deref() {
            Some("ci_still_running") => CiStatus::Pending,
            Some("mergeable") => CiStatus::Success,
            Some("ci_must_pass") => CiStatus::Failure,
            _ => CiStatus::Unknown,
        };

        // Fetch commits for this MR
        let commit_hashes = fetch_gitlab_mr_commits(host, &token, owner, repo, mr.iid).await;

        let updated_at = mr
            .updated_at
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|d| d.with_timezone(&chrono::Utc));

        result.push(PullRequest {
            number: mr.iid,
            title: mr.title,
            description: mr.description,
            status,
            ci_status,
            has_conflicts: mr.has_conflicts.unwrap_or(false),
            url: format!("https://{host}/{owner}/{repo}/-/merge_requests/{}", mr.iid),
            commit_hashes,
            updated_at,
        });
    }

    Ok(result)
}

/// Get GitLab username from token
async fn get_gitlab_username(host: &str, token: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct GitLabUser {
        username: String,
    }

    let client = reqwest::Client::new();
    let response = client
        .get(format!("https://{host}/api/v4/user"))
        .header("PRIVATE-TOKEN", token)
        .header("User-Agent", "better-notes")
        .send()
        .await
        .context("Failed to fetch GitLab user")?;

    let user: GitLabUser = response
        .json()
        .await
        .context("Failed to parse GitLab user")?;
    Ok(user.username)
}

/// Fetch commits for a GitLab MR
async fn fetch_gitlab_mr_commits(
    host: &str,
    token: &str,
    owner: &str,
    repo: &str,
    mr_iid: u32,
) -> Vec<String> {
    #[derive(Deserialize)]
    struct GitLabCommit {
        id: String,
    }

    let project_path = format!("{owner}/{repo}").replace('/', "%2F");
    let url = format!(
        "https://{host}/api/v4/projects/{project_path}/merge_requests/{mr_iid}/commits?per_page=100"
    );

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("PRIVATE-TOKEN", token)
        .header("User-Agent", "better-notes")
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => resp
            .json::<Vec<GitLabCommit>>()
            .await
            .map(|commits| commits.into_iter().map(|c| c.id).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}
