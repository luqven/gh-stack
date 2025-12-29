//! GitHub CLI detection and installation helpers
//!
//! Provides detection of the `gh` CLI and platform-specific installation
//! instructions for helping users create PRs.

use dialoguer::Confirm;
use std::error::Error;
use std::io::{self, IsTerminal};
use std::process::Command;

/// Check if the GitHub CLI (`gh`) is installed
pub fn is_gh_installed() -> bool {
    Command::new("gh")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if the GitHub CLI is authenticated
pub fn is_gh_authenticated() -> bool {
    Command::new("gh")
        .args(["auth", "status"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get platform-specific installation instructions for the GitHub CLI
pub fn install_instructions() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "brew install gh"
    }

    #[cfg(target_os = "linux")]
    {
        "sudo apt install gh    # Debian/Ubuntu\n  sudo dnf install gh    # Fedora"
    }

    #[cfg(target_os = "windows")]
    {
        "winget install GitHub.cli"
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        "See https://cli.github.com/manual/installation"
    }
}

/// Print a suggestion to create a PR for a branch
///
/// Shows the `gh pr create` command and, if `gh` is not installed,
/// provides installation instructions.
pub fn suggest_create_pr(head: &str, base: &str) {
    println!("No PR found for branch '{}'.\n", head);
    println!("Create a PR with:");
    println!("  gh pr create --base {} --head {}\n", base, head);

    if !is_gh_installed() {
        println!("Note: 'gh' CLI is not installed.\n");
        println!("Install with:");
        println!("  {}\n", install_instructions());
        println!("After installing, run 'gh auth login' to authenticate.");
    } else if !is_gh_authenticated() {
        println!("Note: 'gh' CLI is not authenticated.\n");
        println!("Run 'gh auth login' to authenticate.");
    }
}

/// Prompt the user to create a PR and execute if confirmed
///
/// Returns `Ok(true)` if PR was created, `Ok(false)` if user declined or
/// `gh` is not available, `Err` on failure.
///
/// In non-interactive mode (no TTY), always returns `Ok(false)`.
pub fn prompt_create_pr(head: &str, base: &str) -> Result<bool, Box<dyn Error>> {
    // Check if we're in an interactive terminal
    if !io::stdout().is_terminal() {
        suggest_create_pr(head, base);
        return Ok(false);
    }

    // Check if gh is installed
    if !is_gh_installed() {
        suggest_create_pr(head, base);
        return Ok(false);
    }

    // Check if gh is authenticated
    if !is_gh_authenticated() {
        println!("No PR found for branch '{}'.\n", head);
        println!("The 'gh' CLI is installed but not authenticated.");
        println!("Run 'gh auth login' to authenticate, then try again.");
        return Ok(false);
    }

    println!("No PR found for branch '{}'.\n", head);

    let create = Confirm::new()
        .with_prompt(format!("Create a PR from '{}' into '{}'?", head, base))
        .default(true)
        .interact()?;

    if !create {
        return Ok(false);
    }

    println!("\nCreating PR...\n");

    let status = Command::new("gh")
        .args(["pr", "create", "--base", base, "--head", head])
        .status()?;

    Ok(status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_install_instructions_not_empty() {
        let instructions = install_instructions();
        assert!(!instructions.is_empty());
        // Should contain some actionable command
        assert!(instructions.contains("install") || instructions.contains("cli.github.com"));
    }

    #[test]
    fn test_is_gh_installed_returns_bool() {
        // Just verify it returns a bool without panicking
        let _result = is_gh_installed();
    }

    #[test]
    fn test_is_gh_authenticated_returns_bool() {
        // Just verify it returns a bool without panicking
        let _result = is_gh_authenticated();
    }
}
