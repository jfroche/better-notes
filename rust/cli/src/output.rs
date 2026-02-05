use std::collections::{BTreeMap, HashSet};

use anyhow::Result;
use chrono::{DateTime, NaiveDate, Timelike, Utc};

use crate::forge::Forge;
use crate::git::Commit;
use crate::pr::{CiStatus, PrStatus, PullRequest};
use crate::summary::Summarizer;

/// Time grouping key - either a date or an hour within a day
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum TimeGroup {
    Date(NaiveDate),
    Hour(NaiveDate, u32),
}

impl TimeGroup {
    fn format(&self) -> String {
        match self {
            TimeGroup::Date(date) => format!("{}", date.format("%Y-%m-%d (%A)")),
            TimeGroup::Hour(date, hour) => format!("{} {:02}h", date.format("%Y-%m-%d"), hour),
        }
    }
}

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


/// Group items by time (date or hour depending on single_day flag)
/// The late_night_offset shifts the day boundary - commits before that hour count as previous day.
fn group_by_time<'a>(
    commits: &'a [Commit],
    prs: &'a [PullRequest],
    pr_commits: &HashSet<&str>,
    single_day: bool,
    late_night_offset: u32,
) -> BTreeMap<TimeGroup, Vec<DisplayItem<'a>>> {
    let mut by_time: BTreeMap<TimeGroup, Vec<DisplayItem<'a>>> = BTreeMap::new();

    let make_key = |dt: DateTime<Utc>| -> TimeGroup {
        // Shift the date back if the hour is before the offset (late-night work)
        let logical_date = if dt.hour() < late_night_offset {
            dt.date_naive() - chrono::Duration::days(1)
        } else {
            dt.date_naive()
        };

        if single_day {
            TimeGroup::Hour(logical_date, dt.hour())
        } else {
            TimeGroup::Date(logical_date)
        }
    };

    // Add commits not associated with PRs
    for commit in commits {
        if pr_commits.contains(commit.hash.as_str()) {
            continue;
        }
        let key = make_key(commit.date);
        by_time
            .entry(key)
            .or_default()
            .push(DisplayItem::Commit(commit));
    }

    // Add PRs
    for pr in prs {
        let dt = pr.updated_at.unwrap_or_else(Utc::now);
        let key = make_key(dt);
        by_time.entry(key).or_default().push(DisplayItem::Pr(pr));
    }

    by_time
}

/// Format output without LLM summaries
pub fn format_without_summary(
    groups: &[(Forge, Vec<Commit>, Vec<PullRequest>)],
    single_day: bool,
    late_night_offset: u32,
) -> String {
    let mut output = String::new();
    output.push_str("## Git activity\n\n");

    for (forge, commits, prs) in groups {
        if commits.is_empty() && prs.is_empty() {
            continue;
        }

        output.push_str(&format!("### {}\n\n", forge.display_name()));

        // Collect commits that are part of PRs to filter them out
        let pr_commits = collect_pr_commit_hashes(prs);

        // Group by time (reverse order - most recent first)
        let by_time = group_by_time(commits, prs, &pr_commits, single_day, late_night_offset);
        for (time_group, items) in by_time.into_iter().rev() {
            output.push_str(&format!("#### {}\n\n", time_group.format()));

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
    single_day: bool,
    late_night_offset: u32,
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

        // Group by time (reverse order - most recent first)
        let by_time = group_by_time(commits, prs, &pr_commits, single_day, late_night_offset);
        for (time_group, items) in by_time.into_iter().rev() {
            output.push_str(&format!("#### {}\n\n", time_group.format()));

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
