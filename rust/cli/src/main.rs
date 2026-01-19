use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod forge;
mod git;
mod output;
mod pr;
mod summary;

#[derive(Parser)]
#[command(name = "better-notes")]
#[command(about = "Tools for enhancing daily notes")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate git standup report with summaries
    Standup(StandupArgs),
}

#[derive(Args)]
struct StandupArgs {
    /// Target date (default: today). Formats: YYYY-MM-DD, "yesterday", "2 days ago"
    #[arg(short, long)]
    date: Option<String>,

    /// Days to look back from target date (default: 1)
    #[arg(short = 'n', long, default_value = "1")]
    days: u32,

    /// Projects root directory
    #[arg(short, long, default_value = "/home/jfroche/projects")]
    projects_dir: PathBuf,

    /// Skip LLM summarization (just list commits)
    #[arg(long)]
    no_summary: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Standup(args) => run_standup(args).await?,
    }

    Ok(())
}

async fn run_standup(args: StandupArgs) -> Result<()> {
    tracing::info!("Running standup for {:?}", args.projects_dir);

    // Parse target date
    let target_date = git::parse_date(&args.date)?;
    let since = target_date - chrono::Duration::days(args.days as i64);

    tracing::debug!("Looking for commits from {} to {}", since, target_date);

    // Discover repositories
    let repos = git::discover_repositories(&args.projects_dir)?;
    tracing::info!("Found {} repositories", repos.len());

    // Collect commits from all repositories
    let mut all_commits = Vec::new();
    for repo in &repos {
        match git::get_commits(repo, &since, &target_date) {
            Ok(commits) => {
                if !commits.is_empty() {
                    tracing::debug!("Found {} commits in {:?}", commits.len(), repo.path);
                    all_commits.extend(commits.into_iter().map(|c| (repo.clone(), c)));
                }
            }
            Err(e) => {
                tracing::warn!("Failed to get commits from {:?}: {}", repo.path, e);
            }
        }
    }

    // Deduplicate and group by forge
    let grouped = git::deduplicate_and_group(all_commits, &args.projects_dir);

    // Fetch PR status for each group
    let mut enriched_groups = Vec::new();
    for (forge, commits) in grouped {
        let prs = pr::fetch_prs_for_commits(&forge, &commits).await?;
        enriched_groups.push((forge, commits, prs));
    }

    // Generate output (group by hours if single day)
    let single_day = args.days == 1;
    let output = if args.no_summary {
        output::format_without_summary(&enriched_groups, single_day)
    } else {
        output::format_with_summary(&enriched_groups, single_day).await?
    };

    println!("{output}");

    Ok(())
}
