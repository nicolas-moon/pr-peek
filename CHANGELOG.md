# Changelog

All notable changes to pr-peek. Each section is generated from the commit history by `just changelog <version>` and may be edited by hand.

## [0.5.0] - 2026-10-13

### Added

- CI checks column: the combined status of each PR's latest commit (`✓ passing`, `✗ failing`, `● pending`) ([#3](https://github.com/nicolas-moon/pr-peek/pull/3))
- `--reviewing` / `-r` lists open PRs awaiting the user's review, with an author column ([#3](https://github.com/nicolas-moon/pr-peek/pull/3))
- `<owner> <repo>` and `<user>` are optional: the repository defaults to the current checkout's `upstream` or `origin` GitHub remote, the user to the token's owner ([#3](https://github.com/nicolas-moon/pr-peek/pull/3))
- `--json` output includes `author`, `checkStatus`, and `role` ([#3](https://github.com/nicolas-moon/pr-peek/pull/3))

## [0.4.0] - 2026-10-12

### Added

- `pr-peek update` replaces the binary with the latest release after verifying its published SHA-256 checksum ([#2](https://github.com/nicolas-moon/pr-peek/pull/2))

## [0.3.0] - 2026-10-12

### Added

- `--json` prints the raw PR data for scripts; also enabled by `PR_PEEK_JSON=1`
- Tern plugin that renders pr-peek output as a native table ([#1](https://github.com/nicolas-moon/pr-peek/pull/1))
- `--version` reports the git commit, target, and rustc version of the build ([#1](https://github.com/nicolas-moon/pr-peek/pull/1))

## [0.2.1] - 2026-10-08

### Changed

- Version bump only; no changes from 0.2.0

## [0.2.0] - 2026-10-08

### Added

- Initial release: list a GitHub user's open PRs in a repository as a table with branch, link, draft status, review decision, and merge queue position
- Authentication via `--token`, `GITHUB_TOKEN`, or the `gh` CLI
- Prebuilt, signed binaries for macOS and Linux with a shell installer
