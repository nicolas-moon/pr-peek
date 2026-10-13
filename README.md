# pr-peek

A fast, opinionated CLI for checking who has open pull requests where — no browser tabs required.

Point it at a repo and a GitHub username — or just run it inside a checkout — and get back a clean table of every open PR that person has in flight: number, title, branch flow, link, draft status, review decision, CI checks, and merge queue position. Flip it around with `--reviewing` to see the PRs waiting on their review instead.

```
$ pr-peek rust-lang rust ferris

Open PRs from ferris in rust-lang/rust:

╭────────┬───────────────────────────┬────────────────────────────────┬──────────────────────────────────────────────┬───────┬───────────────────┬───────────┬────────────────────╮
│ number │ title                     │ branch                         │ link                                         │ draft │ review            │ checks    │ queue              │
├────────┼───────────────────────────┼────────────────────────────────┼──────────────────────────────────────────────┼───────┼───────────────────┼───────────┼────────────────────┤
│ #12345 │ Fix off-by-one in scanner │ ferris/fix-scanner → master    │ https://github.com/rust-lang/rust/pull/12345 │       │ ✓ approved        │ ✓ passing │ #2 awaiting checks │
│ #12350 │ WIP: refactor lexer       │ ferris/lexer-refactor → master │ https://github.com/rust-lang/rust/pull/12350 │ ✓     │ ○ review required │ ✗ failing │                    │
╰────────┴───────────────────────────┴────────────────────────────────┴──────────────────────────────────────────────┴───────┴───────────────────┴───────────┴────────────────────╯
```

## Features

- **Review status** — shows GitHub's overall review decision for each PR: `✓ approved`, `○ review required`, or `✗ changes requested`. Blank when the repo has no required-review rule and nobody has reviewed yet.
- **CI checks** — the combined status of each PR's latest commit: `✓ passing`, `✗ failing`, or `● pending`. Blank when the commit has no checks or statuses.
- **Merge queue position** — if a PR is sitting in a [merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue), shows its position and state (`#3 queued`, `#1 awaiting checks`, `#1 mergeable`, `unmergeable`, `locked`).
- **Review queue** — `--reviewing` lists open PRs waiting on the user's review (requested directly or through a team), with an `author` column.
- **Smart defaults** — run it inside a checkout and it picks the repo from your git remote; leave out the user and it shows your own PRs.
- **Zero-config auth** — reuses your `gh` CLI token automatically, or set `GITHUB_TOKEN` yourself.
- **Handles pagination** — walks every page of results, so it works on repos with hundreds of open PRs.
- **Case-insensitive matching** — `ferris`, `Ferris`, and `FERRIS` all match the same user.
- **Readable output** — a clean, aligned table instead of raw JSON, with color and clickable links in a terminal.
- **Self-update** — `pr-peek update` fetches the latest release, verifies its published checksum, and replaces the binary in place.

## Install

### Prebuilt binaries

Each [GitHub release](https://github.com/nicolas-moon/pr-peek/releases) ships archives for
macOS (Apple Silicon and Intel) and Linux (x86_64 and aarch64), plus an installer script:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/nicolas-moon/pr-peek/releases/latest/download/pr-peek-installer.sh | sh
```

Or download the `pr-peek-<target>.tar.xz` for your platform from the release page and put the
binary somewhere on your `PATH`.

### From source

Requires [Rust](https://www.rust-lang.org/tools/install) (2024 edition toolchain).

```sh
git clone https://github.com/nicolas-moon/pr-peek.git
cd pr-peek
cargo install --path .
```

This installs the `pr-peek` binary to `~/.cargo/bin` (make sure that's on your `PATH`).

### Updating

An installed binary can update itself:

```sh
pr-peek update
```

This checks the [latest GitHub release](https://github.com/nicolas-moon/pr-peek/releases), verifies the archive against its published SHA-256 checksum, and replaces the running binary in place. No token is needed (the releases API is public). It works for binaries installed with the installer script or `cargo install`; if the binary lives in a directory you don't own (e.g. `/usr/local/bin`), re-run with `sudo`.

## Usage

```sh
pr-peek [<owner> <repo>] [<user>]
```

For example:

```sh
pr-peek rust-lang rust ferris   # ferris's PRs in rust-lang/rust
pr-peek rust-lang rust          # your PRs in rust-lang/rust
pr-peek ferris                  # ferris's PRs in the current checkout's repo
pr-peek                         # your PRs in the current checkout's repo
```

Without `<owner> <repo>`, pr-peek reads the repository from the current directory's git remotes: `upstream` first (in a fork, that's where your PRs live), then `origin`. The remote must point at github.com. Without `<user>`, it lists PRs for the account that owns the token.

Add `--reviewing` (`-r`) to list the open PRs awaiting the user's review instead of the ones they opened:

```sh
pr-peek -r                      # PRs waiting on your review here
pr-peek rust-lang rust ferris -r
```

Add `--json` for machine-readable output (the raw PR data as pretty-printed JSON, including author, review, CI check, and merge queue fields, plus a `role` of `author` or `reviewRequested`) — handy for scripts and pipelines:

```sh
pr-peek rust-lang rust ferris --json
```

### Authentication

pr-peek uses GitHub's GraphQL API (the only place review decisions and merge queue state are exposed), which requires authentication. Credentials are looked up in this order:

1. `--token` flag
2. `GITHUB_TOKEN` environment variable
3. `gh auth token`, if you have the [GitHub CLI](https://cli.github.com/) installed and logged in

If none are found, pr-peek exits with an error telling you how to fix it.

```sh
pr-peek rust-lang rust ferris --token ghp_xxxxxxxxxxxx
```

For private repos, the token needs `repo` scope (classic) or read access to pull requests (fine-grained).

## Why

Checking "does so-and-so have any PRs open on this repo right now" usually means opening GitHub, filtering by author, and squinting at a list. pr-peek turns that into a single command you can run before standup, in a script, or as part of a review-rotation check (`pr-peek -r`).

## Tern plugin

If you use the [Tern](https://github.com/stencil-hq/tern-sdk) terminal, this repo ships a plugin that renders pr-peek's output natively instead of as plain text: a styled table of the PRs with toned review, CI check, and merge-queue cells (plus an author column for `--reviewing`), summary chips, and clickable `#N` and `open ↗` links.

From a checkout of this repo (the plugin lives in `tern/`):

```sh
tern plugin link /path/to/pr-peek/tern
tern plugin reload
```

The plugin hooks shell spawning and sets `PR_PEEK_JSON=1` in every new Tern pane, so pr-peek run in those panes automatically prints the JSON the lens renders — no changes to your shell environment needed. Panes opened before the plugin was loaded don't have the variable; run `pr-peek … --json` there instead (or `export PR_PEEK_JSON=1`). If the output ever isn't JSON — an older binary, an error message — the lens steps aside and shows the raw text.

## Releasing

Releases are built by [dist](https://axodotdev.github.io/cargo-dist/) via `.github/workflows/release.yml`. Pushing a tag like `v0.2.0` builds binaries for every target in `dist-workspace.toml`, creates a GitHub release, and attaches the archives, checksums, and installer script.

```sh
# 1. bump `version` in Cargo.toml, commit, push
# 2. tag and push (fails if the tag doesn't match Cargo.toml)
just release-tag 0.2.0
```

The workflow's plan step also runs on pull requests, so a broken release config surfaces before you tag. After editing `dist-workspace.toml`, run `just dist-generate` to refresh the workflow file.

macOS binaries are signed with a Developer ID certificate using the `CODESIGN_IDENTITY`, `CODESIGN_CERTIFICATE`, and `CODESIGN_CERTIFICATE_PASSWORD` repository secrets; if those are missing the release still builds but ships an unsigned macOS binary.

## License

Released under the [MIT License](LICENSE).
