use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::forge::Forge;
use crate::git::Commit;

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
    #[allow(dead_code)]
    pub title: String,
    pub status: PrStatus,
    pub ci_status: CiStatus,
    pub has_conflicts: bool,
    pub url: String,
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
    #[derive(Deserialize)]
    struct GhPr {
        number: u32,
        title: String,
        state: String,
        #[serde(rename = "mergeStateStatus")]
        merge_state_status: Option<String>,
        #[serde(rename = "statusCheckRollup")]
        status_check_rollup: Option<Vec<GhCheck>>,
    }

    #[derive(Deserialize)]
    struct GhCheck {
        conclusion: Option<String>,
        status: Option<String>,
    }

    let output = Command::new("gh")
        .args([
            "pr",
            "list",
            "--repo",
            &format!("{owner}/{repo}"),
            "--author",
            "@me",
            "--state",
            "all",
            "--limit",
            "20",
            "--json",
            "number,title,state,mergeStateStatus,statusCheckRollup",
        ])
        .output()
        .context("Failed to run gh pr list")?;

    if !output.status.success() {
        tracing::warn!(
            "gh pr list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(Vec::new());
    }

    let prs: Vec<GhPr> = serde_json::from_slice(&output.stdout).unwrap_or_default();

    Ok(prs
        .into_iter()
        .map(|pr| {
            let status = match pr.state.as_str() {
                "MERGED" => PrStatus::Merged,
                "CLOSED" => PrStatus::Closed,
                _ => PrStatus::Open,
            };

            let ci_status = pr
                .status_check_rollup
                .as_ref()
                .and_then(|checks| {
                    if checks
                        .iter()
                        .any(|c| c.conclusion.as_deref() == Some("FAILURE"))
                    {
                        Some(CiStatus::Failure)
                    } else if checks
                        .iter()
                        .all(|c| c.conclusion.as_deref() == Some("SUCCESS"))
                    {
                        Some(CiStatus::Success)
                    } else if checks
                        .iter()
                        .any(|c| c.status.as_deref() == Some("IN_PROGRESS"))
                    {
                        Some(CiStatus::Pending)
                    } else {
                        None
                    }
                })
                .unwrap_or(CiStatus::Unknown);

            let has_conflicts = pr.merge_state_status.as_deref() == Some("CONFLICTING");

            PullRequest {
                number: pr.number,
                title: pr.title,
                status,
                ci_status,
                has_conflicts,
                url: format!("https://github.com/{owner}/{repo}/pull/{}", pr.number),
            }
        })
        .collect())
}

async fn fetch_gitea_prs(host: &str, owner: &str, repo: &str) -> Result<Vec<PullRequest>> {
    #[derive(Deserialize)]
    struct GiteaPr {
        number: u32,
        title: String,
        state: String,
        merged: Option<bool>,
        mergeable: Option<bool>,
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

        result.push(PullRequest {
            number: pr.number,
            title: pr.title,
            status,
            ci_status,
            has_conflicts: pr.mergeable == Some(false),
            url: format!("{scheme}://{host}/{owner}/{repo}/pulls/{}", pr.number),
        });
    }

    Ok(result)
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
    // Use glab CLI for GitLab
    let output = Command::new("glab")
        .args([
            "mr",
            "list",
            "--repo",
            &format!("{owner}/{repo}"),
            "--author",
            "@me",
            "--all",
            "-F",
            "json",
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            #[derive(Deserialize)]
            struct GlabMr {
                iid: u32,
                title: String,
                state: String,
                has_conflicts: Option<bool>,
                #[serde(rename = "detailed_merge_status")]
                detailed_merge_status: Option<String>,
            }

            let mrs: Vec<GlabMr> = serde_json::from_slice(&o.stdout).unwrap_or_default();

            Ok(mrs
                .into_iter()
                .map(|mr| {
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

                    PullRequest {
                        number: mr.iid,
                        title: mr.title,
                        status,
                        ci_status,
                        has_conflicts: mr.has_conflicts.unwrap_or(false),
                        url: format!("https://{host}/{owner}/{repo}/-/merge_requests/{}", mr.iid),
                    }
                })
                .collect())
        }
        _ => {
            tracing::warn!("glab mr list failed");
            Ok(Vec::new())
        }
    }
}
