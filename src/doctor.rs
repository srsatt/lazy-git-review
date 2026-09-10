use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DependencyReport {
    pub name: &'static str,
    pub capability: &'static str,
    pub available: bool,
    pub path: Option<PathBuf>,
    pub setup: &'static str,
}

pub fn diagnose() -> Vec<DependencyReport> {
    diagnose_with_path(env::var_os("PATH").as_deref())
}

fn diagnose_with_path(path: Option<&OsStr>) -> Vec<DependencyReport> {
    [
        ("git", "raw_git_review", "Install Git 2.31 or newer."),
        (
            "typescript-language-server",
            "typescript_semantics",
            "Install typescript-language-server and TypeScript, then ensure both are on PATH.",
        ),
        (
            "vscode-html-language-server",
            "html_semantics",
            "Install vscode-langservers-extracted and add its binaries to PATH.",
        ),
        (
            "vscode-css-language-server",
            "css_semantics",
            "Install vscode-langservers-extracted and add its binaries to PATH.",
        ),
        (
            "nvim",
            "editor_navigation",
            "Install Neovim, Diffview, and code-review.nvim.",
        ),
        (
            "gh",
            "github_reviews",
            "Install GitHub CLI, then configure an account-bound command and expected account.",
        ),
        (
            "python3",
            "gh_dash_review",
            "Install Python 3 to use the gh-dash and YouTrack helper scripts.",
        ),
    ]
    .into_iter()
    .map(|(name, capability, setup)| {
        let found = find_executable(name, path);
        DependencyReport {
            name,
            capability,
            available: found.is_some(),
            path: found,
            setup,
        }
    })
    .collect()
}

fn find_executable(name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    env::split_paths(path?)
        .map(|directory| directory.join(name))
        .find(|candidate| is_executable(candidate))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    #[test]
    fn missing_tool_disables_only_its_capability() {
        let dir = TempDir::new().unwrap();
        let git = dir.path().join("git");
        fs::write(&git, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&git, fs::Permissions::from_mode(0o700)).unwrap();
        let reports = diagnose_with_path(Some(dir.path().as_os_str()));
        assert!(reports.iter().find(|r| r.name == "git").unwrap().available);
        assert!(!reports.iter().find(|r| r.name == "nvim").unwrap().available);
        assert!(
            reports
                .iter()
                .find(|r| r.name == "nvim")
                .unwrap()
                .setup
                .contains("Neovim")
        );
    }
}
