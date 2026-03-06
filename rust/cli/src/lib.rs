//! Library crate for better-notes, a toolkit for enhancing daily notes.
//!
//! Each module supports a subcommand that addresses a different aspect of daily notes
//! improvement. The current modules power the `standup` subcommand, which generates
//! daily standup reports from git activity and PR metadata across multiple forges.
//!
//! - [`conversation`]: Extract Claude Code session context and convert to markdown.
//! - [`forge`]: Detect hosting platforms from remote URLs and generate API/web URLs.
//! - [`git`]: Discover repositories, extract commits, deduplicate across worktrees.
//! - [`output`]: Format commits and PRs as time-grouped markdown.
//! - [`pr`]: Fetch PR/MR metadata from GitHub, GitLab, and Gitea APIs.
//! - [`summary`]: Generate narrative summaries via the Anthropic Claude API.

pub mod conversation;
pub mod forge;
pub mod git;
pub mod output;
pub mod pr;
pub mod summary;
