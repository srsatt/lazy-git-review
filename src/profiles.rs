use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use globset::Glob;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, Result};

const PROFILE_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GitHubProfile {
    pub name: String,
    pub repository_patterns: Vec<String>,
    pub github_command: Vec<String>,
    pub expected_account: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_host: Option<String>,
}

impl GitHubProfile {
    pub fn command(&self) -> Vec<OsString> {
        self.github_command.iter().map(OsString::from).collect()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileStore {
    pub version: u32,
    pub profiles: Vec<GitHubProfile>,
}

impl Default for ProfileStore {
    fn default() -> Self {
        Self {
            version: PROFILE_VERSION,
            profiles: Vec::new(),
        }
    }
}

impl ProfileStore {
    pub fn default_path() -> Result<PathBuf> {
        ProjectDirs::from("dev", "lazy-git-review", "lgr")
            .map(|directories| directories.config_dir().join("profiles.json"))
            .ok_or_else(|| AppError::InvalidInput {
                code: "config_directory_unavailable",
                message: "could not determine per-user application configuration directory".into(),
            })
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let store: Self = serde_json::from_slice(&fs::read(path)?).map_err(|error| {
            invalid(
                "invalid_profile_config",
                format!("{} is not valid profile JSON: {error}", path.display()),
            )
        })?;
        store.validate()?;
        Ok(store)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let parent = path.parent().ok_or_else(|| {
            invalid(
                "invalid_profile_path",
                format!("{} has no parent directory", path.display()),
            )
        })?;
        fs::create_dir_all(parent)?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                invalid(
                    "invalid_profile_path",
                    format!("{} has no UTF-8 file name", path.display()),
                )
            })?;
        let temporary = parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&serde_json::to_vec_pretty(self)?)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    pub fn upsert(&mut self, profile: GitHubProfile) -> Result<()> {
        validate_profile(&profile)?;
        if let Some(existing) = self
            .profiles
            .iter_mut()
            .find(|existing| existing.name == profile.name)
        {
            *existing = profile;
        } else {
            self.profiles.push(profile);
            self.profiles
                .sort_by(|left, right| left.name.cmp(&right.name));
        }
        Ok(())
    }

    pub fn resolve(
        &self,
        host: &str,
        repository: &str,
        explicit_profile: Option<&str>,
    ) -> Result<&GitHubProfile> {
        self.validate()?;
        if let Some(name) = explicit_profile {
            return self
                .profiles
                .iter()
                .find(|profile| profile.name == name)
                .ok_or_else(|| {
                    invalid(
                        "unknown_profile",
                        format!("GitHub profile {name:?} does not exist"),
                    )
                });
        }

        let target = format!("{host}/{repository}");
        let matches: Vec<_> = self
            .profiles
            .iter()
            .filter(|profile| {
                profile.repository_patterns.iter().any(|pattern| {
                    Glob::new(pattern)
                        .expect("validated profile glob")
                        .compile_matcher()
                        .is_match(&target)
                })
            })
            .collect();
        match matches.as_slice() {
            [profile] => Ok(profile),
            [] => Err(invalid(
                "profile_not_matched",
                format!(
                    "no GitHub profile matches {target}; add one with `lgr profile set` or pass --profile"
                ),
            )),
            profiles => Err(invalid(
                "ambiguous_profile",
                format!(
                    "multiple GitHub profiles match {target}: {}; narrow their patterns or pass --profile",
                    profiles
                        .iter()
                        .map(|profile| profile.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != PROFILE_VERSION {
            return Err(invalid(
                "unsupported_profile_version",
                format!(
                    "profile config version {} is unsupported; expected {PROFILE_VERSION}",
                    self.version
                ),
            ));
        }
        let mut names = BTreeSet::new();
        for profile in &self.profiles {
            validate_profile(profile)?;
            if !names.insert(&profile.name) {
                return Err(invalid(
                    "duplicate_profile",
                    format!(
                        "GitHub profile {:?} is defined more than once",
                        profile.name
                    ),
                ));
            }
        }
        Ok(())
    }
}

fn validate_profile(profile: &GitHubProfile) -> Result<()> {
    if profile.name.trim().is_empty() {
        return Err(invalid("invalid_profile", "profile name cannot be empty"));
    }
    if profile.repository_patterns.is_empty() {
        return Err(invalid(
            "invalid_profile",
            format!(
                "profile {:?} needs at least one repository pattern",
                profile.name
            ),
        ));
    }
    for pattern in &profile.repository_patterns {
        Glob::new(pattern).map_err(|error| {
            invalid(
                "invalid_profile_pattern",
                format!("profile {:?} pattern {pattern:?}: {error}", profile.name),
            )
        })?;
    }
    if profile.github_command.is_empty()
        || profile.github_command.iter().any(|part| part.is_empty())
    {
        return Err(invalid(
            "invalid_profile",
            format!(
                "profile {:?} needs a non-empty GitHub command",
                profile.name
            ),
        ));
    }
    if profile.expected_account.trim().is_empty() {
        return Err(invalid(
            "invalid_profile",
            format!("profile {:?} needs an expected account", profile.name),
        ));
    }
    if profile
        .api_host
        .as_ref()
        .is_some_and(|host| host.trim().is_empty())
    {
        return Err(invalid(
            "invalid_profile",
            format!("profile {:?} API host cannot be empty", profile.name),
        ));
    }
    Ok(())
}

fn invalid(code: &'static str, message: impl Into<String>) -> AppError {
    AppError::InvalidInput {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_one_profile_and_rejects_ambiguous_matches() {
        let mut store = ProfileStore::default();
        store
            .upsert(GitHubProfile {
                name: "primary".into(),
                repository_patterns: vec!["github.example.test/example/*".into()],
                github_command: vec!["gh-primary".into()],
                expected_account: "reviewer".into(),
                api_host: None,
            })
            .unwrap();
        assert_eq!(
            store
                .resolve("github.example.test", "example/tool", None)
                .unwrap()
                .name,
            "primary"
        );
        store
            .upsert(GitHubProfile {
                name: "overlap".into(),
                repository_patterns: vec!["github.example.test/example/*".into()],
                github_command: vec!["other-gh".into()],
                expected_account: "other".into(),
                api_host: None,
            })
            .unwrap();
        assert_eq!(
            store
                .resolve("github.example.test", "example/tool", None)
                .unwrap_err()
                .code(),
            "ambiguous_profile"
        );
        assert_eq!(
            store
                .resolve("github.example.test", "example/tool", Some("primary"))
                .unwrap()
                .command(),
            vec![OsString::from("gh-primary")]
        );
    }

    #[test]
    fn persists_profiles_without_secrets() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profiles.json");
        let mut store = ProfileStore::default();
        store
            .upsert(GitHubProfile {
                name: "work".into(),
                repository_patterns: vec!["github.example.test/company/*".into()],
                github_command: vec!["gh-work".into(), "--hostname".into()],
                expected_account: "employee".into(),
                api_host: Some("github.example.test".into()),
            })
            .unwrap();
        store.save(&path).unwrap();
        assert_eq!(ProfileStore::load(&path).unwrap(), store);
        assert!(!fs::read_to_string(path).unwrap().contains("token"));
    }
}
