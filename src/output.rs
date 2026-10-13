//! Rendering a search result as a terminal table or as JSON.

use anyhow::{Context, Result};
use owo_colors::{OwoColorize, Style as ColorStyle};
use serde::Serialize;
use std::{env, io::IsTerminal};
use tabled::{
    builder::Builder,
    settings::{Color, Style, Width, object::Rows, peaker::Priority, themes::Colorization},
};

use crate::github::{
    CheckStatus, MergeQueueEntry, MergeQueueState, PullRequest, ReviewDecision, Role,
};

/// Everything printed for one run; also the shape of `--json` output.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report<'a> {
    pub owner: &'a str,
    pub repo: &'a str,
    pub user: &'a str,
    pub role: Role,
    pub pull_requests: &'a [PullRequest],
}

/// Machine-readable mode: the raw data, no styling. An empty result still
/// prints valid JSON (with an empty list).
pub fn print_json(report: &Report) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(report).context("failed to encode JSON output")?
    );
    Ok(())
}

pub fn print_table(report: &Report) {
    let Report {
        owner,
        repo,
        user,
        role,
        pull_requests,
    } = report;

    let subject = match role {
        Role::Author => format!("from {user}"),
        Role::ReviewRequested => format!("awaiting review from {user}"),
    };

    if pull_requests.is_empty() {
        println!("No open PRs {subject} in {owner}/{repo}.");
        return;
    }

    println!("Open PRs {subject} in {owner}/{repo}:\n");

    let is_tty = std::io::stdout().is_terminal();
    let use_color = is_tty && env::var_os("NO_COLOR").is_none();

    let palette = if use_color {
        Palette::colored()
    } else {
        Palette::plain()
    };

    // The author is the point of a review listing; otherwise it is the user.
    let show_author = *role == Role::ReviewRequested;

    let mut builder = Builder::default();
    builder.push_record(
        ["number", "title"]
            .into_iter()
            .chain(show_author.then_some("author"))
            .chain(["branch", "link", "draft", "review", "checks", "queue"]),
    );

    for pr in *pull_requests {
        let mut row = vec![
            format!("#{}", pr.number).style(palette.number).to_string(),
            pr.title.clone(),
        ];
        if show_author {
            row.push(pr.author.as_deref().unwrap_or("ghost").to_owned());
        }
        row.extend([
            format!(
                "{} {} {}",
                pr.head_ref_name.style(palette.head),
                "→".style(palette.arrow),
                pr.base_ref_name.style(palette.base)
            ),
            // In a terminal, show a compact clickable label; otherwise
            // print the full URL so pipes and logs stay useful.
            if is_tty {
                hyperlink(&pr.url, "open ↗").style(palette.link).to_string()
            } else {
                pr.url.clone()
            },
            if pr.is_draft {
                "✓".style(palette.draft).to_string()
            } else {
                String::new()
            },
            palette.review_cell(pr.review_decision),
            palette.checks_cell(pr.check_status),
            palette.queue_cell(pr.merge_queue_entry.as_ref()),
        ]);
        builder.push_record(row);
    }

    let mut table = builder.build();
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
    // Semantic status styles shared by the review, checks, and queue columns.
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

    fn checks_cell(&self, status: Option<CheckStatus>) -> String {
        let (label, style) = match status {
            Some(CheckStatus::Success) => ("✓ passing", self.ok),
            Some(CheckStatus::Failure | CheckStatus::Error) => ("✗ failing", self.blocked),
            Some(CheckStatus::Pending | CheckStatus::Expected) => ("● pending", self.pending),
            Some(CheckStatus::Unknown) => ("? unknown", self.pending),
            None => return String::new(),
        };
        label.style(style).to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn checks_cells_group_states() {
        let palette = Palette::plain();
        assert!(palette.checks_cell(None).is_empty());
        assert_eq!(palette.checks_cell(Some(CheckStatus::Success)), "✓ passing");
        assert_eq!(palette.checks_cell(Some(CheckStatus::Failure)), "✗ failing");
        assert_eq!(palette.checks_cell(Some(CheckStatus::Error)), "✗ failing");
        assert_eq!(palette.checks_cell(Some(CheckStatus::Pending)), "● pending");
        assert_eq!(
            palette.checks_cell(Some(CheckStatus::Expected)),
            "● pending"
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
            author: Some("ferris".to_owned()),
            head_ref_name: "head".to_owned(),
            base_ref_name: "base".to_owned(),
            review_decision: None,
            check_status: Some(CheckStatus::Failure),
            merge_queue_entry: None,
        };

        let out = Report {
            owner: "owner",
            repo: "repo",
            user: "user",
            role: Role::ReviewRequested,
            pull_requests: std::slice::from_ref(&pr),
        };
        let rendered = serde_json::to_string(&out).unwrap();

        assert!(rendered.contains(r#""role":"reviewRequested""#));
        assert!(rendered.contains(r#""pullRequests""#));
        assert!(rendered.contains(r#""author":"ferris""#));
        assert!(rendered.contains(r#""headRefName":"head""#));
        assert!(rendered.contains(r#""reviewDecision":null"#));
        assert!(rendered.contains(r#""checkStatus":"FAILURE""#));
    }
}
