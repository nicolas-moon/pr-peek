// src/main.rs

mod github;
mod output;
mod target;
mod update;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use reqwest::Client;

use crate::{
    github::{PullRequest, Role, Sort},
    output::Report,
};

#[derive(Parser)]
#[command(
    version = concat!(
        env!("CARGO_PKG_VERSION"),
        " (",
        env!("BUILD_COMMIT"),
        ")\n  target: ",
        env!("BUILD_TARGET"),
        "\n  rustc:  ",
        env!("BUILD_RUSTC"),
    ),
    about = "List a GitHub user's open PRs in a repository",
    override_usage = "pr-peek [OPTIONS] [<OWNER> <REPO>] [<USER>]\n       pr-peek update",
)]
struct Args {
    /// [<OWNER> <REPO>] [<USER>]. Without OWNER and REPO, the repository
    /// comes from the current checkout's `upstream` or `origin` GitHub
    /// remote; without USER, the token's owner (you) is used.
    #[arg(value_name = "ARGS", num_args = 0..=3)]
    targets: Vec<String>,

    /// List PRs awaiting USER's review instead of PRs USER opened
    #[arg(short, long)]
    reviewing: bool,

    /// Only show PRs that are blocked: changes requested, failing checks,
    /// or merge conflicts
    #[arg(short, long)]
    attention: bool,

    /// Order the results (default: newest)
    #[arg(long, default_value = "newest")]
    sort: Sort,

    /// GitHub token. Falls back to GITHUB_TOKEN.
    #[arg(long, env = "GITHUB_TOKEN", hide_env_values = true)]
    token: Option<String>,

    /// Print raw JSON instead of the formatted table. Also enabled by
    /// PR_PEEK_JSON=1, which the Tern plugin sets so its lens can render
    /// the data natively.
    #[arg(
        long,
        env = "PR_PEEK_JSON",
        action = clap::ArgAction::SetTrue,
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    json: bool,

    #[command(subcommand)]
    command: Option<SubCommand>,
}

#[derive(Subcommand)]
enum SubCommand {
    /// Update pr-peek to the latest release
    Update {
        /// Only check for a newer release without updating
        #[arg(long)]
        check: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let client = Client::builder()
        .user_agent(github::user_agent())
        .build()
        .context("failed to create HTTP client")?;

    if let Some(SubCommand::Update { check }) = args.command {
        return update::run(&client, check).await;
    }

    let target = target::resolve(args.targets, target::repo_from_git_remote)?;

    // The GraphQL API has no unauthenticated mode, so a token is mandatory.
    let token = github::token(args.token.as_deref()).context(
        "a GitHub token is required: pass --token, set GITHUB_TOKEN, or run `gh auth login`",
    )?;

    let role = if args.reviewing {
        Role::ReviewRequested
    } else {
        Role::Author
    };

    let search = github::search_query(
        &target.owner,
        &target.repo,
        target.user.as_deref(),
        role,
        args.sort,
    );
    let result = github::search_pull_requests(&client, &token, &search).await?;

    // `--attention` keeps only the PRs that are blocked.
    let pull_requests: Vec<PullRequest> = if args.attention {
        result
            .pull_requests
            .into_iter()
            .filter(PullRequest::is_blocked)
            .collect()
    } else {
        result.pull_requests
    };

    let report = Report {
        owner: &target.owner,
        repo: &target.repo,
        user: target.user.as_deref().unwrap_or(&result.viewer),
        role,
        attention: args.attention,
        pull_requests: &pull_requests,
    };

    if args.json {
        output::print_json(&report)
    } else {
        output::print_table(&report);
        Ok(())
    }
}
