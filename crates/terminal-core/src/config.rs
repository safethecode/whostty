use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub font_size: f32,
    pub scrollback: usize,
    pub default_profile: usize,
    pub font_path: Option<PathBuf>,
    pub font_bold_path: Option<PathBuf>,
    pub font_italic_path: Option<PathBuf>,
    pub font_bold_italic_path: Option<PathBuf>,
    pub profiles: Vec<Profile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub working_directory: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        #[cfg(windows)]
        let profiles = vec![
            Profile::new("PowerShell", "powershell.exe", &["-NoLogo"]),
            Profile::new("PowerShell 7", "pwsh.exe", &["-NoLogo"]),
            Profile::new("WSL", "wsl.exe", &["--cd", "~"]),
            Profile::new("Command Prompt", "cmd.exe", &[]),
        ];
        #[cfg(not(windows))]
        let profiles = vec![Profile::new(
            "Shell",
            &std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()),
            &["-l"],
        )];
        Self {
            font_size: 15.0,
            scrollback: 10_000,
            default_profile: 0,
            font_path: None,
            font_bold_path: None,
            font_italic_path: None,
            font_bold_italic_path: None,
            profiles,
        }
    }
}

impl Profile {
    pub fn new(name: &str, program: &str, args: &[&str]) -> Self {
        Self {
            name: name.into(),
            program: program.into(),
            args: args.iter().map(|arg| (*arg).into()).collect(),
            working_directory: None,
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Whostty")
            .join("config.toml")
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text =
            std::fs::read_to_string(path).with_context(|| format!("Read {}", path.display()))?;
        let settings: Self = toml::from_str(&text).context("Invalid config.toml")?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        if !self.font_size.is_finite() || !(8.0..=40.0).contains(&self.font_size) {
            bail!("font_size must be between 8 and 40");
        }
        if self.scrollback > 100_000 {
            bail!("scrollback must be at most 100000");
        }
        if self.profiles.is_empty() || self.default_profile >= self.profiles.len() {
            bail!("default_profile must refer to an existing profile");
        }
        for profile in &self.profiles {
            if profile.name.trim().is_empty() || profile.program.trim().is_empty() {
                bail!("Each profile needs a name and program");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_round_trip_and_invalid_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Whostty/config.toml");
        let mut settings = Settings::default();
        settings.save(&path).unwrap();
        assert_eq!(
            Settings::load(&path).unwrap().profiles[0].program,
            settings.profiles[0].program
        );
        settings.default_profile = 99;
        assert!(settings.save(&path).is_err());
    }

    #[test]
    fn malformed_existing_configuration_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "font_size = 'bad'").unwrap();
        assert!(Settings::load(&path).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "font_size = 'bad'");
    }
}
