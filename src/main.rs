#![cfg_attr(all(target_os = "windows", not(test)), windows_subsystem = "windows")]

#[cfg(windows)]
mod win;

const APP_NAME: &str = "AppStackClassic";
const LOGO_REVISION: &str = env!("APPSTACK_CLASSIC_LOGO_REVISION");

fn main() {
    #[cfg(windows)]
    if let Err(error) = win::run() {
        win::shell::message(None, "AppStack", &error, true);
        std::process::exit(1);
    }
    #[cfg(not(windows))]
    eprintln!("AppStack requires Windows 10 or later.");
}
