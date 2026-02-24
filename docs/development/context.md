---
title: Context specification
---

## Problem domain

Daily notes and standup reports require assembling information scattered across multiple tools: git logs from several repositories, pull request metadata from different forges, and CI status from various pipelines.
This assembly is tedious and error-prone when done manually, particularly for developers who work across multiple repositories hosted on different platforms (GitHub, GitLab, Gitea) and who sometimes work past midnight, complicating date-based attribution.

Shell-based tools like [git-standup](https://github.com/kamranahmedse/git-standup) address part of the problem by listing recent commits, but they operate on a single repository and a single forge, without PR enrichment or narrative summarization.

## Vision

better-notes is a growing toolkit where each subcommand addresses a different aspect of daily notes improvement.

The `standup` subcommand is the first tool.
It automates the generation of daily standup reports by discovering repositories under a project root, extracting commits within a date range, detecting the hosting forge for each repository, enriching the output with PR/MR metadata and CI status, and optionally producing a narrative summary via a large language model.
It extends git-standup's approach with multi-forge support, a late-night offset for shifted day boundaries, and LLM-based summarization.

Future subcommands will address other aspects of daily notes that are currently handled manually or inadequately.

## Stakeholders

The tool is developed and operated by a single developer.
Output is consumed for daily standup reports and timesheet entries.

## Objectives

The objectives below apply to the `standup` subcommand, the first tool in the toolkit.

- Automatically discover git repositories under a configurable project root.
- Extract commits for the current user within a specified date range, using author dates for filtering.
- Detect the hosting forge (GitHub, GitLab, Gitea) from repository remote URLs.
- Fetch PR/MR metadata (title, status, CI status, merge conflicts) from each forge's API, matching PRs to extracted commits.
- Deduplicate commits that appear in multiple worktrees of the same repository.
- Support a late-night offset so that commits before a configurable hour (e.g., 02:00) are attributed to the previous logical day.
- Format output as structured markdown, grouped by repository and time period.
- Optionally generate a narrative summary of each repository's activity using the Anthropic API.
- Filter PRs by the same date range used for commits, so the report reflects a consistent time window.

## Constraints

- Assumes a single git user identity across all repositories (matches the `user.name` from git config).
- Requires HTTPS API access to forges; SSH-only environments are not supported for API calls.
- Authentication tokens are read from CLI configuration files (`gh`, `glab`, `tea`) or environment variables; the tool does not manage credentials itself.
- LLM summarization requires an `ANTHROPIC_API_KEY` environment variable and network access to the Anthropic API.
- Repository discovery uses a fixed maximum traversal depth of 10 directories.
