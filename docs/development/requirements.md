---
title: Requirements specification
---

## Functional requirements

These requirements describe the `standup` subcommand, the first tool in the better-notes toolkit.

- *FR-1 Repository discovery*: The tool discovers git repositories by recursively walking a configurable project root directory up to a maximum depth of 10 levels.
  It identifies directories containing a `.git` folder and extracts the primary remote URL to determine the hosting forge.

- *FR-2 Commit extraction*: For each discovered repository, the tool extracts commits authored by the current user within a specified date range.
  Filtering uses author dates (not committer dates) and supports full timestamps.
  Commits without descriptions (such as jj intermediate snapshots) are excluded.

- *FR-3 Forge detection*: The tool determines the hosting forge from the repository's remote URL.
  Supported forges are GitHub (github.com), GitLab (gitlab.com and self-hosted instances with "gitlab" in the hostname), and Gitea (known instances and hostnames containing "gitea").
  Unknown self-hosted forges default to the Gitea API.

- *FR-4 PR/MR fetching*: For repositories with a detected forge and a valid authentication token, the tool fetches pull requests or merge requests authored by the current user.
  Each PR includes its title, status (open, merged, closed), CI status (pending, success, failure, unknown), merge conflict state, and associated commit hashes.

- *FR-5 Commit deduplication*: Commits that appear in multiple worktrees of the same repository are deduplicated.
  Grouping uses the repository's forge identity (owner/repo) when available, falling back to the repository path.

- *FR-6 Late-night offset*: A configurable hour boundary (0-6, default 2) shifts the logical day boundary.
  Commits authored before this hour are attributed to the previous calendar day.
  This allows late-night work sessions to appear under the correct logical day.

- *FR-7 Markdown formatting*: Output is structured markdown grouped by repository and time period.
  When viewing a single day, items are grouped by hour; when viewing multiple days, items are grouped by date.
  Commits that are part of a displayed PR are omitted from the commit list to avoid duplication.

- *FR-8 LLM summarization*: When an Anthropic API key is available and summarization is not disabled, the tool generates a narrative summary for each repository's activity.
  The summary is produced by sending commit messages and PR descriptions to the Claude API, which returns a first-person past-tense summary suitable for standup notes.

- *FR-9 Date-range PR filtering*: PRs are filtered to the same date range used for commit extraction, based on their `updated_at` timestamp.
  This ensures the report reflects a consistent time window rather than including stale or future PRs.

## Non-functional requirements

- *NFR-1 Graceful degradation on missing tokens*: When a forge authentication token is unavailable, the tool skips PR fetching for that forge and proceeds with commit-only output.
  A warning is emitted via the tracing system.

- *NFR-2 Graceful degradation on missing API key*: When the Anthropic API key is not set, the tool omits LLM summaries and produces commit-and-PR-only output without error.

- *NFR-3 Structured logging*: The tool uses tracing-based structured logging.
  Debug-level output is enabled via the `--debug` flag, which sets the tracing filter to `better_notes=debug`.
  The default log level is controlled by the `RUST_LOG` environment variable.

- *NFR-4 Reproducible build*: The project builds reproducibly via its Nix flake using crane.
  The Nix package wraps the binary with runtime dependencies (git, gh, tea, glab) on `PATH`.
  Passthrough tests include clippy, cargo doc, cargo deny, and cargo nextest.
