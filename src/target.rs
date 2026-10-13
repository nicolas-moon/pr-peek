//! Which repository and user to query: from the command line, falling back
//! to the current checkout's GitHub remote and the token's owner.

use anyhow::Result;
use std::process::Command;

#[derive(Debug, PartialEq, Eq)]
pub struct Target {
    pub owner: String,
    pub repo: String,
    /// `None` means the token's owner.
    pub user: Option<String>,
}

/// Map the positional arguments onto a target:
///
/// | args                     | repo           | user         |
/// |--------------------------|----------------|--------------|
/// | (none)                   | `infer_repo()` | token owner  |
/// | `<user>`                 | `infer_repo()` | `<user>`     |
/// | `<owner> <repo>`         | as given       | token owner  |
/// | `<owner> <repo> <user>`  | as given       | `<user>`     |
///
/// `infer_repo` only runs when the repository was not given.
pub fn resolve(
    args: Vec<String>,
    infer_repo: impl FnOnce() -> Result<(String, String)>,
) -> Result<Target> {
    let mut args = args.into_iter();
    let (owner, repo, user) = match (args.next(), args.next(), args.next(), args.next()) {
        (None, ..) => {
            let (owner, repo) = infer_repo()?;
            (owner, repo, None)
        }
        (Some(user), None, ..) => {
            let (owner, repo) = infer_repo()?;
            (owner, repo, Some(user))
        }
        (Some(owner), Some(repo), user, None) => (owner, repo, user),
        _ => anyhow::bail!("expected at most three arguments: [<owner> <repo>] [<user>]"),
    };
    Ok(Target { owner, repo, user })
}

/// Remotes consulted for the current repository, in order. `upstream`
/// comes first because in a fork checkout that is where PRs are opened.
const REMOTES: [&str; 2] = ["upstream", "origin"];

/// The GitHub `(owner, repo)` of the git checkout in the working directory.
pub fn repo_from_git_remote() -> Result<(String, String)> {
    for remote in REMOTES {
        let Ok(output) = Command::new("git")
            .args(["remote", "get-url", remote])
            .output()
        else {
            break; // git is not installed
        };

        if !output.status.success() {
            continue;
        }

        if let Some(repo) = std::str::from_utf8(&output.stdout)
            .ok()
            .and_then(|url| parse_github_remote(url.trim()))
        {
            return Ok(repo);
        }
    }

    anyhow::bail!(
        "couldn't tell which repository to use: no `upstream` or `origin` remote points at github.com here; pass <owner> <repo>"
    )
}

/// `(owner, repo)` from a github.com remote URL in any of git's forms:
/// `git@github.com:o/r.git`, `ssh://git@github.com/o/r`,
/// `https://[user@]github.com/o/r.git`, `git://github.com/o/r`.
fn parse_github_remote(url: &str) -> Option<(String, String)> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.split_once('@').map_or(rest, |(_, rest)| rest);
    let path = rest.strip_prefix("github.com")?.strip_prefix([':', '/'])?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);

    let (owner, repo) = path.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some((owner.to_owned(), repo.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn inferred() -> Result<(String, String)> {
        Ok(("here".to_owned(), "checkout".to_owned()))
    }

    fn not_inferred() -> Result<(String, String)> {
        panic!("the repository was given; nothing should be inferred")
    }

    fn target(owner: &str, repo: &str, user: Option<&str>) -> Target {
        Target {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            user: user.map(str::to_owned),
        }
    }

    #[test]
    fn positional_arguments_map_by_count() {
        assert_eq!(
            resolve(args(&[]), inferred).unwrap(),
            target("here", "checkout", None)
        );
        assert_eq!(
            resolve(args(&["ferris"]), inferred).unwrap(),
            target("here", "checkout", Some("ferris"))
        );
        assert_eq!(
            resolve(args(&["rust-lang", "rust"]), not_inferred).unwrap(),
            target("rust-lang", "rust", None)
        );
        assert_eq!(
            resolve(args(&["rust-lang", "rust", "ferris"]), not_inferred).unwrap(),
            target("rust-lang", "rust", Some("ferris"))
        );
        assert!(resolve(args(&["a", "b", "c", "d"]), not_inferred).is_err());
    }

    #[test]
    fn parses_github_remote_forms() {
        let expected = Some(("rust-lang".to_owned(), "rust".to_owned()));
        for url in [
            "git@github.com:rust-lang/rust.git",
            "git@github.com:rust-lang/rust",
            "ssh://git@github.com/rust-lang/rust.git",
            "https://github.com/rust-lang/rust.git",
            "https://github.com/rust-lang/rust",
            "https://github.com/rust-lang/rust/",
            "https://ferris@github.com/rust-lang/rust.git",
            "git://github.com/rust-lang/rust.git",
        ] {
            assert_eq!(parse_github_remote(url), expected, "{url}");
        }
    }

    #[test]
    fn rejects_non_github_remotes() {
        for url in [
            "git@gitlab.com:rust-lang/rust.git",
            "https://github.company.com/rust-lang/rust.git",
            "https://github.com/rust-lang",
            "https://github.com/rust-lang/rust/tree/master",
            "/home/ferris/rust",
        ] {
            assert_eq!(parse_github_remote(url), None, "{url}");
        }
    }
}
