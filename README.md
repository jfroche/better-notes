---
title: better-notes
---

## Overview

better-notes is a toolkit for enhancing daily notes.
Each subcommand addresses a different aspect of assembling and improving a coherent record of daily work.
The first subcommand, `standup`, generates daily standup reports from git activity across multiple repositories and forges (GitHub, GitLab, Gitea), with optional PR metadata enrichment, Claude Code conversation context, and LLM summarization.
It is inspired by [git-standup](https://github.com/kamranahmedse/git-standup), extending it with multi-forge support, late-night work attribution, conversation context from Claude Code sessions, and narrative summaries.

## Installation

From the Nix flake:

```bash
nix run github:jfroche/better-notes
```

From source with Cargo:

```bash
cargo install --path rust/cli
```

Or build with Nix:

```bash
nix build
```

## Quick start

Generate a standup report for today's activity:

```bash
better-notes standup
```

Show yesterday's activity without LLM summarization:

```bash
better-notes standup -d yesterday --no-summary
```

## Usage

```
better-notes standup [OPTIONS]
```

Options:

- `-d, --date <DATE>` — target date (default: today). Accepts `YYYY-MM-DD`, `yesterday`, or `"N days ago"`.
- `-n, --days <DAYS>` — number of days to look back from the target date (default: 1).
- `--late-night-offset <HOURS>` — hour boundary (0–6) for day rollover. Commits before this hour count as the previous day (default: 2).
- `-p, --projects-dir <PATH>` — root directory to scan for git repositories (default: `~/projects`).
- `--no-summary` — skip LLM summarization and output raw commit lists.
- `--no-conversation` — skip extraction of Claude Code conversation context.
- `--conversations-dir <PATH>` — directory for generated conversation markdown files (default: `./conversations`).
- `--debug` — enable debug logging.

## Conversation context

When running `standup`, better-notes reads Claude Code session history from `~/.claude/history.jsonl` and matches entries to discovered repositories by project path.
Conversations are interleaved chronologically with commits and PRs in the output, providing the *why* alongside the *what* — questions asked, decisions explored, and intent that doesn't appear in commit messages.

For each unique session, [cclog](https://github.com/annenpolka/cclog) converts the full session transcript to readable markdown.
The generated files are placed in the conversations directory (default `./conversations/`) and linked from the standup output.
Multiple prompts from the same session link to the same markdown file.

If `cclog` is not available on PATH, conversation entries still appear in the timeline but without links to generated markdown.
If `~/.claude/history.jsonl` does not exist, the feature is silently skipped.
Use `--no-conversation` to disable conversation extraction entirely.

## Jujutsu compatibility

Repositories using [jujutsu](https://jj-vcs.github.io/jj/) with the git backend are discovered and queried the same way as plain git repositories, since jj exposes a `.git` directory.
Three jj-specific behaviors are built in:

- Commit collection uses `git log HEAD --remotes` to avoid jj's internal refs and orphan changes, while still including unpushed local work.
- Commits without a description (jj intermediate snapshots) are filtered out.
- Filtering uses author date rather than committer date, since jj preserves the original author date when rebasing or amending while updating the committer date.

No extra configuration is needed.

## Authentication

better-notes reads forge authentication tokens from CLI configuration files or environment variables.

- *GitHub*: `GITHUB_TOKEN` environment variable, or `gh` CLI config (`~/.config/gh/hosts.yml`)
- *GitLab*: `GITLAB_TOKEN` environment variable, or `glab` CLI config (`~/.config/glab-cli/config.yml`)
- *Gitea*: `tea` CLI config (`~/.config/tea/config.yml`)

For LLM-based summaries, set the `ANTHROPIC_API_KEY` environment variable.
If a token or API key is missing, the corresponding feature degrades gracefully (PRs are skipped or summaries are omitted).

## Documentation

Development documentation (context, requirements, architecture) is in [`docs/development/`](docs/development/).

## Development

Enter the development shell:

```bash
nix develop
```

Common tasks via the Justfile:

```bash
just test    # run tests with cargo-nextest
just lint    # clippy with --deny warnings
just fmt     # format with cargo fmt
just build   # release build
```
