//! GitHub GraphQL access: auth, the PR search query, and its response types.

use anyhow::{Context, Result};
use reqwest::{
    Client,
    header::{ACCEPT, AUTHORIZATION, USER_AGENT},
};
use serde::{Deserialize, Deserializer, Serialize};
use std::{fmt, process::Command};

const GITHUB_GRAPHQL_URL: &str = "https://api.github.com/graphql";

/// Search is used (rather than `repository.pullRequests`) so GitHub filters
/// by author or requested reviewer server-side. `viewer` names the token's
/// owner, which stands in for an omitted user.
const PULL_REQUESTS_QUERY: &str = r#"
query($q: String!, $after: String) {
  viewer { login }
  search(query: $q, type: ISSUE, first: 100, after: $after) {
    pageInfo { hasNextPage endCursor }
    nodes {
      ... on PullRequest {
        number
        title
        url
        isDraft
        author { login }
        headRefName
        baseRefName
        reviewDecision
        commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
        mergeQueueEntry { position state }
      }
    }
  }
}
"#;

/// Which relationship between the user and a PR is being listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    /// PRs the user opened.
    Author,
    /// PRs waiting on a review from the user (directly or via a team).
    ReviewRequested,
}

/// The server-side search filter. Qualifier matching is case-insensitive;
/// `@me` resolves to the token's owner.
pub fn search_query(owner: &str, repo: &str, user: Option<&str>, role: Role) -> String {
    let user = user.unwrap_or("@me");
    let qualifier = match role {
        Role::Author => "author",
        Role::ReviewRequested => "review-requested",
    };
    format!("repo:{owner}/{repo} is:pr is:open {qualifier}:{user}")
}

pub fn token(cli_token: Option<&str>) -> Option<String> {
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

#[derive(Serialize)]
struct GraphQlRequest<'a> {
    query: &'a str,
    variables: SearchVariables<'a>,
}

#[derive(Serialize)]
struct SearchVariables<'a> {
    q: &'a str,
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
    viewer: Login,
    search: SearchConnection,
}

#[derive(Debug, Deserialize)]
struct Login {
    login: String,
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
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub is_draft: bool,
    /// `None` when the author's account was deleted (GitHub's "ghost").
    #[serde(deserialize_with = "login")]
    pub author: Option<String>,
    pub head_ref_name: String,
    pub base_ref_name: String,
    /// `None` when the repo has no required-review rule and nobody has
    /// reviewed yet.
    pub review_decision: Option<ReviewDecision>,
    /// Combined CI state of the head commit; `None` when it has no checks
    /// or statuses.
    #[serde(
        rename(deserialize = "commits", serialize = "checkStatus"),
        deserialize_with = "last_commit_rollup"
    )]
    pub check_status: Option<CheckStatus>,
    /// `None` unless the PR is currently sitting in a merge queue.
    pub merge_queue_entry: Option<MergeQueueEntry>,
}

/// `author { login }` → the login.
fn login<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(Option::<Login>::deserialize(deserializer)?.map(|actor| actor.login))
}

/// `commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }`
/// → the rollup state of that one commit.
fn last_commit_rollup<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<CheckStatus>, D::Error> {
    #[derive(Deserialize)]
    struct Commits {
        nodes: Vec<CommitNode>,
    }

    #[derive(Deserialize)]
    struct CommitNode {
        commit: Commit,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Commit {
        status_check_rollup: Option<Rollup>,
    }

    #[derive(Deserialize)]
    struct Rollup {
        state: CheckStatus,
    }

    let mut commits = Commits::deserialize(deserializer)?;
    Ok(commits
        .nodes
        .pop()
        .and_then(|node| node.commit.status_check_rollup)
        .map(|rollup| rollup.state))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

/// GitHub's `StatusState` for a commit's combined checks and statuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckStatus {
    Error,
    Expected,
    Failure,
    Pending,
    Success,
    /// Forward-compat: GitHub may add states we don't know about yet.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MergeQueueEntry {
    pub position: u64,
    pub state: MergeQueueState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MergeQueueState {
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

pub struct SearchResult {
    /// Login of the token's owner.
    pub viewer: String,
    pub pull_requests: Vec<PullRequest>,
}

/// Run `search` (see [`search_query`]) and collect every page of results.
pub async fn search_pull_requests(
    client: &Client,
    token: &str,
    search: &str,
) -> Result<SearchResult> {
    let mut after: Option<String> = None;
    let mut pull_requests = Vec::new();

    loop {
        let body = GraphQlRequest {
            query: PULL_REQUESTS_QUERY,
            variables: SearchVariables {
                q: search,
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

        let SearchData { viewer, search } = data.context("GitHub response contained no data")?;
        let SearchConnection { page_info, nodes } = search;

        pull_requests.extend(nodes);

        match (page_info.has_next_page, page_info.end_cursor) {
            (true, Some(cursor)) => after = Some(cursor),
            _ => {
                return Ok(SearchResult {
                    viewer: viewer.login,
                    pull_requests,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RESPONSE: &str = r#"{
      "data": {
        "viewer": { "login": "ferris" },
        "search": {
          "pageInfo": { "hasNextPage": true, "endCursor": "cursor-2" },
          "nodes": [
            {
              "number": 12345,
              "title": "Fix off-by-one in scanner",
              "url": "https://github.com/rust-lang/rust/pull/12345",
              "isDraft": false,
              "author": { "login": "ferris" },
              "headRefName": "ferris/fix-scanner",
              "baseRefName": "master",
              "reviewDecision": "APPROVED",
              "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": "SUCCESS" } } } ] },
              "mergeQueueEntry": { "position": 2, "state": "AWAITING_CHECKS" }
            },
            {
              "number": 12350,
              "title": "WIP: refactor lexer",
              "url": "https://github.com/rust-lang/rust/pull/12350",
              "isDraft": true,
              "author": null,
              "headRefName": "ferris/lexer-refactor",
              "baseRefName": "master",
              "reviewDecision": null,
              "commits": { "nodes": [ { "commit": { "statusCheckRollup": null } } ] },
              "mergeQueueEntry": null
            },
            {
              "number": 12351,
              "title": "Add new feature",
              "url": "https://github.com/rust-lang/rust/pull/12351",
              "isDraft": false,
              "author": { "login": "ferris" },
              "headRefName": "ferris/feature",
              "baseRefName": "master",
              "reviewDecision": "CHANGES_REQUESTED",
              "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": "FUTURE_STATE" } } } ] },
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
        let SearchData { viewer, search } = data.unwrap();
        assert_eq!(viewer.login, "ferris");
        assert!(search.page_info.has_next_page);
        assert_eq!(search.page_info.end_cursor.as_deref(), Some("cursor-2"));
        assert_eq!(search.nodes.len(), 3);

        let pr = &search.nodes[0];
        assert_eq!(pr.number, 12345);
        assert_eq!(pr.author.as_deref(), Some("ferris"));
        assert_eq!(pr.review_decision, Some(ReviewDecision::Approved));
        assert_eq!(pr.check_status, Some(CheckStatus::Success));
        let queue = pr.merge_queue_entry.as_ref().unwrap();
        assert_eq!(queue.position, 2);
        assert_eq!(queue.state, MergeQueueState::AwaitingChecks);

        // null author/review/checks/queue fields decode to None
        assert_eq!(search.nodes[1].author, None);
        assert_eq!(search.nodes[1].review_decision, None);
        assert_eq!(search.nodes[1].check_status, None);
        assert!(search.nodes[1].merge_queue_entry.is_none());
        assert!(search.nodes[1].is_draft);

        // unknown check and queue states degrade to Unknown instead of failing
        assert_eq!(search.nodes[2].check_status, Some(CheckStatus::Unknown));
        let state = search.nodes[2].merge_queue_entry.as_ref().unwrap().state;
        assert_eq!(state, MergeQueueState::Unknown);
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
            search_query("rust-lang", "rust", Some("ferris"), Role::Author),
            "repo:rust-lang/rust is:pr is:open author:ferris"
        );
        assert_eq!(
            search_query("rust-lang", "rust", Some("ferris"), Role::ReviewRequested),
            "repo:rust-lang/rust is:pr is:open review-requested:ferris"
        );
        // An omitted user means the token's owner.
        assert_eq!(
            search_query("rust-lang", "rust", None, Role::ReviewRequested),
            "repo:rust-lang/rust is:pr is:open review-requested:@me"
        );
    }

    #[test]
    fn cli_token_is_trimmed() {
        assert_eq!(token(Some("  ghp_test  ")).as_deref(), Some("ghp_test"));
    }
}
