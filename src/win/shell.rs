#![allow(unsafe_op_in_unsafe_fn)]

use appstack::{model::*, storage::Store};
use std::{
    mem::{ManuallyDrop, size_of},
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        Foundation::*,
        System::{
            Com::{StructuredStorage::*, *},
            Ole::*,
            Registry::*,
            Variant::VT_LPWSTR,
        },
        UI::{
            Shell::{Common::*, PropertiesSystem::*, *},
            WindowsAndMessaging::*,
        },
    },
    core::{GUID, HRESULT, Interface, PCWSTR, PWSTR, w},
};

pub fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value
        .as_ref()
        .encode_wide()
        .filter(|value| *value != 0)
        .chain(Some(0))
        .collect()
}

pub fn failure(error: impl std::fmt::Display) -> String {
    error.to_string()
}

pub struct Apartment;

impl Apartment {
    pub fn new() -> Result<Self, String> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .map_err(failure)?;
        }
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

pub struct OleApartment;

impl OleApartment {
    pub fn new() -> Result<Self, String> {
        unsafe {
            OleInitialize(None).map_err(failure)?;
        }
        Ok(Self)
    }
}

impl Drop for OleApartment {
    fn drop(&mut self) {
        unsafe {
            OleUninitialize();
        }
    }
}

pub fn message(owner: Option<HWND>, title: &str, text: &str, error: bool) {
    unsafe {
        MessageBoxW(
            owner,
            PCWSTR(wide(text).as_ptr()),
            PCWSTR(wide(title).as_ptr()),
            MB_OK
                | if error {
                    MB_ICONERROR
                } else {
                    MB_ICONINFORMATION
                },
        );
    }
}

pub fn confirm(owner: HWND, text: &str) -> bool {
    unsafe {
        MessageBoxW(
            Some(owner),
            PCWSTR(wide(text).as_ptr()),
            w!("AppStack"),
            MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
        ) == IDYES
    }
}

pub fn launch(entry: &Entry) -> Result<(), String> {
    let Target::Application(path) = &entry.target else {
        return Err("Expected an application".into());
    };
    let verb = if entry.administrator {
        w!("runas")
    } else {
        w!("open")
    };
    let directory = Path::new(path)
        .parent()
        .filter(|parent| parent.is_dir())
        .map(wide);
    let result = unsafe {
        ShellExecuteW(
            None,
            verb,
            PCWSTR(wide(path).as_ptr()),
            PCWSTR(wide(&entry.arguments).as_ptr()),
            directory
                .as_ref()
                .map(|directory| PCWSTR(directory.as_ptr()))
                .unwrap_or(PCWSTR::null()),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as usize <= 32 {
        Err(format!(
            "Windows could not open {} (Shell error {}).",
            entry.name, result.0 as usize
        ))
    } else {
        Ok(())
    }
}

pub fn open_path(path: &Path) -> Result<(), String> {
    launch(&Entry {
        name: path.to_string_lossy().into_owned(),
        target: Target::Application(path.to_string_lossy().into_owned()),
        ..Entry::default()
    })
}

pub fn select_files(owner: HWND, folder: bool, multiple: bool) -> Result<Vec<PathBuf>, String> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).map_err(failure)?;
        let mut options = FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_FILEMUSTEXIST;
        if folder {
            options |= FOS_PICKFOLDERS;
        }
        if multiple {
            options |= FOS_ALLOWMULTISELECT;
        }
        dialog.SetOptions(options).map_err(failure)?;
        if let Err(error) = dialog.Show(Some(owner)) {
            if error.code() == HRESULT::from_win32(1223) {
                return Ok(Vec::new());
            }
            return Err(failure(error));
        }
        let items = dialog.GetResults().map_err(failure)?;
        let mut paths = Vec::new();
        for index in 0..items.GetCount().map_err(failure)? {
            let item = items.GetItemAt(index).map_err(failure)?;
            paths.push(PathBuf::from(item_name(&item, SIGDN_FILESYSPATH)?));
        }
        Ok(paths)
    }
}

pub fn save_file(owner: HWND, name: &str) -> Result<Option<PathBuf>, String> {
    unsafe {
        let dialog: IFileSaveDialog =
            CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).map_err(failure)?;
        dialog
            .SetOptions(FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT | FOS_PATHMUSTEXIST)
            .map_err(failure)?;
        dialog
            .SetFileName(PCWSTR(wide(name).as_ptr()))
            .map_err(failure)?;
        dialog.SetDefaultExtension(w!("agz")).map_err(failure)?;
        let filters = [COMDLG_FILTERSPEC {
            pszName: w!("AppStack backup"),
            pszSpec: w!("*.agz"),
        }];
        dialog.SetFileTypes(&filters).map_err(failure)?;
        if let Err(error) = dialog.Show(Some(owner)) {
            if error.code() == HRESULT::from_win32(1223) {
                return Ok(None);
            }
            return Err(failure(error));
        }
        Ok(Some(PathBuf::from(item_name(
            &dialog.GetResult().map_err(failure)?,
            SIGDN_FILESYSPATH,
        )?)))
    }
}

pub unsafe fn item_name(item: &IShellItem, kind: SIGDN) -> Result<String, String> {
    let name = item.GetDisplayName(kind).map_err(failure)?;
    let result = name.to_string().map_err(failure);
    CoTaskMemFree(Some(name.0.cast()));
    result
}

const APP_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 5,
};
const TITLE: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xf29f85e0_4ff9_1068_ab91_08002b27b3d9),
    pid: 2,
};

unsafe fn set_string(store: &IPropertyStore, key: &PROPERTYKEY, text: &str) -> Result<(), String> {
    let buffer = wide(text);
    let allocation = CoTaskMemAlloc(buffer.len() * size_of::<u16>());
    if allocation.is_null() {
        return Err("Could not allocate a Shell property".into());
    }
    std::ptr::copy_nonoverlapping(buffer.as_ptr(), allocation.cast::<u16>(), buffer.len());
    let value = PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_LPWSTR,
                Anonymous: PROPVARIANT_0_0_0 {
                    pwszVal: PWSTR(allocation.cast()),
                },
                ..Default::default()
            }),
        },
    };
    store.SetValue(key, &value).map_err(failure)
}

pub fn group_app_id(group: &Group) -> String {
    format!("{}.Group.{}", crate::APP_NAME, group.id)
}

pub fn window_app_id(window: HWND, identity: &str) -> Result<(), String> {
    unsafe {
        let properties: IPropertyStore = SHGetPropertyStoreForWindow(window).map_err(failure)?;
        set_string(&properties, &APP_ID, identity)?;
        properties.Commit().map_err(failure)
    }
}

pub fn shortcut(store: &Store, group: &Group, icon: Option<&Path>) -> Result<PathBuf, String> {
    let folder = store.root.join("shortcuts").join(&group.id);
    std::fs::create_dir_all(&folder).map_err(failure)?;
    let previous = std::fs::read_dir(&folder)
        .map_err(failure)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|extension| extension == "lnk"));
    let sanitized: String = group
        .name
        .chars()
        .take(100)
        .map(|character| {
            if "<>:\"/\\|?*".contains(character) || character.is_control() {
                '_'
            } else {
                character
            }
        })
        .collect();
    let path = previous.unwrap_or_else(|| {
        folder.join(format!(
            "{} - {}.lnk",
            crate::APP_NAME,
            sanitized.trim_end_matches([' ', '.'])
        ))
    });
    unsafe {
        let link = make_link(
            store,
            group,
            &format!("--group {}", group.id),
            &group.name,
            icon,
        )?;
        let persist: IPersistFile = link.cast().map_err(failure)?;
        persist
            .Save(PCWSTR(wide(&path).as_ptr()), true)
            .map_err(failure)?;
        SHChangeNotify(
            SHCNE_UPDATEITEM,
            SHCNF_PATHW,
            Some(wide(&path).as_ptr().cast()),
            None,
        );
    }
    Ok(path)
}

unsafe fn make_link(
    store: &Store,
    group: &Group,
    command: &str,
    title: &str,
    icon: Option<&Path>,
) -> Result<IShellLinkW, String> {
    let executable = std::env::current_exe().map_err(failure)?;
    let link: IShellLinkW =
        CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(failure)?;
    link.SetPath(PCWSTR(wide(&executable).as_ptr()))
        .map_err(failure)?;
    let arguments = format!("{command} --data-dir \"{}\"", store.root.display());
    link.SetArguments(PCWSTR(wide(&arguments).as_ptr()))
        .map_err(failure)?;
    link.SetDescription(PCWSTR(wide(title).as_ptr()))
        .map_err(failure)?;
    link.SetWorkingDirectory(PCWSTR(
        wide(executable.parent().unwrap_or(Path::new("."))).as_ptr(),
    ))
    .map_err(failure)?;
    if let Some(icon) = icon {
        link.SetIconLocation(PCWSTR(wide(icon).as_ptr()), 0)
            .map_err(failure)?;
    }
    let properties: IPropertyStore = link.cast().map_err(failure)?;
    set_string(&properties, &APP_ID, &group_app_id(group))?;
    set_string(&properties, &TITLE, title)?;
    properties.Commit().map_err(failure)?;
    Ok(link)
}

pub fn update_jump_list(store: &Store, group: &Group, launch_all: bool) -> Result<(), String> {
    unsafe {
        let list: ICustomDestinationList =
            CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER).map_err(failure)?;
        list.SetAppID(PCWSTR(wide(group_app_id(group)).as_ptr()))
            .map_err(failure)?;
        let mut slots = 0;
        let _removed: IObjectArray = list.BeginList(&mut slots).map_err(failure)?;
        let collection: IObjectCollection =
            CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)
                .map_err(failure)?;
        let edit = make_link(
            store,
            group,
            &format!("--edit {}", group.id),
            "Edit this group",
            None,
        )?;
        collection.AddObject(&edit).map_err(failure)?;
        if launch_all {
            let launch = make_link(
                store,
                group,
                &format!("--launch-all {}", group.id),
                "Launch all",
                None,
            )?;
            collection.AddObject(&launch).map_err(failure)?;
        }
        let objects: IObjectArray = collection.cast().map_err(failure)?;
        list.AddUserTasks(&objects).map_err(failure)?;
        list.CommitList().map_err(failure)
    }
}

pub fn startup(store: &Store, enabled: bool) -> Result<(), String> {
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()
        .map_err(failure)?;
        let result = if enabled {
            let executable = std::env::current_exe().map_err(failure)?;
            let command = wide(format!(
                "\"{}\" --silent --data-dir \"{}\"",
                executable.display(),
                store.root.display()
            ));
            let bytes =
                std::slice::from_raw_parts(command.as_ptr().cast::<u8>(), command.len() * 2);
            RegSetValueExW(
                key,
                PCWSTR(wide(crate::APP_NAME).as_ptr()),
                None,
                REG_SZ,
                Some(bytes),
            )
            .ok()
            .map_err(failure)
        } else {
            let status = RegDeleteValueW(key, PCWSTR(wide(crate::APP_NAME).as_ptr()));
            if status == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                status.ok().map_err(failure)
            }
        };
        let _ = RegCloseKey(key);
        result
    }
}

pub fn tray(
    owner: HWND,
    id: u32,
    title: &str,
    icon: HICON,
    callback: u32,
    operation: NOTIFY_ICON_MESSAGE,
) {
    unsafe {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: owner,
            uID: id,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: callback,
            hIcon: icon,
            ..Default::default()
        };
        let title = wide(title);
        let length = (title.len() - 1).min(data.szTip.len() - 1);
        data.szTip[..length].copy_from_slice(&title[..length]);
        let _ = Shell_NotifyIconW(operation, &data);
    }
}

pub fn drag_shortcut(owner: HWND, path: &Path) -> Result<(), String> {
    unsafe {
        let item: IShellItem =
            SHCreateItemFromParsingName(PCWSTR(wide(path).as_ptr()), None).map_err(failure)?;
        let data: IDataObject = item
            .BindToHandler(None, &BHID_DataObject)
            .map_err(failure)?;
        let _ = SHDoDragDrop(Some(owner), &data, None, DROPEFFECT_COPY | DROPEFFECT_LINK);
        Ok(())
    }
}

/// Shortcut files only point at an application, so moving one never breaks what it opens.
pub fn is_shortcut(path: &str) -> bool {
    let path = Path::new(path);
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("lnk") || extension.eq_ignore_ascii_case("url")
    }) && path.is_file()
}

/// `canonicalize` gives `\\?\` paths, which the Shell and people read less reliably.
pub fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{share}"))
    } else if let Some(local) = text.strip_prefix(r"\\?\") {
        PathBuf::from(local)
    } else {
        path.to_path_buf()
    }
}

/// Moves files into `folder` as Explorer does, with its prompts (such as administrator
/// permission for the shared desktop) and its undo. Returns where each file ended up, or None
/// for one that stayed where it was because it was skipped, cancelled or could not move.
pub fn move_files(
    owner: HWND,
    files: &[PathBuf],
    folder: &Path,
) -> Result<Vec<Option<PathBuf>>, String> {
    let inside = |file: &PathBuf| {
        file.parent().is_some_and(|parent| {
            parent
                .to_string_lossy()
                .eq_ignore_ascii_case(&folder.to_string_lossy())
        })
    };
    let mut placed: Vec<_> = files
        .iter()
        .map(|file| inside(file).then(|| file.clone()))
        .collect();
    let mut moves = Vec::new();
    for (index, file) in files.iter().enumerate() {
        if placed[index].is_some() {
            continue;
        }
        let stem = file.file_stem().unwrap_or_default().to_string_lossy();
        let extension = file
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default();
        // Explorer's own "Name (2)" when the name is taken.
        if let Some(destination) = (1..)
            .map(|number| {
                folder.join(if number == 1 {
                    format!("{stem}{extension}")
                } else {
                    format!("{stem} ({number}){extension}")
                })
            })
            .find(|candidate| {
                !candidate.exists() && moves.iter().all(|(_, taken)| taken != candidate)
            })
        {
            moves.push((index, destination));
        }
    }
    if moves.is_empty() {
        return Ok(placed);
    }
    std::fs::create_dir_all(folder).map_err(failure)?;
    unsafe {
        let operation: IFileOperation =
            CoCreateInstance(&FileOperation, None, CLSCTX_ALL).map_err(failure)?;
        operation.SetOwnerWindow(owner).map_err(failure)?;
        operation
            .SetOperationFlags(FOF_ALLOWUNDO | FOF_NOCONFIRMMKDIR)
            .map_err(failure)?;
        let target: IShellItem =
            SHCreateItemFromParsingName(PCWSTR(wide(folder).as_ptr()), None).map_err(failure)?;
        for (index, destination) in &moves {
            let item: IShellItem =
                SHCreateItemFromParsingName(PCWSTR(wide(&files[*index]).as_ptr()), None)
                    .map_err(failure)?;
            let name = wide(destination.file_name().unwrap_or_default());
            operation
                .MoveItem(&item, &target, PCWSTR(name.as_ptr()), None)
                .map_err(failure)?;
        }
        // Cancelling or skipping a file is the person's choice; it simply stays where it was.
        let _ = operation.PerformOperations();
    }
    for (index, destination) in moves {
        placed[index] = destination.is_file().then_some(destination);
    }
    Ok(placed)
}

pub fn desktop() -> Result<PathBuf, String> {
    unsafe {
        let path =
            SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None).map_err(failure)?;
        let result = path.to_string().map(PathBuf::from).map_err(failure);
        CoTaskMemFree(Some(path.0.cast()));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_shortcuts_renames_taken_names_and_keeps_files_already_inside() {
        let _apartment = Apartment::new().unwrap();
        let root = std::env::temp_dir().join(format!("AppStack move test {}", new_id()));
        let (desktop, items) = (root.join("Desktop"), root.join("items"));
        std::fs::create_dir_all(&desktop).unwrap();
        std::fs::create_dir_all(&items).unwrap();
        for name in ["Editor.lnk", "Game.url"] {
            std::fs::write(desktop.join(name), name).unwrap();
        }
        std::fs::write(items.join("Editor.lnk"), "taken").unwrap();
        assert!(is_shortcut(&desktop.join("Game.url").to_string_lossy()));
        assert!(!is_shortcut(&desktop.join("Missing.lnk").to_string_lossy()));
        let files = [
            desktop.join("Editor.lnk"),
            desktop.join("Game.url"),
            items.join("Editor.lnk"),
        ];
        let placed = move_files(HWND::default(), &files, &items).unwrap();
        assert_eq!(
            placed,
            [
                Some(items.join("Editor (2).lnk")),
                Some(items.join("Game.url")),
                Some(items.join("Editor.lnk")),
            ]
        );
        assert!(!desktop.join("Editor.lnk").exists() && !desktop.join("Game.url").exists());
        assert_eq!(
            std::fs::read_to_string(items.join("Editor (2).lnk")).unwrap(),
            "Editor.lnk"
        );
        assert_eq!(plain(Path::new(r"\\?\C:\Data")), Path::new(r"C:\Data"));
        assert_eq!(
            plain(Path::new(r"\\?\UNC\server\share")),
            Path::new(r"\\server\share")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shortcuts_preserve_identifiers_and_paths_after_rename() {
        let _apartment = Apartment::new().unwrap();
        let store = Store {
            root: std::env::temp_dir().join(format!("AppStack shortcut test {}", new_id())),
        };
        let mut group = Group {
            name: "Work tools".into(),
            ..Group::default()
        };
        let first = shortcut(&store, &group, None).unwrap();
        group.name = "Renamed tools".into();
        let second = shortcut(&store, &group, None).unwrap();
        assert_eq!(first, second);
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            let persist: IPersistFile = link.cast().unwrap();
            persist
                .Load(PCWSTR(wide(&second).as_ptr()), STGM_READ)
                .unwrap();
            let mut arguments = vec![0u16; 8192];
            link.GetArguments(&mut arguments).unwrap();
            let arguments = String::from_utf16_lossy(
                &arguments[..arguments.iter().position(|value| *value == 0).unwrap()],
            );
            assert!(arguments.contains(&format!("--group {}", group.id)));
            assert!(arguments.contains(&format!("--data-dir \"{}\"", store.root.display())));
            let properties: IPropertyStore = link.cast().unwrap();
            let value = properties.GetValue(&APP_ID).unwrap();
            let identity = windows::core::BSTR::try_from(&value).unwrap().to_string();
            assert_eq!(identity, group_app_id(&group));
        }
        std::fs::remove_dir_all(store.root).unwrap();
    }
}
