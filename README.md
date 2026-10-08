# pr-peek

A fast, opinionated CLI for checking who has open pull requests where — no browser tabs required.

Point it at a repo and a GitHub username, and get back a clean table of every open PR that person has in flight: number, title, branch flow, link, draft status, review decision, and merge queue position.

```
$ pr-peek rust-lang rust ferris

Open PRs from ferris in rust-lang/rust:

╭────────┬───────────────────────────┬─────────────────────────────────┬──────────────────────────────────────────────┬───────┬───────────────────┬────────────────────╮
│ number │ title                     │ branch                          │ link                                         │ draft │ review            │ queue              │
├────────┼───────────────────────────┼─────────────────────────────────┼──────────────────────────────────────────────┼───────┼───────────────────┼────────────────────┤
│ #12345 │ Fix off-by-one in scanner │ ferris/fix-scanner → master     │ https://github.com/rust-lang/rust/pull/12345 │       │ ✓ approved        │ #2 awaiting checks │
│ #12350 │ WIP: refactor lexer       │ ferris/lexer-refactor → master  │ https://github.com/rust-lang/rust/pull/12350 │ ✓     │ ○ review required │                    │
╰────────┴───────────────────────────┴─────────────────────────────────┴──────────────────────────────────────────────┴───────┴───────────────────┴────────────────────╯
```

## Features

- **Review status** — shows GitHub's overall review decision for each PR: `✓ approved`, `○ review required`, or `✗ changes requested`. Blank when the repo has no required-review rule and nobody has reviewed yet.
- **Merge queue position** — if a PR is sitting in a [merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue), shows its position and state (`#3 queued`, `#1 awaiting checks`, `#1 mergeable`, `unmergeable`, `locked`).
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
pr-peek <owner> <repo> <user>
```

For example:

```sh
pr-peek rust-lang rust ferris
```

Add `--json` for machine-readable output (the raw PR data as pretty-printed JSON, including review and merge queue fields) — handy for scripts and pipelines:

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

Checking "does so-and-so have any PRs open on this repo right now" usually means opening GitHub, filtering by author, and squinting at a list. pr-peek turns that into a single command you can run before standup, in a script, or as part of a review-rotation check.

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
