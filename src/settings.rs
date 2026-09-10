use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, Result};
use crate::profiles::{GitHubProfile, ProfileStore};

const SETTINGS_VERSION: u32 = 3;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProfile {
    pub command: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TuiSettings {
    #[serde(default = "default_theme_name")]
    pub theme: String,
    #[serde(default)]
    pub themes: BTreeMap<String, ThemePalette>,
    #[serde(default = "default_true")]
    pub syntax_highlighting: bool,
}

impl Default for TuiSettings {
    fn default() -> Self {
        Self {
            theme: default_theme_name(),
            themes: BTreeMap::new(),
            syntax_highlighting: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewUnitSettings {
    #[serde(default = "default_partition_threshold")]
    pub threshold: usize,
    #[serde(default = "default_partition_target")]
    pub target: usize,
    #[serde(default = "default_partition_ceiling")]
    pub hard_ceiling: usize,
    #[serde(default = "default_partition_context")]
    pub context_lines: usize,
}

impl Default for ReviewUnitSettings {
    fn default() -> Self {
        Self {
            threshold: default_partition_threshold(),
            target: default_partition_target(),
            hard_ceiling: default_partition_ceiling(),
            context_lines: default_partition_context(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TestProfile {
    pub executable: PathBuf,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub prepare_argv: Vec<String>,
    #[serde(default)]
    pub prepare_executable: Option<PathBuf>,
    #[serde(default = "default_project_root")]
    pub project_root: PathBuf,
    #[serde(default = "default_test_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_test_output_limit")]
    pub max_output_bytes: usize,
    #[serde(default = "default_report_limit")]
    pub max_report_bytes: usize,
    #[serde(default = "default_report_format")]
    pub report_format: String,
    #[serde(default = "default_report_path")]
    pub report_path: PathBuf,
    #[serde(default = "default_attribution")]
    pub attribution: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThemePalette {
    pub background: String,
    pub foreground: String,
    pub muted: String,
    pub border: String,
    pub focus: String,
    pub selection: String,
    pub important: String,
    pub test: String,
    pub usage: String,
    pub definition: String,
    pub addition: String,
    pub deletion: String,
    pub warning: String,
    pub error: String,
    #[serde(default)]
    pub syntax: SyntaxPalette,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SyntaxPalette {
    pub comment: String,
    pub string: String,
    pub number: String,
    pub keyword: String,
    pub function: String,
    pub r#type: String,
    pub property: String,
    pub variable: String,
}

impl Default for SyntaxPalette {
    fn default() -> Self {
        Self {
            comment: "#637777".into(),
            string: "#ADDB67".into(),
            number: "#F78C6C".into(),
            keyword: "#C792EA".into(),
            function: "#82AAFF".into(),
            r#type: "#FFCB8B".into(),
            property: "#7FDBCA".into(),
            variable: "#D6DEEB".into(),
        }
    }
}

fn default_theme_name() -> String {
    "night-owl".into()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub data_dir: PathBuf,
    pub scripts_dir: PathBuf,
    #[serde(alias = "default_harness")]
    pub default_agent_profile: String,
    #[serde(alias = "harnesses")]
    pub agent_profiles: BTreeMap<String, AgentProfile>,
    pub github_profiles: Vec<GitHubProfile>,
    #[serde(default)]
    pub tui: TuiSettings,
    #[serde(default)]
    pub review_units: ReviewUnitSettings,
    #[serde(default)]
    pub test_profiles: BTreeMap<String, TestProfile>,
}

impl Settings {
    pub fn default_path() -> Result<PathBuf> {
        let home = std::env::var_os("HOME").ok_or_else(|| {
            invalid(
                "home_directory_unavailable",
                "HOME is not set; pass --settings with an explicit path",
            )
        })?;
        Ok(PathBuf::from(home).join(".lgr/settings.json"))
    }

    pub fn defaults_for(path: &Path) -> Result<Self> {
        let root = path.parent().ok_or_else(|| {
            invalid(
                "invalid_settings_path",
                format!("{} has no parent directory", path.display()),
            )
        })?;
        let mut agent_profiles = BTreeMap::new();
        agent_profiles.insert(
            "codex".into(),
            AgentProfile {
                command: vec![
                    "codex".into(),
                    "exec".into(),
                    "--json".into(),
                    "--sandbox".into(),
                    "read-only".into(),
                    "--ephemeral".into(),
                ],
            },
        );
        agent_profiles.insert(
            "opencode".into(),
            AgentProfile {
                command: vec!["opencode".into(), "run".into()],
            },
        );
        Ok(Self {
            version: SETTINGS_VERSION,
            data_dir: root.join("data"),
            scripts_dir: root.join("scripts"),
            default_agent_profile: "codex".into(),
            agent_profiles,
            github_profiles: ProfileStore::default().profiles,
            tui: TuiSettings::default(),
            review_units: ReviewUnitSettings::default(),
            test_profiles: BTreeMap::new(),
        })
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Self::defaults_for(path);
        }
        let mut settings: Self = serde_json::from_slice(&fs::read(path)?).map_err(|error| {
            invalid(
                "invalid_settings",
                format!("{} is not valid settings JSON: {error}", path.display()),
            )
        })?;
        if settings.version == 1 || settings.version == 2 {
            settings.version = SETTINGS_VERSION;
        }
        let root = path.parent().ok_or_else(|| {
            invalid(
                "invalid_settings_path",
                format!("{} has no parent directory", path.display()),
            )
        })?;
        if settings.data_dir.is_relative() {
            settings.data_dir = root.join(&settings.data_dir);
        }
        if settings.scripts_dir.is_relative() {
            settings.scripts_dir = root.join(&settings.scripts_dir);
        }
        settings.validate()?;
        Ok(settings)
    }

    pub fn initialize(path: &Path) -> Result<Self> {
        let settings = Self::load(path)?;
        settings.ensure_layout()?;
        if !path.exists() {
            settings.save(path)?;
        }
        Ok(settings)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let parent = path.parent().ok_or_else(|| {
            invalid(
                "invalid_settings_path",
                format!("{} has no parent directory", path.display()),
            )
        })?;
        fs::create_dir_all(parent)?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                invalid(
                    "invalid_settings_path",
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

    pub fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(&self.data_dir)?;
        fs::create_dir_all(&self.scripts_dir)?;
        Ok(())
    }

    pub fn selected_agent_profile(&self) -> Result<(&str, &AgentProfile)> {
        self.agent_profiles
            .get_key_value(&self.default_agent_profile)
            .map(|(name, profile)| (name.as_str(), profile))
            .ok_or_else(|| {
                invalid(
                    "unknown_default_agent_profile",
                    format!(
                        "default agent profile {:?} is not configured",
                        self.default_agent_profile
                    ),
                )
            })
    }

    pub fn profile_store(&self) -> ProfileStore {
        ProfileStore {
            version: 1,
            profiles: self.github_profiles.clone(),
        }
    }

    pub fn set_profiles(&mut self, profiles: ProfileStore) {
        self.github_profiles = profiles.profiles;
    }

    fn validate(&self) -> Result<()> {
        if self.version != SETTINGS_VERSION {
            return Err(invalid(
                "unsupported_settings_version",
                format!(
                    "settings version {} is unsupported; expected {SETTINGS_VERSION}",
                    self.version
                ),
            ));
        }
        if self.data_dir.as_os_str().is_empty() || self.scripts_dir.as_os_str().is_empty() {
            return Err(invalid(
                "invalid_settings",
                "data_dir and scripts_dir cannot be empty",
            ));
        }
        if self.data_dir == self.scripts_dir {
            return Err(invalid(
                "invalid_settings",
                "data_dir and scripts_dir must be different directories",
            ));
        }
        for (name, profile) in &self.agent_profiles {
            if name.trim().is_empty()
                || profile.command.is_empty()
                || profile.command.iter().any(|part| part.is_empty())
            {
                return Err(invalid(
                    "invalid_agent_profile",
                    format!("agent profile {name:?} needs a name and non-empty command argv"),
                ));
            }
        }
        self.selected_agent_profile()?;
        self.profile_store().validate()?;
        if !crate::tui::theme::is_builtin(&self.tui.theme)
            && !self.tui.themes.contains_key(&self.tui.theme)
        {
            return Err(invalid(
                "unknown_tui_theme",
                format!(
                    "TUI theme {:?} is not built in or declared in tui.themes",
                    self.tui.theme
                ),
            ));
        }
        for (name, palette) in &self.tui.themes {
            if name.trim().is_empty()
                || !palette.colors().iter().all(|value| valid_hex_color(value))
            {
                return Err(invalid(
                    "invalid_tui_theme",
                    format!("TUI theme {name:?} requires non-empty name and #RRGGBB colors"),
                ));
            }
        }
        if self.review_units.target == 0
            || self.review_units.target > self.review_units.hard_ceiling
            || self.review_units.hard_ceiling > self.review_units.threshold
            || self.review_units.context_lines > 20
        {
            return Err(invalid(
                "invalid_review_unit_settings",
                "review unit target must be positive and no greater than hard_ceiling/threshold; context_lines max is 20",
            ));
        }
        for (name, profile) in &self.test_profiles {
            if name.trim().is_empty()
                || profile.executable.as_os_str().is_empty()
                || profile.timeout_seconds == 0
                || profile.max_output_bytes == 0
                || profile.max_output_bytes > 8 * 1024 * 1024
                || profile.max_report_bytes == 0
                || profile.max_report_bytes > 64 * 1024 * 1024
                || !matches!(
                    profile.report_format.as_str(),
                    "manifest-v1" | "istanbul-json" | "lcov"
                )
                || !matches!(profile.attribution.as_str(), "suite" | "file")
                || profile.project_root.is_absolute()
                || profile.report_path.is_absolute()
                || profile
                    .project_root
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
                || profile
                    .report_path
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
            {
                return Err(invalid(
                    "invalid_test_profile",
                    format!(
                        "test profile {name:?} has invalid command, limits, timeout, or report format"
                    ),
                ));
            }
        }
        Ok(())
    }
}

fn default_true() -> bool {
    true
}
fn default_partition_threshold() -> usize {
    120
}
fn default_partition_target() -> usize {
    80
}
fn default_partition_ceiling() -> usize {
    120
}
fn default_partition_context() -> usize {
    3
}
fn default_project_root() -> PathBuf {
    PathBuf::from(".")
}
fn default_test_timeout() -> u64 {
    600
}
fn default_test_output_limit() -> usize {
    8 * 1024 * 1024
}
fn default_report_limit() -> usize {
    64 * 1024 * 1024
}
fn default_report_format() -> String {
    "istanbul-json".into()
}
fn default_report_path() -> PathBuf {
    PathBuf::from("coverage/coverage-final.json")
}
fn default_attribution() -> String {
    "suite".into()
}

impl ThemePalette {
    fn colors(&self) -> Vec<&str> {
        vec![
            &self.background,
            &self.foreground,
            &self.muted,
            &self.border,
            &self.focus,
            &self.selection,
            &self.important,
            &self.test,
            &self.usage,
            &self.definition,
            &self.addition,
            &self.deletion,
            &self.warning,
            &self.error,
            &self.syntax.comment,
            &self.syntax.string,
            &self.syntax.number,
            &self.syntax.keyword,
            &self.syntax.function,
            &self.syntax.r#type,
            &self.syntax.property,
            &self.syntax.variable,
        ]
    }
}

fn valid_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn invalid(code: &'static str, message: impl Into<String>) -> AppError {
    AppError::InvalidInput {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn defaults_to_generic_agent_profiles_and_sibling_layout() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".lgr/settings.json");
        let settings = Settings::defaults_for(&path).unwrap();
        assert_eq!(settings.data_dir, directory.path().join(".lgr/data"));
        assert_eq!(settings.scripts_dir, directory.path().join(".lgr/scripts"));
        assert_eq!(
            settings.selected_agent_profile().unwrap().1.command,
            vec![
                "codex",
                "exec",
                "--json",
                "--sandbox",
                "read-only",
                "--ephemeral"
            ]
        );
        assert_eq!(
            settings.agent_profiles["opencode"].command,
            vec!["opencode", "run"]
        );
        assert_eq!(settings.tui.theme, "night-owl");
    }

    #[test]
    fn initializes_private_json_and_resolves_relative_directories() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".lgr/settings.json");
        let initialized = Settings::initialize(&path).unwrap();
        assert!(initialized.data_dir.is_dir());
        assert!(initialized.scripts_dir.is_dir());
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let mut relative = initialized;
        relative.data_dir = "state".into();
        relative.scripts_dir = "commands".into();
        relative.save(&path).unwrap();
        let loaded = Settings::load(&path).unwrap();
        assert_eq!(loaded.data_dir, directory.path().join(".lgr/state"));
        assert_eq!(loaded.scripts_dir, directory.path().join(".lgr/commands"));
    }

    #[test]
    fn loads_version_one_harness_fields_as_agent_profiles() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(
            &path,
            r#"{
                "version": 1,
                "data_dir": "data",
                "scripts_dir": "scripts",
                "default_harness": "legacy",
                "harnesses": {"legacy": {"command": ["legacy-agent"]}},
                "github_profiles": []
            }"#,
        )
        .unwrap();

        let settings = Settings::load(&path).unwrap();
        assert_eq!(settings.version, SETTINGS_VERSION);
        assert_eq!(settings.default_agent_profile, "legacy");
        assert_eq!(
            settings.agent_profiles["legacy"].command,
            vec!["legacy-agent"]
        );
        assert_eq!(settings.tui, TuiSettings::default());
        assert_eq!(settings.review_units, ReviewUnitSettings::default());
        assert!(settings.test_profiles.is_empty());
    }

    #[test]
    fn loads_version_two_custom_palette_without_syntax_roles() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(
            &path,
            r##"{
                "version": 2,
                "data_dir": "data",
                "scripts_dir": "scripts",
                "default_agent_profile": "codex",
                "agent_profiles": {"codex": {"command": ["codex"]}},
                "github_profiles": [],
                "tui": {
                    "theme": "custom",
                    "themes": {"custom": {
                        "background":"#000000","foreground":"#ffffff","muted":"#777777",
                        "border":"#888888","focus":"#00ffff","selection":"#005555",
                        "important":"#ffff00","test":"#00ff00","usage":"#00ffff",
                        "definition":"#ff00ff","addition":"#00ff00","deletion":"#ff0000",
                        "warning":"#ffff00","error":"#ff0000"
                    }}
                }
            }"##,
        )
        .unwrap();
        let settings = Settings::load(&path).unwrap();
        assert_eq!(settings.version, SETTINGS_VERSION);
        assert_eq!(
            settings.tui.themes["custom"].syntax,
            SyntaxPalette::default()
        );
        assert!(settings.tui.syntax_highlighting);
    }

    #[test]
    fn accepts_named_custom_tui_palette_and_rejects_unknown_theme() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut settings = Settings::defaults_for(&path).unwrap();
        let palette = ThemePalette {
            background: "#000000".into(),
            foreground: "#ffffff".into(),
            muted: "#777777".into(),
            border: "#888888".into(),
            focus: "#00ffff".into(),
            selection: "#005555".into(),
            important: "#ffff00".into(),
            test: "#00ff00".into(),
            usage: "#00ffff".into(),
            definition: "#ff00ff".into(),
            addition: "#00ff00".into(),
            deletion: "#ff0000".into(),
            warning: "#ffff00".into(),
            error: "#ff0000".into(),
            syntax: SyntaxPalette::default(),
        };
        settings.tui.themes.insert("custom".into(), palette);
        settings.tui.theme = "custom".into();
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap().tui.theme, "custom");

        settings.tui.theme = "missing".into();
        assert!(settings.save(&path).is_err());
    }
}
