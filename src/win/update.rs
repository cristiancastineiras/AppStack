//! Updates from the project's GitHub releases. Installed copies run the new installer;
//! any other copy (the portable executable) swaps its own file and restarts.

use super::shell::{failure, wide};
use serde::Deserialize;
use std::{
    ffi::c_void,
    path::{Path, PathBuf},
    process::Command,
};
use windows::{
    Win32::Networking::WinHttp::*,
    core::{PCWSTR, w},
};

const REPOSITORY: &str = "cristiancastineiras/AppStack";
const SETUP: &str = "AppStackClassic-setup.exe";
const PORTABLE: &str = "AppStackClassic-portable.exe";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

pub struct Release {
    pub version: String,
    pub notes: String,
    url: String,
    size: u64,
}

#[derive(Deserialize)]
struct Latest {
    tag_name: String,
    #[serde(default)]
    body: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// The installer leaves its uninstaller beside the executable; portable copies have none.
fn installed(executable: &Path) -> bool {
    executable.with_file_name("Uninstall.exe").is_file()
}

/// The newer release this copy can install, if GitHub has one with its asset uploaded.
pub fn latest() -> Result<Option<Release>, String> {
    let executable = std::env::current_exe().map_err(failure)?;
    let body = get(
        &format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"),
        1 << 22,
    )?;
    let latest: Latest = serde_json::from_slice(&body).map_err(failure)?;
    Ok(newer(latest, installed(&executable)))
}

fn newer(latest: Latest, installed: bool) -> Option<Release> {
    let version = latest.tag_name.trim_start_matches('v');
    if parse(version)? <= parse(CURRENT)? {
        return None;
    }
    let name = if installed { SETUP } else { PORTABLE };
    // Assets upload after the release is published; until then there is nothing to install.
    let asset = latest.assets.into_iter().find(|asset| asset.name == name)?;
    asset
        .browser_download_url
        .starts_with(&format!(
            "https://github.com/{REPOSITORY}/releases/download/"
        ))
        .then(|| Release {
            version: version.to_string(),
            notes: summary(&latest.body),
            url: asset.browser_download_url,
            size: asset.size,
        })
}

fn parse(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split('.').map(|part| part.parse().ok());
    let parsed = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(parsed)
}

/// The changelog's bullet points without Markdown emphasis or commit links.
fn summary(body: &str) -> String {
    body.lines()
        .filter_map(|line| line.trim().strip_prefix("* "))
        .take(8)
        .map(|line| {
            let text = line.split(" ([").next().unwrap_or(line);
            format!("• {}", text.replace("**", ""))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Downloads the release and returns the command to run once AppStack has exited.
pub fn prepare(release: &Release, data_dir: &Path) -> Result<Command, String> {
    let executable = std::env::current_exe().map_err(failure)?;
    let bytes = get(&release.url, release.size as usize)?;
    if bytes.len() as u64 != release.size {
        return Err("The update download was incomplete".into());
    }
    if installed(&executable) {
        let setup = std::env::temp_dir().join(SETUP);
        std::fs::write(&setup, bytes).map_err(failure)?;
        let mut command = Command::new(setup);
        command.args(["/S", "/RUN"]);
        return Ok(command);
    }
    let sibling = |suffix: &str| -> PathBuf {
        let mut name = executable.file_name().unwrap_or_default().to_os_string();
        name.push(suffix);
        executable.with_file_name(name)
    };
    let (staged, old) = (sibling(".new"), sibling(".old"));
    std::fs::write(&staged, bytes).map_err(failure)?;
    let _ = std::fs::remove_file(&old);
    // Windows lets a running executable be renamed, though not replaced.
    std::fs::rename(&executable, &old).map_err(failure)?;
    if let Err(error) = std::fs::rename(&staged, &executable) {
        let _ = std::fs::rename(&old, &executable);
        return Err(failure(error));
    }
    let mut command = Command::new(executable);
    command.arg("--data-dir").arg(data_dir);
    Ok(command)
}

/// Removes the executable a previous portable update left behind.
pub fn clean_up(executable: &Path) {
    let mut name = executable.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    let _ = std::fs::remove_file(executable.with_file_name(name));
}

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

fn get(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let (host, path) = url
        .strip_prefix("https://")
        .and_then(|rest| rest.split_once('/'))
        .ok_or("Updates are only downloaded over HTTPS")?;
    let open = |handle: *mut c_void| {
        if handle.is_null() {
            Err(failure(windows::core::Error::from_thread()))
        } else {
            Ok(Handle(handle))
        }
    };
    unsafe {
        let session = open(WinHttpOpen(
            PCWSTR(wide(format!("AppStackClassic/{CURRENT}")).as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        let connection = open(WinHttpConnect(
            session.0,
            PCWSTR(wide(host).as_ptr()),
            INTERNET_DEFAULT_HTTPS_PORT,
            0,
        ))?;
        let request = open(WinHttpOpenRequest(
            connection.0,
            w!("GET"),
            PCWSTR(wide(format!("/{path}")).as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(failure)?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(failure)?;
        let (mut status, mut length) = (0u32, 4u32);
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut length,
            std::ptr::null_mut(),
        )
        .map_err(failure)?;
        if status != 200 {
            return Err(format!("GitHub answered HTTP {status}"));
        }
        let mut body = Vec::new();
        let mut chunk = vec![0u8; 1 << 16];
        loop {
            let mut read = 0;
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                &mut read,
            )
            .map_err(failure)?;
            if read == 0 {
                return Ok(body);
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() > limit {
                return Err("The download is larger than expected".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, assets: &[(&str, &str)]) -> Latest {
        Latest {
            tag_name: tag.into(),
            body: "## [9.0.0](https://github.com/x) (2026-10-29)\n\n\n### Features\n\n* **popup:** hand cursor ([abc1234](https://github.com/x/commit/abc1234))\n* auto-update\n".into(),
            assets: assets
                .iter()
                .map(|(name, url)| Asset {
                    name: (*name).into(),
                    browser_download_url: (*url).into(),
                    size: 1,
                })
                .collect(),
        }
    }

    #[test]
    fn only_newer_releases_with_this_copys_asset_from_this_repository_are_offered() {
        let url = format!("https://github.com/{REPOSITORY}/releases/download/v9.0.0/{PORTABLE}");
        let offered = newer(release("v9.0.0", &[(PORTABLE, &url)]), false).unwrap();
        assert_eq!(offered.version, "9.0.0");
        assert_eq!(offered.notes, "• popup: hand cursor\n• auto-update");
        assert!(newer(release("v9.0.0", &[(PORTABLE, &url)]), true).is_none());
        assert!(newer(release(&format!("v{CURRENT}"), &[(PORTABLE, &url)]), false).is_none());
        assert!(newer(release("v0.9.0", &[(PORTABLE, &url)]), false).is_none());
        assert!(newer(release("v9.0.0-beta", &[(PORTABLE, &url)]), false).is_none());
        let elsewhere = format!("https://example.com/{PORTABLE}");
        assert!(newer(release("v9.0.0", &[(PORTABLE, &elsewhere)]), false).is_none());
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(parse("1.10.0") > parse("1.9.9"));
        assert_eq!(parse("1.2"), None);
        assert_eq!(parse("1.2.3.4"), None);
    }
}
