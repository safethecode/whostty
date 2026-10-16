use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub font_size: f32,
    pub font_ligatures: bool,
    pub scrollback: usize,
    pub default_profile: usize,
    pub inherit_working_directory: bool,
    pub font_path: Option<PathBuf>,
    pub font_bold_path: Option<PathBuf>,
    pub font_italic_path: Option<PathBuf>,
    pub font_bold_italic_path: Option<PathBuf>,
    pub theme: crate::theme::Theme,
    pub keybindings: Vec<crate::shortcuts::Binding>,
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
    #[serde(default)]
    pub shell_integration: bool,
}

impl Default for Settings {
    fn default() -> Self {
        #[cfg(windows)]
        let profiles = vec![
            Profile::powershell("PowerShell", "powershell.exe"),
            Profile::powershell("PowerShell 7", "pwsh.exe"),
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
            font_ligatures: true,
            scrollback: 10_000,
            default_profile: 0,
            inherit_working_directory: true,
            font_path: None,
            font_bold_path: None,
            font_italic_path: None,
            font_bold_italic_path: None,
            theme: Default::default(),
            keybindings: Vec::new(),
            profiles,
        }
    }
}

impl Profile {
    #[cfg(windows)]
    fn powershell(name: &str, program: &str) -> Self {
        let mut profile = Self::new(name, program, &["-NoLogo"]);
        profile.shell_integration = true;
        profile
    }

    pub fn supports_shell_integration(&self) -> bool {
        let name = Path::new(&self.program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        matches!(
            name.as_str(),
            "powershell.exe" | "powershell" | "pwsh.exe" | "pwsh"
        ) && self.args.iter().all(|arg| {
            matches!(
                arg.to_ascii_lowercase().as_str(),
                "-nologo" | "-noprofile" | "-noexit"
            )
        })
    }

    pub fn new(name: &str, program: &str, args: &[&str]) -> Self {
        Self {
            name: name.into(),
            program: program.into(),
            args: args.iter().map(|arg| (*arg).into()).collect(),
            working_directory: None,
            shell_integration: false,
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
        crate::atomic_file::write(path, toml::to_string_pretty(self)?.as_bytes())?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        crate::shortcuts::Shortcuts::new(&self.keybindings)?;
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
            if profile.shell_integration && !profile.supports_shell_integration() {
                bail!("shell_integration requires an interactive PowerShell profile with only -NoLogo, -NoProfile or -NoExit arguments");
            }
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
    fn shell_integration_accepts_only_interactive_powershell_profiles() {
        let mut settings = Settings {
            profiles: vec![Profile::new(
                "PowerShell",
                "powershell.exe",
                &["-NoLogo", "-NoProfile"],
            )],
            default_profile: 0,
            ..Settings::default()
        };
        settings.profiles[0].shell_integration = true;
        assert!(settings.validate().is_ok());
        settings.profiles[0]
            .args
            .extend(["-Command".into(), "exit 7".into()]);
        assert!(settings.validate().is_err());
        settings.profiles[0].shell_integration = false;
        assert!(settings.validate().is_ok());
        let restored: Settings = toml::from_str(&toml::to_string(&settings).unwrap()).unwrap();
        assert!(restored.inherit_working_directory);
        assert!(!restored.profiles[0].shell_integration);
    }

    #[test]
    fn malformed_existing_configuration_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "font_size = 'bad'").unwrap();
        assert!(Settings::load(&path).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "font_size = 'bad'");
    }

    #[test]
    fn repeated_saves_expose_only_complete_configs_to_readers() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut settings = Settings::default();
        settings.save(&path).unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let reader_done = done.clone();
        let reader_path = path.clone();
        let reader = std::thread::spawn(move || {
            while !reader_done.load(Ordering::Acquire) {
                let text = std::fs::read_to_string(&reader_path).unwrap();
                let settings: Settings = toml::from_str(&text).expect("Partial config was visible");
                settings.validate().unwrap();
            }
        });
        let writes = (8..24).try_for_each(|size| {
            settings.font_size = size as f32;
            settings.save(&path)
        });
        done.store(true, Ordering::Release);
        reader.join().unwrap();
        writes.unwrap();
        assert_eq!(Settings::load(&path).unwrap().font_size, 23.0);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        let original = std::fs::read(&path).unwrap();
        settings.font_size = f32::NAN;
        assert!(settings.save(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn saves_preserve_user_config_symlinks_and_unix_modes() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("managed.toml");
        let link = dir.path().join("config.toml");
        let mut settings = Settings::default();
        settings.save(&target).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&target, &link).unwrap();
        settings.font_size = 18.0;
        settings.save(&link).unwrap();
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(Settings::load(&target).unwrap().font_size, 18.0);
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[test]
    fn read_only_configuration_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut settings = Settings::default();
        settings.save(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let permissions = std::fs::metadata(&path).unwrap().permissions();
        let mut read_only = permissions.clone();
        read_only.set_readonly(true);
        std::fs::set_permissions(&path, read_only).unwrap();
        settings.font_size = 18.0;
        let result = settings.save(&path);
        std::fs::set_permissions(&path, permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}
