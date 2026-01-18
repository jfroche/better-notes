use std::fmt;

use regex::Regex;
use url::Url;

/// Represents a git forge (hosting platform)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Forge {
    GitHub {
        owner: String,
        repo: String,
    },
    Gitea {
        host: String,
        owner: String,
        repo: String,
    },
    GitLab {
        host: String,
        owner: String,
        repo: String,
    },
}

impl Forge {
    /// Parse a git remote URL and detect the forge type
    pub fn from_remote_url(url: &str) -> Option<Self> {
        // Handle SSH URLs: git@host:owner/repo.git
        if let Some(forge) = Self::parse_ssh_url(url) {
            return Some(forge);
        }

        // Handle HTTPS URLs: https://host/owner/repo.git
        if let Some(forge) = Self::parse_https_url(url) {
            return Some(forge);
        }

        None
    }

    fn parse_ssh_url(url: &str) -> Option<Self> {
        // Pattern: git@host:owner/repo.git or ssh://git@host/owner/repo.git
        let ssh_regex =
            Regex::new(r"^(?:ssh://)?(?:git@|gitea@)([^:/]+)[:/]([^/]+)/([^/]+?)(?:\.git)?$")
                .ok()?;

        let caps = ssh_regex.captures(url)?;
        let host = caps.get(1)?.as_str();
        let owner = caps.get(2)?.as_str().to_string();
        let repo = caps.get(3)?.as_str().to_string();

        Some(Self::from_host(host, owner, repo))
    }

    fn parse_https_url(url: &str) -> Option<Self> {
        let parsed = Url::parse(url).ok()?;
        let host = parsed.host_str()?;
        let path = parsed
            .path()
            .trim_start_matches('/')
            .trim_end_matches(".git");
        let parts: Vec<&str> = path.split('/').collect();

        if parts.len() >= 2 {
            let owner = parts[0].to_string();
            let repo = parts[1].to_string();
            Some(Self::from_host(host, owner, repo))
        } else {
            None
        }
    }

    fn from_host(host: &str, owner: String, repo: String) -> Self {
        match host {
            "github.com" => Forge::GitHub { owner, repo },
            "gitlab.com" => Forge::GitLab {
                host: "gitlab.com".to_string(),
                owner,
                repo,
            },
            h if h.starts_with("gitlab.") => Forge::GitLab {
                host: h.to_string(),
                owner,
                repo,
            },
            // Known Gitea instances
            "git.pyxel.lan" | "git.affinitic.be" | "gitea.com" => Forge::Gitea {
                host: host.to_string(),
                owner,
                repo,
            },
            h if h.contains("gitea") => Forge::Gitea {
                host: h.to_string(),
                owner,
                repo,
            },
            // Default to Gitea for unknown self-hosted instances
            _ => Forge::Gitea {
                host: host.to_string(),
                owner,
                repo,
            },
        }
    }

    /// Generate URL for a commit
    pub fn commit_url(&self, hash: &str) -> String {
        match self {
            Forge::GitHub { owner, repo } => {
                format!("https://github.com/{owner}/{repo}/commit/{hash}")
            }
            Forge::Gitea { host, owner, repo } => {
                let scheme = if host.ends_with(".lan") {
                    "http"
                } else {
                    "https"
                };
                format!("{scheme}://{host}/{owner}/{repo}/commit/{hash}")
            }
            Forge::GitLab { host, owner, repo } => {
                format!("https://{host}/{owner}/{repo}/commit/{hash}")
            }
        }
    }

    /// Generate URL for a pull request / merge request
    pub fn pr_url(&self, number: u32) -> String {
        match self {
            Forge::GitHub { owner, repo } => {
                format!("https://github.com/{owner}/{repo}/pull/{number}")
            }
            Forge::Gitea { host, owner, repo } => {
                let scheme = if host.ends_with(".lan") {
                    "http"
                } else {
                    "https"
                };
                format!("{scheme}://{host}/{owner}/{repo}/pulls/{number}")
            }
            Forge::GitLab { host, owner, repo } => {
                format!("https://{host}/{owner}/{repo}/-/merge_requests/{number}")
            }
        }
    }

    /// Get the base API URL for this forge
    pub fn api_base_url(&self) -> String {
        match self {
            Forge::GitHub { .. } => "https://api.github.com".to_string(),
            Forge::Gitea { host, .. } => {
                let scheme = if host.ends_with(".lan") {
                    "http"
                } else {
                    "https"
                };
                format!("{scheme}://{host}/api/v1")
            }
            Forge::GitLab { host, .. } => {
                format!("https://{host}/api/v4")
            }
        }
    }

    /// Get owner and repo as a tuple
    pub fn owner_repo(&self) -> (&str, &str) {
        match self {
            Forge::GitHub { owner, repo }
            | Forge::Gitea { owner, repo, .. }
            | Forge::GitLab { owner, repo, .. } => (owner, repo),
        }
    }

    /// Get a display name for grouping in output
    pub fn display_name(&self) -> String {
        let (owner, repo) = self.owner_repo();
        format!("{owner}/{repo}")
    }
}

impl fmt::Display for Forge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Forge::GitHub { owner, repo } => write!(f, "github.com/{owner}/{repo}"),
            Forge::Gitea { host, owner, repo } => write!(f, "{host}/{owner}/{repo}"),
            Forge::GitLab { host, owner, repo } => write!(f, "{host}/{owner}/{repo}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_ssh() {
        let forge = Forge::from_remote_url("git@github.com:numtide/nix-fleet.git").unwrap();
        assert!(
            matches!(forge, Forge::GitHub { owner, repo } if owner == "numtide" && repo == "nix-fleet")
        );
    }

    #[test]
    fn test_parse_github_https() {
        let forge = Forge::from_remote_url("https://github.com/numtide/nix-fleet.git").unwrap();
        assert!(
            matches!(forge, Forge::GitHub { owner, repo } if owner == "numtide" && repo == "nix-fleet")
        );
    }

    #[test]
    fn test_parse_gitea_ssh() {
        let forge = Forge::from_remote_url("git@git.pyxel.lan:jfroche/project.git").unwrap();
        assert!(
            matches!(forge, Forge::Gitea { host, owner, repo } if host == "git.pyxel.lan" && owner == "jfroche" && repo == "project")
        );
    }

    #[test]
    fn test_commit_url_github() {
        let forge = Forge::GitHub {
            owner: "numtide".to_string(),
            repo: "nix-fleet".to_string(),
        };
        assert_eq!(
            forge.commit_url("abc123"),
            "https://github.com/numtide/nix-fleet/commit/abc123"
        );
    }

    #[test]
    fn test_pr_url_gitlab() {
        let forge = Forge::GitLab {
            host: "gitlab.com".to_string(),
            owner: "org".to_string(),
            repo: "project".to_string(),
        };
        assert_eq!(
            forge.pr_url(42),
            "https://gitlab.com/org/project/-/merge_requests/42"
        );
    }
}
