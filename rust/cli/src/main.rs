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
    /// Enable debug logging
    #[arg(long, global = true)]
    debug: bool,

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
    let cli = Cli::parse();

    let default_level = if cli.debug { "debug" } else { "warn" };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    match cli.command {
        Commands::Standup(args) => run_standup(args).await?,
    }

    Ok(())
}

async fn run_standup(args: StandupArgs) -> Result<()> {
    tracing::info!("Running standup for {:?}", args.projects_dir);

    // Parse target date (end of day)
    let target_date = git::parse_date(&args.date)?;
    // Calculate start of first day to include: with days=1, show only target date
    let start_date = target_date.date_naive() - chrono::Duration::days((args.days - 1) as i64);
    let since = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        start_date.and_hms_opt(0, 0, 0).unwrap(),
        chrono::Utc,
    );

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
        // Filter PRs to only those updated within the date range
        let filtered_prs: Vec<_> = prs
            .into_iter()
            .filter(|pr| {
                pr.updated_at
                    .map(|dt| dt >= since && dt <= target_date)
                    .unwrap_or(false)
            })
            .collect();
        enriched_groups.push((forge, commits, filtered_prs));
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
