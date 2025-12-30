//! Browser and URL utilities for PR creation
//!
//! Provides cross-platform browser opening and GitHub URL generation
//! for creating pull requests without depending on the `gh` CLI.

use dialoguer::Confirm;
use std::error::Error;
use std::io::IsTerminal;
use std::process::Command;

/// Extract GitHub base URL from a git remote URL
///
/// Returns the base URL (e.g., "https://github.com" or "https://github.mycompany.com")
///
/// # Examples
/// - `git@github.com:owner/repo.git` → `https://github.com`
/// - `git@github.mycompany.com:org/repo.git` → `https://github.mycompany.com`
/// - `https://github.com/owner/repo.git` → `https://github.com`
pub fn parse_github_host(remote_url: &str) -> Option<String> {
    // SSH format: git@<host>:owner/repo.git
    if remote_url.starts_with("git@") {
        let host = remote_url.strip_prefix("git@")?.split(':').next()?;
        return Some(format!("https://{}", host));
    }

    // HTTPS/HTTP format: https://<host>/owner/repo.git
    if remote_url.starts_with("https://") || remote_url.starts_with("http://") {
        let without_protocol = remote_url.split("://").nth(1)?;
        let host = without_protocol.split('/').next()?;
        let protocol = if remote_url.starts_with("https://") {
            "https"
        } else {
            "http"
        };
        return Some(format!("{}://{}", protocol, host));
    }

    None
}

/// Build GitHub PR creation URL with pre-filled branches
///
/// The URL opens GitHub's compare view with the PR creation form expanded.
pub fn build_pr_url(github_host: &str, repo: &str, base: &str, head: &str) -> String {
    format!(
        "{}/{}/compare/{}...{}?expand=1",
        github_host, repo, base, head
    )
}

/// Open URL in default browser (cross-platform)
///
/// Uses platform-specific commands:
/// - macOS: `open`
/// - Linux: `xdg-open`
/// - Windows: `cmd /C start`
pub fn open_url(url: &str) -> Result<(), Box<dyn Error>> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).status()?;
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(url).status()?;
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        return Err("Unsupported platform for opening browser".into());
    }

    Ok(())
}

/// Print URL for creating PR (non-interactive/CI mode)
pub fn suggest_create_pr(github_host: &str, repo: &str, head: &str, base: &str) {
    let url = build_pr_url(github_host, repo, base, head);
    println!("No PR found for branch '{}'.\n", head);
    println!("Create a PR at:");
    println!("  {}\n", url);
}

/// Prompt user and open browser to create PR (interactive mode)
///
/// Returns `Ok(true)` if user chose to open browser, `Ok(false)` if declined.
/// In non-interactive mode, prints the URL and returns `Ok(false)`.
pub fn prompt_create_pr(
    github_host: &str,
    repo: &str,
    head: &str,
    base: &str,
) -> Result<bool, Box<dyn Error>> {
    if !std::io::stdout().is_terminal() {
        suggest_create_pr(github_host, repo, head, base);
        return Ok(false);
    }

    let url = build_pr_url(github_host, repo, base, head);
    println!("No PR found for branch '{}'.\n", head);

    let open = Confirm::new()
        .with_prompt(format!(
            "Open browser to create PR from '{}' into '{}'?",
            head, base
        ))
        .default(true)
        .interact()?;

    if open {
        println!("\nOpening browser...");
        println!("  {}\n", url);
        open_url(&url)?;
    }

    Ok(open)
}

#[cfg(test)]
mod tests {
    use super::*;

    // === parse_github_host tests ===

    #[test]
    fn test_parse_github_host_ssh() {
        assert_eq!(
            parse_github_host("git@github.com:owner/repo.git"),
            Some("https://github.com".to_string())
        );
    }

    #[test]
    fn test_parse_github_host_ssh_no_suffix() {
        assert_eq!(
            parse_github_host("git@github.com:owner/repo"),
            Some("https://github.com".to_string())
        );
    }

    #[test]
    fn test_parse_github_host_ssh_enterprise() {
        assert_eq!(
            parse_github_host("git@github.mycompany.com:org/repo.git"),
            Some("https://github.mycompany.com".to_string())
        );
    }

    #[test]
    fn test_parse_github_host_https() {
        assert_eq!(
            parse_github_host("https://github.com/owner/repo.git"),
            Some("https://github.com".to_string())
        );
    }

    #[test]
    fn test_parse_github_host_https_no_suffix() {
        assert_eq!(
            parse_github_host("https://github.com/owner/repo"),
            Some("https://github.com".to_string())
        );
    }

    #[test]
    fn test_parse_github_host_https_enterprise() {
        assert_eq!(
            parse_github_host("https://github.mycompany.com/org/repo.git"),
            Some("https://github.mycompany.com".to_string())
        );
    }

    #[test]
    fn test_parse_github_host_http() {
        assert_eq!(
            parse_github_host("http://github.com/owner/repo.git"),
            Some("http://github.com".to_string())
        );
    }

    #[test]
    fn test_parse_github_host_invalid() {
        assert_eq!(parse_github_host("not-a-url"), None);
    }

    #[test]
    fn test_parse_github_host_empty() {
        assert_eq!(parse_github_host(""), None);
    }

    // === build_pr_url tests ===

    #[test]
    fn test_build_pr_url() {
        assert_eq!(
            build_pr_url("https://github.com", "owner/repo", "main", "feature"),
            "https://github.com/owner/repo/compare/main...feature?expand=1"
        );
    }

    #[test]
    fn test_build_pr_url_enterprise() {
        assert_eq!(
            build_pr_url(
                "https://github.mycompany.com",
                "org/repo",
                "develop",
                "my-branch"
            ),
            "https://github.mycompany.com/org/repo/compare/develop...my-branch?expand=1"
        );
    }

    #[test]
    fn test_build_pr_url_with_slashes_in_branch() {
        assert_eq!(
            build_pr_url(
                "https://github.com",
                "owner/repo",
                "main",
                "feature/my-feature"
            ),
            "https://github.com/owner/repo/compare/main...feature/my-feature?expand=1"
        );
    }
}
