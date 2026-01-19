use std::collections::{BTreeMap, HashSet};

use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};

use crate::forge::Forge;
use crate::git::Commit;
use crate::pr::{CiStatus, PrStatus, PullRequest};
use crate::summary::Summarizer;

/// Collect all commit hashes associated with PRs
fn collect_pr_commit_hashes(prs: &[PullRequest]) -> HashSet<&str> {
    prs.iter()
        .flat_map(|pr| pr.commit_hashes.iter().map(|h| h.as_str()))
        .collect()
}

/// Item that can be displayed (either a commit or a PR)
enum DisplayItem<'a> {
    Commit(&'a Commit),
    Pr(&'a PullRequest),
}

impl<'a> DisplayItem<'a> {
    fn date(&self) -> Option<DateTime<Utc>> {
        match self {
            DisplayItem::Commit(c) => Some(c.date),
            DisplayItem::Pr(pr) => pr.updated_at,
        }
    }
}

/// Group items by date
fn group_by_date<'a>(
    commits: &'a [Commit],
    prs: &'a [PullRequest],
    pr_commits: &HashSet<&str>,
) -> BTreeMap<NaiveDate, Vec<DisplayItem<'a>>> {
    let mut by_date: BTreeMap<NaiveDate, Vec<DisplayItem<'a>>> = BTreeMap::new();

    // Add commits not associated with PRs
    for commit in commits {
        if pr_commits.contains(commit.hash.as_str()) {
            continue;
        }
        let date = commit.date.date_naive();
        by_date
            .entry(date)
            .or_default()
            .push(DisplayItem::Commit(commit));
    }

    // Add PRs
    for pr in prs {
        let date = pr
            .updated_at
            .map(|d| d.date_naive())
            .unwrap_or_else(|| Utc::now().date_naive());
        by_date.entry(date).or_default().push(DisplayItem::Pr(pr));
    }

    by_date
}

/// Format output without LLM summaries
pub fn format_without_summary(groups: &[(Forge, Vec<Commit>, Vec<PullRequest>)]) -> String {
    let mut output = String::new();
    output.push_str("## Git activity\n\n");

    for (forge, commits, prs) in groups {
        if commits.is_empty() && prs.is_empty() {
            continue;
        }

        output.push_str(&format!("### {}\n\n", forge.display_name()));

        // Collect commits that are part of PRs to filter them out
        let pr_commits = collect_pr_commit_hashes(prs);

        // Group by date (reverse order - most recent first)
        let by_date = group_by_date(commits, prs, &pr_commits);
        for (date, items) in by_date.into_iter().rev() {
            output.push_str(&format!("#### {}\n\n", date.format("%Y-%m-%d (%A)")));

            for item in items {
                match item {
                    DisplayItem::Commit(commit) => {
                        let url = forge.commit_url(&commit.hash);
                        output.push_str(&format!(
                            "- [{}]({}) - {}\n",
                            commit.short_hash, url, commit.subject
                        ));
                    }
                    DisplayItem::Pr(pr) => {
                        output.push_str(&format_pr(pr));
                    }
                }
            }

            output.push('\n');
        }
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

        // Collect commits that are part of PRs to filter them out from listing
        let pr_commits = collect_pr_commit_hashes(prs);

        // Generate summary from all commits and PRs
        if let Some(ref summarizer) = summarizer {
            if !commits.is_empty() || !prs.is_empty() {
                match summarizer.summarize(commits, prs).await {
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

        // Group by date (reverse order - most recent first)
        let by_date = group_by_date(commits, prs, &pr_commits);
        for (date, items) in by_date.into_iter().rev() {
            output.push_str(&format!("#### {}\n\n", date.format("%Y-%m-%d (%A)")));

            for item in items {
                match item {
                    DisplayItem::Commit(commit) => {
                        let url = forge.commit_url(&commit.hash);
                        output.push_str(&format!(
                            "- [{}]({}) - {}\n",
                            commit.short_hash, url, commit.subject
                        ));
                    }
                    DisplayItem::Pr(pr) => {
                        output.push_str(&format_pr(pr));
                    }
                }
            }

            output.push('\n');
        }
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

    format!(
        "- PR [#{}]({}) {} - {}\n",
        pr.number, pr.url, pr.title, status_str
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_pr_merged() {
        let pr = PullRequest {
            number: 42,
            title: "Test PR".to_string(),
            description: Some("Test description".to_string()),
            status: PrStatus::Merged,
            ci_status: CiStatus::Success,
            has_conflicts: false,
            url: "https://github.com/org/repo/pull/42".to_string(),
            commit_hashes: vec!["abc123".to_string()],
            updated_at: None,
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
            description: None,
            status: PrStatus::Open,
            ci_status: CiStatus::Pending,
            has_conflicts: true,
            url: "https://github.com/org/repo/pull/23".to_string(),
            commit_hashes: vec![],
            updated_at: None,
        };

        let output = format_pr(&pr);
        assert!(output.contains("Open"));
        assert!(output.contains("CI: pending"));
        assert!(output.contains("conflicts"));
    }
}
