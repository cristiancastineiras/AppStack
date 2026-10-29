use crate::model::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const MAX_CONFIG: u64 = 16 * 1024 * 1024;
const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;
/// Data folder holding the shortcut files that drops on a popup moved out of their folders.
pub const ITEMS: &str = "items";

#[derive(Clone, Debug)]
pub struct Store {
    pub root: PathBuf,
}

impl Store {
    pub fn discover(portable: bool, explicit: Option<PathBuf>) -> Result<Self, String> {
        let executable = std::env::current_exe().map_err(error)?;
        let directory = executable.parent().ok_or("Executable has no directory")?;
        let root = explicit.unwrap_or_else(|| {
            if portable || directory.join("portable.txt").exists() {
                directory.join("config")
            } else {
                std::env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(std::env::temp_dir)
                    .join("AppStack")
            }
        });
        fs::create_dir_all(&root).map_err(error)?;
        Ok(Self {
            root: root.canonicalize().map_err(error)?,
        })
    }

    pub fn load(&self) -> Result<Config, String> {
        let path = self.root.join("appstack.json");
        if !path.exists() {
            return Ok(Config::default());
        }
        let config: Config =
            serde_json::from_slice(&read_limited(&path, MAX_CONFIG)?).map_err(error)?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, config: &Config) -> Result<(), String> {
        config.validate()?;
        fs::create_dir_all(&self.root).map_err(error)?;
        let path = self.root.join("appstack.json");
        let temporary = self.root.join(format!("{}.tmp", new_id()));
        let operation = (|| -> Result<(), String> {
            let mut file = File::create(&temporary).map_err(error)?;
            file.write_all(&serde_json::to_vec_pretty(config).map_err(error)?)
                .map_err(error)?;
            file.sync_all().map_err(error)?;
            drop(file);
            if path.exists() {
                fs::copy(&path, self.root.join("appstack.json.bak")).map_err(error)?;
            }
            replace_file(&temporary, &path)
        })();
        if operation.is_err() {
            let _ = fs::remove_file(temporary);
        }
        operation
    }

    pub fn import(&self, path: &Path) -> Result<Vec<Group>, String> {
        if path.is_dir() {
            if path.join("appstack.json").exists() {
                return self.import(&path.join("appstack.json"));
            }
            let mut groups = import_taskbar_groups(path)?;
            self.adopt_icons(&mut groups)?;
            return Ok(groups);
        }
        if path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("agz") || extension.eq_ignore_ascii_case("zip")
        }) {
            return self.import_archive(path);
        }
        let mut groups = decode_groups(&read_limited(path, MAX_CONFIG)?)?;
        resolve_relative_icons(&mut groups, path.parent().unwrap_or(Path::new(".")));
        self.adopt_icons(&mut groups)?;
        Ok(groups)
    }

    fn import_archive(&self, path: &Path) -> Result<Vec<Group>, String> {
        let mut archive = ZipArchive::new(File::open(path).map_err(error)?).map_err(error)?;
        if archive.len() > 20_000 {
            return Err("Archive has too many entries".into());
        }
        let mut total = 0u64;
        for index in 0..archive.len() {
            let entry = archive.by_index(index).map_err(error)?;
            total = total
                .checked_add(entry.size())
                .ok_or("Archive is too large")?;
            if total > MAX_ARCHIVE
                || entry.enclosed_name().is_none()
                || entry.name().contains('\\')
                || entry.name().contains(':')
                || entry
                    .unix_mode()
                    .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err("Archive contains an unsafe path or exceeds 256 MiB".into());
            }
        }
        let config_name = "appstack.json";
        if !archive.file_names().any(|entry| entry == config_name) {
            return Err("Archive has no AppStack configuration".into());
        }
        let bytes = {
            let mut entry = archive.by_name(config_name).map_err(error)?;
            let mut bytes = Vec::new();
            (&mut entry)
                .take(MAX_CONFIG + 1)
                .read_to_end(&mut bytes)
                .map_err(error)?;
            if bytes.len() as u64 > MAX_CONFIG {
                return Err("Configuration is too large".into());
            }
            bytes
        };
        let mut groups = decode_groups(&bytes)?;
        let destination = self.root.join("assets").join(new_id());
        fs::create_dir_all(&destination).map_err(error)?;
        let extraction = (|| -> Result<(), String> {
            let mut extracted = Vec::new();
            let mut actual_total = 0u64;
            for index in 0..archive.len() {
                let mut entry = archive.by_index(index).map_err(error)?;
                let relative = entry.enclosed_name().ok_or("Invalid archive path")?;
                if entry.is_dir() {
                    continue;
                }
                let output = destination.join(&relative);
                fs::create_dir_all(output.parent().ok_or("Invalid asset path")?).map_err(error)?;
                let mut file = File::create(&output).map_err(error)?;
                actual_total += std::io::copy(
                    &mut (&mut entry).take(MAX_ARCHIVE - actual_total + 1),
                    &mut file,
                )
                .map_err(error)?;
                if actual_total > MAX_ARCHIVE {
                    return Err("Archive exceeds extraction limit".into());
                }
                extracted.push((relative.to_string_lossy().replace('\\', "/"), output));
            }
            for group in &mut groups {
                remap_asset(&mut group.appearance.icon, &extracted);
                for entry in &mut group.entries {
                    remap_asset(&mut entry.icon, &extracted);
                    if let Target::Application(path) = &mut entry.target {
                        if path.starts_with(&format!("{ITEMS}/")) {
                            remap_asset(path, &extracted);
                        }
                    }
                }
            }
            Ok(())
        })();
        if extraction.is_err() {
            let _ = fs::remove_dir_all(&destination);
        }
        extraction?;
        Ok(groups)
    }

    pub fn adopt_icons(&self, groups: &mut [Group]) -> Result<(), String> {
        let directory = self.root.join("assets").join(new_id());
        let mut copied = HashMap::new();
        for group in groups {
            for icon in std::iter::once(&mut group.appearance.icon)
                .chain(group.entries.iter_mut().map(|entry| &mut entry.icon))
            {
                let source = PathBuf::from(&*icon);
                if icon.is_empty() || !source.is_file() || source.starts_with(&self.root) {
                    continue;
                }
                if let Some(previous) = copied.get(&source) {
                    *icon = String::clone(previous);
                    continue;
                }
                let extension = source
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("ico");
                if !["ico", "png", "jpg", "jpeg", "bmp"]
                    .iter()
                    .any(|value| extension.eq_ignore_ascii_case(value))
                {
                    continue;
                }
                fs::create_dir_all(&directory).map_err(error)?;
                let output = directory.join(format!("{}.{}", new_id(), extension));
                fs::copy(&source, &output).map_err(error)?;
                *icon = output.to_string_lossy().into_owned();
                copied.insert(source, icon.clone());
            }
        }
        Ok(())
    }

    pub fn export(&self, config: &Config, path: &Path) -> Result<(), String> {
        config.validate()?;
        let mut portable = config.clone();
        let temporary = path.with_extension(format!("{}.tmp", new_id()));
        let operation = (|| -> Result<(), String> {
            let mut archive = ZipWriter::new(File::create(&temporary).map_err(error)?);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            let mut resources = HashMap::new();
            let items = self.root.join(ITEMS).canonicalize().ok();
            for group in &mut portable.groups {
                // Shortcuts moved into AppStack exist nowhere else, so the backup carries them.
                for entry in &mut group.entries {
                    let Target::Application(path) = &mut entry.target else {
                        continue;
                    };
                    let source = PathBuf::from(&*path);
                    let Some(relative) = items.as_ref().and_then(|items| {
                        let canonical = source.canonicalize().ok()?;
                        Some(canonical.strip_prefix(items).ok()?.to_path_buf())
                    }) else {
                        continue;
                    };
                    if !source.is_file() {
                        continue;
                    }
                    if let Some(name) = resources.get(&source) {
                        *path = String::clone(name);
                        continue;
                    }
                    let name = format!("{ITEMS}/{}", relative.to_string_lossy().replace('\\', "/"));
                    archive.start_file(&name, options).map_err(error)?;
                    archive
                        .write_all(&read_limited(&source, MAX_CONFIG)?)
                        .map_err(error)?;
                    resources.insert(source, name.clone());
                    *path = name;
                }
                for icon in std::iter::once(&mut group.appearance.icon)
                    .chain(group.entries.iter_mut().map(|entry| &mut entry.icon))
                {
                    let source = PathBuf::from(&*icon);
                    if !source.is_file() {
                        continue;
                    }
                    let extension = source
                        .extension()
                        .and_then(|value| value.to_str())
                        .unwrap_or("ico");
                    if !["ico", "png", "jpg", "jpeg", "bmp"]
                        .iter()
                        .any(|value| extension.eq_ignore_ascii_case(value))
                    {
                        continue;
                    }
                    if let Some(name) = resources.get(&source) {
                        *icon = String::clone(name);
                        continue;
                    }
                    let name = format!("assets/{}.{}", new_id(), extension);
                    archive.start_file(&name, options).map_err(error)?;
                    archive
                        .write_all(&read_limited(&source, MAX_CONFIG)?)
                        .map_err(error)?;
                    resources.insert(source, name.clone());
                    *icon = name;
                }
            }
            archive
                .start_file("appstack.json", options)
                .map_err(error)?;
            archive
                .write_all(&serde_json::to_vec_pretty(&portable).map_err(error)?)
                .map_err(error)?;
            archive.finish().map_err(error)?.sync_all().map_err(error)?;
            replace_file(&temporary, path)
        })();
        if operation.is_err() {
            let _ = fs::remove_file(temporary);
        }
        operation
    }
}

fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            },
            core::PCWSTR,
        };
        let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        unsafe {
            MoveFileExW(
                PCWSTR(source.as_ptr()),
                PCWSTR(destination.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(error)
    }
    #[cfg(not(windows))]
    fs::rename(source, destination).map_err(error)
}

fn read_limited(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(error)?;
    if file.metadata().map_err(error)?.len() > maximum {
        return Err("File is too large".into());
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() as u64 > maximum {
        return Err("File is too large".into());
    }
    Ok(bytes)
}

fn error(value: impl std::fmt::Display) -> String {
    value.to_string()
}

pub fn decode_groups(bytes: &[u8]) -> Result<Vec<Group>, String> {
    let value: Value =
        serde_json::from_slice(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes))
            .map_err(error)?;
    if value.get("schema_version").is_none() {
        return Err("Not an AppStack configuration".into());
    }
    let config: Config = serde_json::from_value(value).map_err(error)?;
    config.validate()?;
    Ok(config.groups)
}

pub fn display_name(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let last = normalized
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(path);
    let name = last.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(last);
    if name.is_empty() {
        path.to_string()
    } else {
        name.to_string()
    }
}

fn resolve_relative_icons(groups: &mut [Group], parent: &Path) {
    for group in groups {
        for icon in std::iter::once(&mut group.appearance.icon)
            .chain(group.entries.iter_mut().map(|entry| &mut entry.icon))
        {
            if !icon.is_empty() && Path::new(&*icon).is_relative() {
                *icon = parent.join(&*icon).to_string_lossy().into_owned();
            }
        }
    }
}

fn remap_asset(value: &mut String, extracted: &[(String, PathBuf)]) {
    if value.is_empty() {
        return;
    }
    let normalized = value.replace('\\', "/").to_lowercase();
    if let Some((_, output)) = extracted.iter().find(|(relative, _)| {
        let relative = relative.to_lowercase();
        normalized == relative || normalized.ends_with(&format!("/{relative}"))
    }) {
        *value = output.to_string_lossy().into_owned();
    }
}

pub fn selected_with_dependencies(groups: &[Group], selected: &HashSet<String>) -> Vec<Group> {
    let mut included = selected.clone();
    let mut stack: Vec<_> = selected.iter().cloned().collect();
    while let Some(id) = stack.pop() {
        if let Some(group) = groups.iter().find(|group| group.id == id) {
            for entry in &group.entries {
                if let Target::Group(child) = &entry.target {
                    if included.insert(child.clone()) {
                        stack.push(child.clone());
                    }
                }
            }
        }
    }
    groups
        .iter()
        .filter(|group| included.contains(&group.id))
        .cloned()
        .collect()
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct TbgCategory {
    #[serde(rename = "n")]
    name: String,
    #[serde(rename = "Width")]
    width: usize,
    #[serde(rename = "ShortcutList")]
    shortcuts: TbgShortcuts,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct TbgShortcuts {
    #[serde(rename = "ProgramShortcut")]
    entries: Vec<TbgShortcut>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct TbgShortcut {
    #[serde(rename = "n")]
    name: String,
    #[serde(rename = "FilePath")]
    path: String,
    #[serde(rename = "Arguments")]
    arguments: String,
}

fn import_taskbar_groups(root: &Path) -> Result<Vec<Group>, String> {
    let mut folders = vec![root.to_path_buf()];
    if root.join("config").is_dir() {
        folders.push(root.join("config"));
    }
    for entry in fs::read_dir(root).map_err(error)?.take(1000) {
        let path = entry.map_err(error)?.path();
        if path.is_dir() {
            folders.push(path.clone());
            if path.join("config").is_dir() {
                folders.push(path.join("config"));
            }
        }
    }
    let mut files = HashSet::new();
    for folder in folders {
        if folder.join("ObjectData.xml").exists() {
            files.insert(folder.join("ObjectData.xml"));
        }
        for entry in fs::read_dir(&folder).map_err(error)?.take(1000) {
            let path = entry.map_err(error)?.path().join("ObjectData.xml");
            if path.is_file() {
                files.insert(path);
            }
        }
    }
    let mut files: Vec<_> = files.into_iter().collect();
    files.sort();
    let mut groups = Vec::new();
    for file in files {
        let category: TbgCategory =
            quick_xml::de::from_reader(read_limited(&file, MAX_CONFIG)?.as_slice())
                .map_err(error)?;
        let folder = file.parent().ok_or("Invalid TaskbarGroups folder")?;
        let mut group = Group {
            name: if category.name.trim().is_empty() {
                display_name(&folder.to_string_lossy())
            } else {
                category.name
            },
            ..Group::default()
        };
        group.appearance.columns = category.width.clamp(1, 16);
        for name in ["GroupIcon.ico", "GroupImage.png"] {
            if folder.join(name).exists() {
                group.appearance.icon = folder.join(name).to_string_lossy().into_owned();
                group.appearance.grid_icon = false;
                break;
            }
        }
        for shortcut in category.shortcuts.entries {
            if shortcut.path.trim().is_empty() {
                continue;
            }
            let name = if shortcut.name.trim().is_empty() {
                display_name(&shortcut.path)
            } else {
                shortcut.name
            };
            let icon = folder.join("Icons").join(format!("{name}.png"));
            group.entries.push(Entry {
                name,
                target: Target::Application(shortcut.path),
                arguments: shortcut.arguments,
                icon: if icon.is_file() {
                    icon.to_string_lossy().into_owned()
                } else {
                    String::new()
                },
                ..Entry::default()
            });
        }
        groups.push(group);
    }
    if groups.is_empty() {
        return Err("No AppStack configuration or TaskbarGroups ObjectData.xml files found".into());
    }
    Config {
        groups: groups.clone(),
        ..Config::default()
    }
    .validate()?;
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary() -> Store {
        let root = std::env::temp_dir().join(format!("appstack-test-{}", new_id()));
        fs::create_dir_all(&root).unwrap();
        Store { root }
    }

    #[test]
    fn atomic_saves_keep_the_previous_configuration() {
        let store = temporary();
        let mut config = Config::default();
        store.save(&config).unwrap();
        config.groups.push(Group::default());
        store.save(&config).unwrap();
        assert_eq!(store.load().unwrap().groups.len(), 1);
        let previous: Config =
            serde_json::from_slice(&fs::read(store.root.join("appstack.json.bak")).unwrap())
                .unwrap();
        assert!(previous.groups.is_empty());
        fs::write(store.root.join("appstack.json"), "not json").unwrap();
        assert!(store.load().is_err());
        fs::remove_dir_all(store.root).unwrap();
    }

    #[test]
    fn archive_round_trip_preserves_icons() {
        let store = temporary();
        let icon = store.root.join("example.ico");
        fs::write(&icon, b"icon bytes").unwrap();
        let mut group = Group::default();
        group.appearance.icon = icon.to_string_lossy().into_owned();
        let config = Config {
            groups: vec![group],
            ..Config::default()
        };
        let archive = store.root.join("backup.agz");
        store.export(&config, &archive).unwrap();
        let groups = store.import(&archive).unwrap();
        assert_eq!(fs::read(&groups[0].appearance.icon).unwrap(), b"icon bytes");
        fs::remove_dir_all(store.root).unwrap();
    }

    #[test]
    fn archive_round_trip_carries_shortcuts_moved_into_items() {
        let store = temporary();
        fs::create_dir_all(store.root.join(ITEMS)).unwrap();
        let moved = store.root.join(ITEMS).join("Editor.lnk");
        fs::write(&moved, b"shortcut bytes").unwrap();
        let elsewhere = store.root.join("Elsewhere.lnk");
        fs::write(&elsewhere, b"other").unwrap();
        let entry = |path: &Path| Entry {
            name: "Editor".into(),
            target: Target::Application(path.to_string_lossy().into_owned()),
            ..Entry::default()
        };
        let group = Group {
            entries: vec![entry(&moved), entry(&elsewhere)],
            ..Group::default()
        };
        let config = Config {
            groups: vec![group],
            ..Config::default()
        };
        let archive = store.root.join("backup.agz");
        store.export(&config, &archive).unwrap();
        let groups = store.import(&archive).unwrap();
        let Target::Application(restored) = &groups[0].entries[0].target else {
            panic!("expected an application");
        };
        assert_ne!(Path::new(restored), moved);
        assert_eq!(fs::read(restored).unwrap(), b"shortcut bytes");
        assert_eq!(
            groups[0].entries[1].target,
            config.groups[0].entries[1].target
        );
        fs::remove_dir_all(store.root).unwrap();
    }

    #[test]
    fn zip_traversal_is_rejected_before_extraction() {
        let store = temporary();
        let path = store.root.join("bad.agz");
        let mut archive = ZipWriter::new(File::create(&path).unwrap());
        archive
            .start_file("../outside.txt", SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"bad").unwrap();
        archive.finish().unwrap();
        assert!(store.import(&path).unwrap_err().contains("unsafe"));
        fs::remove_dir_all(store.root).unwrap();
    }

    #[test]
    fn selective_import_includes_required_subgroups() {
        let child = Group::default();
        let parent = Group {
            entries: vec![Entry {
                name: "Tools".into(),
                target: Target::Group(child.id.clone()),
                ..Entry::default()
            }],
            ..Group::default()
        };
        let selected = HashSet::from([parent.id.clone()]);
        assert_eq!(
            selected_with_dependencies(&[parent, child, Group::default()], &selected).len(),
            2
        );
    }

    #[test]
    fn taskbar_groups_xml_is_imported() {
        let store = temporary();
        fs::write(store.root.join("ObjectData.xml"), r#"<Category><n>Work</n><Width>4</Width><ShortcutList><ProgramShortcut><n>Editor</n><FilePath>C:\Editor.exe</FilePath><Arguments>--new</Arguments></ProgramShortcut></ShortcutList></Category>"#).unwrap();
        let groups = store.import(&store.root).unwrap();
        assert_eq!(groups[0].name, "Work");
        assert_eq!(groups[0].entries[0].arguments, "--new");
        fs::remove_dir_all(store.root).unwrap();
    }
}
