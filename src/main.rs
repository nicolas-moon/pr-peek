// src/main.rs

use anyhow::{Context, Result};
use clap::Parser;
use owo_colors::{OwoColorize, Style as ColorStyle};
use reqwest::{
    Client,
    header::{ACCEPT, AUTHORIZATION, USER_AGENT},
};
use serde::{Deserialize, Serialize};
use std::{env, fmt, io::IsTerminal, process::Command};
use tabled::{
    Table, Tabled,
    settings::{Color, Style, Width, object::Rows, peaker::Priority, themes::Colorization},
};

#[derive(Parser)]
#[command(name = "pr-scout")]
#[command(about = "List a GitHub user's open PRs in a repository")]
struct Args {
    /// Repository owner, such as rust-lang
    owner: String,

    /// Repository name, such as rust
    repo: String,

    /// GitHub username whose PRs should be listed
    user: String,

    /// GitHub token. Falls back to GITHUB_TOKEN.
    #[arg(long, env = "GITHUB_TOKEN", hide_env_values = true)]
    token: Option<String>,

    /// Print raw JSON instead of the formatted table.
    #[arg(long)]
    json: bool,
}

const GITHUB_GRAPHQL_URL: &str = "https://api.github.com/graphql";

/// Search is used (rather than `repository.pullRequests`) so GitHub filters
/// by author server-side; `author:` matching is case-insensitive.
const PULL_REQUESTS_QUERY: &str = r#"
query($q: String!, $after: String) {
  search(query: $q, type: ISSUE, first: 100, after: $after) {
    pageInfo { hasNextPage endCursor }
    nodes {
      ... on PullRequest {
        number
        title
        url
        isDraft
        headRefName
        baseRefName
        reviewDecision
        mergeQueueEntry { position state }
      }
    }
  }
}
"#;

#[derive(Serialize)]
struct GraphQlRequest<'a> {
    query: &'a str,
    variables: SearchVariables,
}

#[derive(Serialize)]
struct SearchVariables {
    q: String,
    after: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphQlResponse {
    data: Option<SearchData>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct SearchData {
    search: SearchConnection,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchConnection {
    page_info: PageInfo,
    nodes: Vec<PullRequest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PullRequest {
    number: u64,
    title: String,
    url: String,
    is_draft: bool,
    head_ref_name: String,
    base_ref_name: String,
    /// `None` when the repo has no required-review rule and nobody has
    /// reviewed yet.
    review_decision: Option<ReviewDecision>,
    /// `None` unless the PR is currently sitting in a merge queue.
    merge_queue_entry: Option<MergeQueueEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

#[derive(Debug, Deserialize, Serialize)]
struct MergeQueueEntry {
    position: u64,
    state: MergeQueueState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum MergeQueueState {
    AwaitingChecks,
    Locked,
    Mergeable,
    Queued,
    Unmergeable,
    /// Forward-compat: GitHub may add states we don't know about yet.
    #[serde(other)]
    Unknown,
}

impl fmt::Display for MergeQueueState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AwaitingChecks => "awaiting checks",
            Self::Locked => "locked",
            Self::Mergeable => "mergeable",
            Self::Queued => "queued",
            Self::Unmergeable => "unmergeable",
            Self::Unknown => "unknown",
        })
    }
}

#[derive(Tabled)]
struct PrRow {
    number: String,
    title: String,
    branch: String,
    link: String,
    draft: String,
    review: String,
    queue: String,
}

/// Shape of `--json` output: the query context plus the raw PR data.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonOutput<'a> {
    owner: &'a str,
    repo: &'a str,
    user: &'a str,
    pull_requests: &'a [PullRequest],
}

/// The server-side search filter. `author:` matching is case-insensitive.
fn search_query(owner: &str, repo: &str, user: &str) -> String {
    format!("repo:{owner}/{repo} is:pr is:open author:{user}")
}

fn github_token(cli_token: Option<&str>) -> Option<String> {
    // Prefer --token flag (clap already falls back to GITHUB_TOKEN env)
    if let Some(token) = cli_token {
        let token = token.trim();
        if !token.is_empty() {
            return Some(token.to_owned());
        }
    }

    // Fall back to GitHub CLI
    let output = Command::new("gh").args(["auth", "token"]).output().ok()?;

    if !output.status.success() {
        return None;
    }

    let token = String::from_utf8(output.stdout).ok()?;
    let token = token.trim();

    if token.is_empty() {
        None
    } else {
        Some(token.to_owned())
    }
}

/// Wrap a label in an OSC 8 escape sequence so terminals render it
/// as a clickable hyperlink.
fn hyperlink(url: &str, label: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\{label}\x1b]8;;\x1b\\")
}

struct Palette {
    number: ColorStyle,
    head: ColorStyle,
    arrow: ColorStyle,
    base: ColorStyle,
    link: ColorStyle,
    draft: ColorStyle,
    // Semantic status styles shared by the review and queue columns.
    ok: ColorStyle,
    pending: ColorStyle,
    blocked: ColorStyle,
}

impl Palette {
    fn colored() -> Self {
        Self {
            number: ColorStyle::new().green(),
            head: ColorStyle::new().cyan(),
            arrow: ColorStyle::new().dimmed(),
            base: ColorStyle::new().magenta(),
            link: ColorStyle::new().blue(),
            draft: ColorStyle::new().yellow(),
            ok: ColorStyle::new().green(),
            pending: ColorStyle::new().yellow(),
            blocked: ColorStyle::new().red(),
        }
    }

    fn plain() -> Self {
        Self {
            number: ColorStyle::new(),
            head: ColorStyle::new(),
            arrow: ColorStyle::new(),
            base: ColorStyle::new(),
            link: ColorStyle::new(),
            draft: ColorStyle::new(),
            ok: ColorStyle::new(),
            pending: ColorStyle::new(),
            blocked: ColorStyle::new(),
        }
    }

    fn review_cell(&self, decision: Option<ReviewDecision>) -> String {
        match decision {
            Some(ReviewDecision::Approved) => "✓ approved".style(self.ok).to_string(),
            Some(ReviewDecision::ChangesRequested) => {
                "✗ changes requested".style(self.blocked).to_string()
            }
            Some(ReviewDecision::ReviewRequired) => {
                "○ review required".style(self.pending).to_string()
            }
            None => String::new(),
        }
    }

    fn queue_cell(&self, entry: Option<&MergeQueueEntry>) -> String {
        let Some(entry) = entry else {
            return String::new();
        };

        let style = match entry.state {
            MergeQueueState::Mergeable => self.ok,
            MergeQueueState::Unmergeable => self.blocked,
            MergeQueueState::AwaitingChecks
            | MergeQueueState::Locked
            | MergeQueueState::Queued
            | MergeQueueState::Unknown => self.pending,
        };

        format!("#{} {}", entry.position, entry.state)
            .style(style)
            .to_string()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let client = Client::builder()
        .user_agent("pr-scout")
        .build()
        .context("failed to create HTTP client")?;

    // The GraphQL API has no unauthenticated mode, so a token is mandatory.
    let token = github_token(args.token.as_deref()).context(
        "a GitHub token is required: pass --token, set GITHUB_TOKEN, or run `gh auth login`",
    )?;

    let search = search_query(&args.owner, &args.repo, &args.user);

    let mut after: Option<String> = None;
    let mut matching_prs = Vec::new();

    loop {
        let body = GraphQlRequest {
            query: PULL_REQUESTS_QUERY,
            variables: SearchVariables {
                q: search.clone(),
                after: after.take(),
            },
        };

        let response = client
            .post(GITHUB_GRAPHQL_URL)
            .header(USER_AGENT, "pr-scout")
            .header(ACCEPT, "application/vnd.github+json")
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .json(&body)
            .send()
            .await
            .context("failed to request pull requests")?
            .error_for_status()
            .context("GitHub returned an error")?;

        let GraphQlResponse { data, errors } = response
            .json()
            .await
            .context("failed to decode GitHub response")?;

        if !errors.is_empty() {
            let messages: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
            anyhow::bail!("GitHub GraphQL error: {}", messages.join("; "));
        }

        let SearchConnection { page_info, nodes } =
            data.context("GitHub response contained no data")?.search;

        matching_prs.extend(nodes);

        match (page_info.has_next_page, page_info.end_cursor) {
            (true, Some(cursor)) => after = Some(cursor),
            _ => break,
        }
    }

    // Machine-readable mode: dump the raw data and skip all table styling.
    // An empty result still prints valid JSON (with an empty list).
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&JsonOutput {
                owner: &args.owner,
                repo: &args.repo,
                user: &args.user,
                pull_requests: &matching_prs,
            })
            .context("failed to encode JSON output")?
        );

        return Ok(());
    }

    if matching_prs.is_empty() {
        println!(
            "No open PRs from {} in {}/{}.",
            args.user, args.owner, args.repo
        );

        return Ok(());
    }

    println!(
        "Open PRs from {} in {}/{}:\n",
        args.user, args.owner, args.repo
    );

    let is_tty = std::io::stdout().is_terminal();
    let use_color = is_tty && env::var_os("NO_COLOR").is_none();

    let palette = if use_color {
        Palette::colored()
    } else {
        Palette::plain()
    };

    let rows: Vec<PrRow> = matching_prs
        .iter()
        .map(|pr| PrRow {
            number: format!("#{}", pr.number).style(palette.number).to_string(),
            title: pr.title.clone(),
            branch: format!(
                "{} {} {}",
                pr.head_ref_name.style(palette.head),
                "→".style(palette.arrow),
                pr.base_ref_name.style(palette.base)
            ),
            // In a terminal, show a compact clickable label; otherwise
            // print the full URL so pipes and logs stay useful.
            link: if is_tty {
                hyperlink(&pr.url, "open ↗").style(palette.link).to_string()
            } else {
                pr.url.clone()
            },
            draft: if pr.is_draft {
                "✓".style(palette.draft).to_string()
            } else {
                String::new()
            },
            review: palette.review_cell(pr.review_decision),
            queue: palette.queue_cell(pr.merge_queue_entry.as_ref()),
        })
        .collect();

    let mut table = Table::new(rows);
    table.with(Style::rounded());

    if use_color {
        table.with(Colorization::exact([Color::BOLD], Rows::first()));
    }

    // Wrap columns to fit the terminal so long titles/URLs don't break rows.
    if let Some((terminal_size::Width(width), _)) = terminal_size::terminal_size() {
        table.with(
            Width::wrap(width as usize)
                .keep_words(true)
                .priority(Priority::max(true)),
        );
    }

    println!("{table}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RESPONSE: &str = r#"{
      "data": {
        "search": {
          "pageInfo": { "hasNextPage": true, "endCursor": "cursor-2" },
          "nodes": [
            {
              "number": 12345,
              "title": "Fix off-by-one in scanner",
              "url": "https://github.com/rust-lang/rust/pull/12345",
              "isDraft": false,
              "headRefName": "ferris/fix-scanner",
              "baseRefName": "master",
              "reviewDecision": "APPROVED",
              "mergeQueueEntry": { "position": 2, "state": "AWAITING_CHECKS" }
            },
            {
              "number": 12350,
              "title": "WIP: refactor lexer",
              "url": "https://github.com/rust-lang/rust/pull/12350",
              "isDraft": true,
              "headRefName": "ferris/lexer-refactor",
              "baseRefName": "master",
              "reviewDecision": null,
              "mergeQueueEntry": null
            },
            {
              "number": 12351,
              "title": "Add new feature",
              "url": "https://github.com/rust-lang/rust/pull/12351",
              "isDraft": false,
              "headRefName": "ferris/feature",
              "baseRefName": "master",
              "reviewDecision": "CHANGES_REQUESTED",
              "mergeQueueEntry": { "position": 1, "state": "FUTURE_STATE" }
            }
          ]
        }
      },
      "errors": []
    }"#;

    #[test]
    fn decodes_graphql_response() {
        let GraphQlResponse { data, errors } =
            serde_json::from_str::<GraphQlResponse>(SAMPLE_RESPONSE).unwrap();

        assert!(errors.is_empty());
        let SearchData { search } = data.unwrap();
        assert!(search.page_info.has_next_page);
        assert_eq!(search.page_info.end_cursor.as_deref(), Some("cursor-2"));
        assert_eq!(search.nodes.len(), 3);

        let pr = &search.nodes[0];
        assert_eq!(pr.number, 12345);
        assert_eq!(pr.review_decision, Some(ReviewDecision::Approved));
        let queue = pr.merge_queue_entry.as_ref().unwrap();
        assert_eq!(queue.position, 2);
        assert_eq!(queue.state, MergeQueueState::AwaitingChecks);

        // null review/queue fields decode to None
        assert_eq!(search.nodes[1].review_decision, None);
        assert!(search.nodes[1].merge_queue_entry.is_none());
        assert!(search.nodes[1].is_draft);

        // unknown queue states degrade to Unknown instead of failing
        let state = search.nodes[2].merge_queue_entry.as_ref().unwrap().state;
        assert_eq!(state, MergeQueueState::Unknown);
    }

    #[test]
    fn decodes_graphql_errors() {
        let response: GraphQlResponse = serde_json::from_str(
            r#"{ "data": null, "errors": [ { "message": "Bad thing happened" } ] }"#,
        )
        .unwrap();

        assert!(response.data.is_none());
        assert_eq!(response.errors.len(), 1);
        assert_eq!(response.errors[0].message, "Bad thing happened");
    }

    #[test]
    fn search_query_filters_server_side() {
        assert_eq!(
            search_query("rust-lang", "rust", "ferris"),
            "repo:rust-lang/rust is:pr is:open author:ferris"
        );
    }

    #[test]
    fn cli_token_is_trimmed() {
        assert_eq!(
            github_token(Some("  ghp_test  ")).as_deref(),
            Some("ghp_test")
        );
    }

    #[test]
    fn review_cells_match_decisions() {
        let palette = Palette::plain();
        assert!(palette.review_cell(None).is_empty());
        assert!(
            palette
                .review_cell(Some(ReviewDecision::Approved))
                .contains("✓ approved")
        );
        assert!(
            palette
                .review_cell(Some(ReviewDecision::ChangesRequested))
                .contains("✗ changes requested")
        );
        assert!(
            palette
                .review_cell(Some(ReviewDecision::ReviewRequired))
                .contains("○ review required")
        );
    }

    #[test]
    fn queue_cells_match_states() {
        let palette = Palette::plain();
        assert!(palette.queue_cell(None).is_empty());

        let entry = MergeQueueEntry {
            position: 3,
            state: MergeQueueState::Mergeable,
        };
        assert!(palette.queue_cell(Some(&entry)).contains("#3 mergeable"));
    }

    #[test]
    fn queue_state_display() {
        assert_eq!(
            MergeQueueState::AwaitingChecks.to_string(),
            "awaiting checks"
        );
        assert_eq!(MergeQueueState::Locked.to_string(), "locked");
        assert_eq!(MergeQueueState::Unknown.to_string(), "unknown");
    }

    #[test]
    fn hyperlink_embeds_osc8_sequence() {
        let link = hyperlink("https://example.com/pull/1", "open ↗");
        assert!(link.starts_with("\x1b]8;;https://example.com/pull/1\x1b\\"));
        assert!(link.ends_with("open ↗\x1b]8;;\x1b\\"));
    }

    #[test]
    fn json_output_uses_camel_case_keys() {
        let pr = PullRequest {
            number: 1,
            title: "title".to_owned(),
            url: "https://example.com/pull/1".to_owned(),
            is_draft: false,
            head_ref_name: "head".to_owned(),
            base_ref_name: "base".to_owned(),
            review_decision: None,
            merge_queue_entry: None,
        };

        let out = JsonOutput {
            owner: "owner",
            repo: "repo",
            user: "user",
            pull_requests: std::slice::from_ref(&pr),
        };
        let rendered = serde_json::to_string(&out).unwrap();

        assert!(rendered.contains(r#""pullRequests""#));
        assert!(rendered.contains(r#""headRefName":"head""#));
        assert!(rendered.contains(r#""reviewDecision":null"#));
    }
}
