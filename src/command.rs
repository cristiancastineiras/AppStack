use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum Action {
    #[default]
    Manager,
    Silent,
    Open(String),
    Edit(String),
    LaunchAll(String),
    Quit,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Request {
    pub action: Action,
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub taskbar: Option<(i32, i32)>,
}

#[derive(Default)]
pub struct Options {
    pub request: Request,
    pub portable: bool,
    pub data_dir: Option<PathBuf>,
    pub position: Option<(i32, i32)>,
    pub check: bool,
    pub list: bool,
    pub help: bool,
}

impl Options {
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--group" => {
                    options.request.action = Action::Open(
                        arguments
                            .next()
                            .ok_or("--group requires an identifier or name")?,
                    )
                }
                "--edit" => {
                    options.request.action = Action::Edit(
                        arguments
                            .next()
                            .ok_or("--edit requires an identifier or name")?,
                    )
                }
                "--launch-all" => {
                    options.request.action = Action::LaunchAll(
                        arguments
                            .next()
                            .ok_or("--launch-all requires an identifier or name")?,
                    )
                }
                "--silent" => options.request.action = Action::Silent,
                "--quit" => options.request.action = Action::Quit,
                "--portable" => options.portable = true,
                "--position" => {
                    let value = arguments.next().ok_or("--position requires X,Y")?;
                    let (left, top) = value.split_once(',').ok_or("--position requires X,Y")?;
                    options.position = Some((
                        left.parse().map_err(|_| "Invalid X coordinate")?,
                        top.parse().map_err(|_| "Invalid Y coordinate")?,
                    ));
                }
                "--data-dir" => {
                    options.data_dir = Some(PathBuf::from(
                        arguments.next().ok_or("--data-dir requires a directory")?,
                    ))
                }
                "--check" => options.check = true,
                "--list" => options.list = true,
                "--help" | "-h" | "--version" => options.help = true,
                _ if !argument.starts_with('-') => options.request.action = Action::Open(argument),
                _ => return Err(format!("Unknown option: {argument}")),
            }
        }
        Ok(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_requests_keep_their_explicit_anchor() {
        let request: Request =
            serde_json::from_str(r#"{"action":"Silent","x":123,"y":456}"#).unwrap();
        assert!(request.taskbar.is_none());
        assert_eq!((request.x, request.y), (123, 456));
    }

    #[test]
    fn commands_preserve_group_names_and_data_directories_with_spaces() {
        let options = Options::parse(
            ["--group", "Work tools", "--data-dir", "C:\\User data"].map(str::to_string),
        )
        .unwrap();
        assert!(matches!(options.request.action, Action::Open(name) if name == "Work tools"));
        assert_eq!(options.data_dir.unwrap(), PathBuf::from("C:\\User data"));
    }

    #[test]
    fn incomplete_and_unknown_commands_are_errors() {
        assert!(Options::parse(["--group".into()]).is_err());
        assert!(Options::parse(["--unknown".into()]).is_err());
    }

    #[test]
    fn explicit_anchor_accepts_negative_monitor_coordinates() {
        let options = Options::parse(["--position", "-1500,800"].map(str::to_string)).unwrap();
        assert_eq!(options.position, Some((-1500, 800)));
    }
}
