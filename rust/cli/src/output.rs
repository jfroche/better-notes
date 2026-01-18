use anyhow::Result;

use crate::forge::Forge;
use crate::git::Commit;
use crate::pr::{CiStatus, PrStatus, PullRequest};
use crate::summary::Summarizer;

/// Format output without LLM summaries
pub fn format_without_summary(groups: &[(Forge, Vec<Commit>, Vec<PullRequest>)]) -> String {
    let mut output = String::new();
    output.push_str("## Git activity\n\n");

    for (forge, commits, prs) in groups {
        if commits.is_empty() && prs.is_empty() {
            continue;
        }

        output.push_str(&format!("### {}\n\n", forge.display_name()));

        // List commits
        for commit in commits {
            let url = forge.commit_url(&commit.short_hash);
            output.push_str(&format!(
                "- [{}]({}) - {}\n",
                commit.short_hash, url, commit.subject
            ));
        }

        // List PRs
        for pr in prs {
            output.push_str(&format_pr(pr));
        }

        output.push('\n');
    }

    output.trim_end().to_string()
}

/// Format output with LLM-generated summaries
pub async fn format_with_summary(
    groups: &[(Forge, Vec<Commit>, Vec<PullRequest>)],
) -> Result<String> {
    let summarizer = match Summarizer::new() {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!("Summarizer not available: {}", e);
            None
        }
    };

    let mut output = String::new();
    output.push_str("## Git activity\n\n");

    for (forge, commits, prs) in groups {
        if commits.is_empty() && prs.is_empty() {
            continue;
        }

        output.push_str(&format!("### {}\n\n", forge.display_name()));

        // Generate summary if available
        if let Some(ref summarizer) = summarizer {
            if !commits.is_empty() {
                match summarizer.summarize(commits).await {
                    Ok(summary) if !summary.is_empty() => {
                        output.push_str(&summary);
                        output.push_str("\n\n");
                    }
                    Err(e) => {
                        tracing::warn!("Failed to generate summary: {}", e);
                    }
                    _ => {}
                }
            }
        }

        // List commits
        for commit in commits {
            let url = forge.commit_url(&commit.short_hash);
            output.push_str(&format!(
                "- [{}]({}) - {}\n",
                commit.short_hash, url, commit.subject
            ));
        }

        // List PRs
        for pr in prs {
            output.push_str(&format_pr(pr));
        }

        output.push('\n');
    }

    Ok(output.trim_end().to_string())
}

fn format_pr(pr: &PullRequest) -> String {
    let mut status_parts = Vec::new();

    // PR status
    status_parts.push(pr.status.to_string());

    // CI status (only for open PRs)
    if pr.status == PrStatus::Open && pr.ci_status != CiStatus::Unknown {
        status_parts.push(format!("CI: {}", pr.ci_status));
    }

    // Conflicts
    if pr.has_conflicts {
        status_parts.push("conflicts".to_string());
    }

    let status_str = status_parts.join(", ");

    format!("- PR [#{}]({}) - {}\n", pr.number, pr.url, status_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_pr_merged() {
        let pr = PullRequest {
            number: 42,
            title: "Test PR".to_string(),
            status: PrStatus::Merged,
            ci_status: CiStatus::Success,
            has_conflicts: false,
            url: "https://github.com/org/repo/pull/42".to_string(),
        };

        let output = format_pr(&pr);
        assert!(output.contains("Merged"));
        assert!(!output.contains("CI:")); // CI not shown for merged PRs
    }

    #[test]
    fn test_format_pr_open_with_conflicts() {
        let pr = PullRequest {
            number: 23,
            title: "WIP".to_string(),
            status: PrStatus::Open,
            ci_status: CiStatus::Pending,
            has_conflicts: true,
            url: "https://github.com/org/repo/pull/23".to_string(),
        };

        let output = format_pr(&pr);
        assert!(output.contains("Open"));
        assert!(output.contains("CI: pending"));
        assert!(output.contains("conflicts"));
    }
}
