---
title: Architecture
---

## CLI structure

better-notes uses clap with derive macros for command-line parsing.
The top-level `Cli` struct holds global options (currently `--debug`) and dispatches to subcommands via a `Commands` enum.
The `standup` subcommand is the first variant; adding a new subcommand means adding a variant to `Commands`, a corresponding argument struct, and a handler in `main`.

## Modules

All modules currently support the `standup` subcommand.

| Module | File | Responsibility |
|--------|------|----------------|
| `forge` | `forge.rs` | Detect hosting platform from remote URLs, generate commit/PR/API URLs |
| `git` | `git.rs` | Discover repositories, extract and parse commits, deduplicate and group by forge |
| `output` | `output.rs` | Format commits and PRs as grouped markdown, apply time-based grouping |
| `pr` | `pr.rs` | Fetch PR/MR metadata from GitHub, GitLab, and Gitea APIs, resolve auth tokens |
| `summary` | `summary.rs` | Generate narrative summaries via the Anthropic Claude API |

## Data flow

The `standup` subcommand follows this pipeline:

1. *CLI args* are parsed into a target date, day count, late-night offset, project root, and summary preference.
2. *Repository discovery* (`git::discover_repositories`) recursively walks the project root, identifies `.git` directories, extracts remote URLs, and detects forges.
3. *Commit extraction* (`git::get_commits`) runs `git log` in each repository, filtering by author identity and date range with full timestamps. Commits without descriptions (jj snapshots) are excluded.
4. *Deduplication and grouping* (`git::deduplicate_and_group`) removes duplicate commits across worktrees and groups results by forge identity or repository path.
5. *PR enrichment* (`pr::get_pull_requests`) queries each forge's API for the current user's PRs, fetches associated commit hashes, CI status, and merge conflict state. PRs are then filtered to the same date range as commits.
6. *Formatting* (`output::format_with_summary` or `output::format_without_summary`) organizes items by time period, omits commits already covered by PRs, and renders structured markdown.
7. *Summarization* (`summary::Summarizer::summarize`), if enabled, sends commit messages and PR descriptions to the Claude API and prepends the returned narrative to each repository section.

## Key types

The core domain types bridge the pipeline stages:

- `Forge` (enum): GitHub, Gitea, or GitLab, each carrying the owner, repo, and (for self-hosted) hostname. Parsed from remote URLs, used for API dispatch and URL generation.
- `Repository` (struct): a filesystem path paired with an optional `Forge`. Produced by discovery, consumed by commit extraction.
- `Commit` (struct): hash, short hash, subject, optional body, author, and UTC datetime. Extracted from `git log` output, consumed by deduplication, formatting, and summarization.
- `PullRequest` (struct): number, title, optional description, status, CI status, conflict state, URL, associated commit hashes, and optional `updated_at` timestamp. Fetched from forge APIs, matched to commits by hash, filtered by date range.
- `PrStatus` (enum): Open, Merged, or Closed.
- `CiStatus` (enum): Pending, Success, Failure, or Unknown.

## Authentication strategy

Token resolution follows a per-forge priority chain, preferring environment variables over CLI configuration files:

- *GitHub*: `GITHUB_TOKEN` env var, then `~/.config/gh/hosts.yml` (`oauth_token` field)
- *GitLab*: `GITLAB_TOKEN` env var, then `~/.config/glab-cli/config.yml` (host-specific `token` field)
- *Gitea*: `~/.config/tea/config.yml` (host-matched `token` field)

When no token is found, PR fetching is skipped for that forge with a tracing warning.
The Anthropic API key is read from the `ANTHROPIC_API_KEY` environment variable; if absent, summarization is silently skipped.

## Build system

The project builds with a Nix flake using [crane](https://crane-lang.org/) for Rust compilation.
The flake provides:

- A default package (`better-notes`) that wraps the binary with git, gh, tea, and glab on `PATH` via `wrapProgram`.
- A development shell with the Rust toolchain (1.85.0 from rust-overlay), cargo-nextest, and cargo-watch.
- Passthrough tests: clippy, cargo doc, cargo deny, and cargo nextest.
- Formatting via treefmt-nix.

The workspace uses `opt-level = "z"` in both dev and release profiles to optimize for binary size.

## Extensibility

Adding a new subcommand involves three steps:

1. Add a variant to the `Commands` enum in `main.rs` with a corresponding argument struct.
2. Add a module under `rust/cli/src/` implementing the subcommand's logic, and re-export it from `lib.rs`.
3. Add a match arm in the `main` function to dispatch to the new handler.

Existing modules (`forge`, `git`, `pr`, `summary`, `output`) can be reused if the new subcommand shares infrastructure such as repository discovery, forge detection, or LLM summarization.
