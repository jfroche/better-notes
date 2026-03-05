use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod conversation;
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

    /// Hour offset for day boundary (0-6). Commits before this hour count as previous day.
    /// Useful for late-night work: --late-night-offset 2 means 00:00-02:00 counts as previous day.
    #[arg(long, default_value = "2", value_parser = clap::value_parser!(u32).range(0..=6))]
    late_night_offset: u32,

    /// Projects root directory (default: ~/projects)
    #[arg(short, long)]
    projects_dir: Option<PathBuf>,

    /// Skip LLM summarization (just list commits)
    #[arg(long)]
    no_summary: bool,

    /// Skip conversation extraction from Claude Code sessions
    #[arg(long)]
    no_conversation: bool,

    /// Directory for generated conversation markdown files
    #[arg(long, default_value = "./conversations")]
    conversations_dir: PathBuf,
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
    let projects_dir = args.projects_dir.unwrap_or_else(|| {
        dirs::home_dir()
            .expect("could not determine home directory")
            .join("projects")
    });
    tracing::info!("Running standup for {:?}", projects_dir);

    // Parse target date (end of day)
    let target_date = git::parse_date(&args.date)?;

    // With late_night_offset, the "logical day" starts at offset:00 and ends at offset:00 next day.
    // E.g., with offset=2, "Monday" runs from Mon 02:00 to Tue 01:59:59.
    let offset_hours = args.late_night_offset as i64;

    // Calculate start of first day to include: with days=1, show only target date
    let start_date = target_date.date_naive() - chrono::Duration::days((args.days - 1) as i64);
    let since = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        start_date.and_hms_opt(offset_hours as u32, 0, 0).unwrap(),
        chrono::Utc,
    );

    // End of range: target_date + 1 day at offset:00 - 1 second (i.e., offset-1:59:59 next day)
    let until = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        (target_date.date_naive() + chrono::Duration::days(1))
            .and_hms_opt(offset_hours as u32, 0, 0)
            .unwrap(),
        chrono::Utc,
    ) - chrono::Duration::seconds(1);

    tracing::debug!("Looking for commits from {} to {}", since, until);

    // Discover repositories
    let repos = git::discover_repositories(&projects_dir)?;
    tracing::info!("Found {} repositories", repos.len());

    // Collect commits from all repositories
    let mut all_commits = Vec::new();
    for repo in &repos {
        match git::get_commits(repo, &since, &until) {
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
    let grouped = git::deduplicate_and_group(all_commits, &projects_dir);

    // Fetch PR status for each group
    let mut enriched_groups = Vec::new();
    for (forge, commits) in grouped {
        let prs = pr::fetch_prs_for_commits(&forge, &commits).await?;
        // Filter PRs to only those updated within the date range
        let filtered_prs: Vec<_> = prs
            .into_iter()
            .filter(|pr| {
                pr.updated_at
                    .map(|dt| dt >= since && dt <= until)
                    .unwrap_or(false)
            })
            .collect();
        enriched_groups.push((forge, commits, filtered_prs));
    }

    // Extract conversation context from Claude Code sessions
    let conversation_groups = if args.no_conversation {
        HashMap::new()
    } else {
        match conversation::read_history(&since, &until) {
            Ok(entries) if !entries.is_empty() => {
                tracing::info!("Found {} conversation entries", entries.len());
                let matched = conversation::match_to_repos(&entries, &repos);
                tracing::debug!("Matched {} entries to repositories", matched.len());
                conversation::convert_all_sessions(&matched, &args.conversations_dir)
            }
            Ok(_) => {
                tracing::debug!("No conversation entries found for date range");
                HashMap::new()
            }
            Err(e) => {
                tracing::warn!("Failed to read conversation history: {}", e);
                HashMap::new()
            }
        }
    };

    // Generate output (group by hours if single day)
    let single_day = args.days == 1;
    let output = if args.no_summary {
        output::format_without_summary(
            &enriched_groups,
            &conversation_groups,
            &args.conversations_dir,
            single_day,
            args.late_night_offset,
        )
    } else {
        output::format_with_summary(
            &enriched_groups,
            &conversation_groups,
            &args.conversations_dir,
            single_day,
            args.late_night_offset,
        )
        .await?
    };

    println!("{output}");

    Ok(())
}
