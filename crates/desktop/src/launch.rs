use std::{ffi::OsString, path::PathBuf};
use whostty_core::config::{Profile, Settings};

pub const HELP: &str = "Whostty — a native Windows terminal\n\nUsage: whostty [OPTIONS] [-e PROGRAM [ARGS...]]\n\n  -d, --working-directory PATH  Start in this directory\n  -p, --profile NAME            Select a configured profile\n      --config PATH             Use this configuration file\n  -e, --command PROGRAM [...]   Run a program without shell evaluation\n      -- PROGRAM [...]         End options and run a program\n      --check                   Check terminal startup and rendering\n  -h, --help                    Show help\n  -V, --version                 Show version\n\nCommand and directory overrides apply to the initial tab.\nNew tabs continue to use configured profiles.\n";

#[derive(Debug, Default)]
pub struct Launch {
    pub config: Option<PathBuf>,
    pub directory: Option<PathBuf>,
    pub profile: Option<String>,
    pub command: Option<(String, Vec<String>)>,
    pub check: bool,
}

#[derive(Debug)]
pub enum Action {
    Run(Launch),
    Help,
    Version,
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Action, String> {
    let mut args = args.into_iter();
    let mut launch = Launch::default();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(Action::Help),
            Some("-V" | "--version") => return Ok(Action::Version),
            Some("--check") => launch.check = true,
            Some("--config") => {
                launch.config = Some(args.next().ok_or("--config needs a path")?.into())
            }
            Some("-d" | "--working-directory") => {
                let path = PathBuf::from(args.next().ok_or("--working-directory needs a path")?);
                if !path.is_dir() {
                    return Err(format!("Not a directory: {}", path.display()));
                }
                launch.directory = Some(
                    path.canonicalize()
                        .map_err(|error| format!("Open directory {}: {error}", path.display()))?,
                );
            }
            Some("-p" | "--profile") => {
                launch.profile = Some(unicode(args.next().ok_or("--profile needs a name")?)?)
            }
            Some("-e" | "--command" | "--") => {
                let program = unicode(args.next().ok_or("--command needs a program")?)?;
                if program.trim().is_empty() {
                    return Err("The command program cannot be empty".into());
                }
                launch.command = Some((program, args.map(unicode).collect::<Result<_, _>>()?));
                break;
            }
            _ => {
                return Err(format!(
                    "Unknown option: {}. Use --help for usage.",
                    arg.to_string_lossy()
                ))
            }
        }
    }
    if launch.check
        && (launch.config.is_some()
            || launch.directory.is_some()
            || launch.profile.is_some()
            || launch.command.is_some())
    {
        return Err("--check is isolated and cannot be combined with launch overrides".into());
    }
    Ok(Action::Run(launch))
}

fn unicode(value: OsString) -> Result<String, String> {
    value
        .into_string()
        .map_err(|_| "Profile names and command arguments must be valid Unicode".into())
}

impl Launch {
    pub fn initial_profile(&self, settings: &Settings) -> Result<Profile, String> {
        let mut profile = if let Some(name) = &self.profile {
            settings
                .profiles
                .iter()
                .find(|profile| &profile.name == name)
                .ok_or_else(|| format!("Unknown profile: {name}"))?
        } else {
            &settings.profiles[settings.default_profile]
        }
        .clone();
        if let Some((program, args)) = &self.command {
            profile.name = std::path::Path::new(program)
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new(program))
                .to_string_lossy()
                .into_owned();
            profile.shell_integration = false;
            profile.program.clone_from(program);
            profile.args.clone_from(args);
        }
        if let Some(directory) = &self.directory {
            profile.working_directory = Some(directory.clone());
        }
        Ok(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn command_boundary_preserves_flags_spaces_empty_strings_and_shell_characters() {
        let Action::Run(launch) = parse(args(&[
            "--config",
            "한글 config.toml",
            "-e",
            "my program",
            "--help",
            "",
            "a b",
            "$(do-not-evaluate);&|",
        ]))
        .unwrap() else {
            panic!("Expected launch")
        };
        assert_eq!(launch.config, Some(PathBuf::from("한글 config.toml")));
        assert_eq!(
            launch.command,
            Some((
                "my program".into(),
                vec![
                    "--help".into(),
                    "".into(),
                    "a b".into(),
                    "$(do-not-evaluate);&|".into()
                ]
            ))
        );
        assert!(matches!(
            parse(args(&["--", "tool", "-d"])).unwrap(),
            Action::Run(_)
        ));
    }

    #[test]
    fn launch_overrides_do_not_modify_saved_profiles() {
        let settings = Settings::default();
        let original = settings.profiles[settings.default_profile].clone();
        let Action::Run(launch) = parse(args(&[
            "-p",
            &original.name,
            "-d",
            ".",
            "-e",
            "custom",
            "arg",
        ]))
        .unwrap() else {
            panic!("Expected launch")
        };
        let profile = launch.initial_profile(&settings).unwrap();
        assert_eq!(profile.program, "custom");
        assert_eq!(
            profile.working_directory,
            Some(std::env::current_dir().unwrap().canonicalize().unwrap())
        );
        assert_eq!(
            settings.profiles[settings.default_profile].program,
            original.program
        );
        assert_eq!(
            settings.profiles[settings.default_profile].working_directory,
            original.working_directory
        );
        assert!(Launch {
            profile: Some("unknown-profile".into()),
            ..Default::default()
        }
        .initial_profile(&settings)
        .is_err());
    }

    #[test]
    fn malformed_options_fail_without_opening_a_window_or_a_shell() {
        for values in [
            vec!["--bad"],
            vec!["-e"],
            vec!["--config"],
            vec!["--profile"],
            vec!["-d"],
            vec!["-d", "Cargo.toml"],
            vec!["-e", ""],
        ] {
            assert!(parse(args(&values)).is_err(), "Accepted {values:?}");
        }
        assert!(matches!(parse(args(&["--help"])).unwrap(), Action::Help));
        assert!(matches!(
            parse(args(&["--version"])).unwrap(),
            Action::Version
        ));
        assert!(matches!(
            parse(args(&["--check"])).unwrap(),
            Action::Run(Launch { check: true, .. })
        ));
        assert!(parse(args(&["--check", "--profile", "PowerShell"])).is_err());
    }
}
