#![allow(unsafe_op_in_unsafe_fn)]

use super::{
    drop_target,
    icons::{self, Icon, Spec},
    jobs::{self, Background, Completed, Job},
    shell::{self, failure, wide},
    ui::{self, Field, Font},
};
use appstack::{
    command::{Action, Request},
    model::*,
    storage::{Store, display_name, selected_with_dependencies},
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    mem::size_of,
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
            Ole::{IDropTarget, RevokeDragDrop},
        },
        UI::{
            Accessibility::*, Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*,
            WindowsAndMessaging::*,
        },
    },
    core::{BOOL, PCWSTR, w},
};

const NEW_GROUP: u16 = 101;
const EDIT_GROUP: u16 = 102;
const DUPLICATE_GROUP: u16 = 103;
const DELETE_GROUP: u16 = 104;
const OPEN_GROUP: u16 = 105;
const SHORTCUT: u16 = 106;
const SHORTCUT_FOLDER: u16 = 107;
const GROUP_UP: u16 = 108;
const GROUP_DOWN: u16 = 109;
const GROUP_ICON: u16 = 110;
const ADD_FILES: u16 = 201;
const ADD_MANUAL: u16 = 202;
const ADD_SUBGROUP: u16 = 203;
const EDIT_ENTRY: u16 = 204;
const DELETE_ENTRY: u16 = 205;
const LAUNCH_ENTRY: u16 = 206;
const LAUNCH_ADMIN: u16 = 207;
const ENTRY_UP: u16 = 208;
const ENTRY_DOWN: u16 = 209;
const ENTRY_ICON: u16 = 210;
const LAUNCH_ALL: u16 = 211;
const BACK: u16 = 213;
const ANIMATION: usize = 1;
const IMPORT: u16 = 301;
const IMPORT_FOLDER: u16 = 302;
const EXPORT: u16 = 303;
const DATA_FOLDER: u16 = 304;
const QUIT: u16 = 399;
const SETTINGS: u16 = 401;
const ABOUT: u16 = 402;
const STATIC_END_ELLIPSIS: u32 = 0x4000;
const SS_CENTERIMAGE: u32 = 0x200;

struct Images {
    handle: HIMAGELIST,
    keys: HashMap<u64, i32>,
}

impl Images {
    fn new(size: i32) -> Self {
        Self {
            handle: unsafe { ImageList_Create(size, size, ILC_COLOR32, 1, 16) },
            keys: HashMap::new(),
        }
    }
    fn add(&self, icon: HICON) -> i32 {
        unsafe { ImageList_ReplaceIcon(self.handle, -1, icon) }
    }
    fn shared(&mut self, key: u64, icon: HICON) -> i32 {
        if let Some(index) = self.keys.get(&key) {
            return *index;
        }
        let index = self.add(icon);
        if index >= 0 {
            self.keys.insert(key, index);
        }
        index
    }
    fn replace(&self, index: usize, icon: HICON) {
        unsafe {
            ImageList_ReplaceIcon(self.handle, index as i32, icon);
        }
    }
    fn clear(&mut self) {
        self.keys.clear();
        unsafe {
            let _ = ImageList_Remove(self.handle, -1);
        }
    }
}

impl Drop for Images {
    fn drop(&mut self) {
        unsafe {
            let _ = ImageList_Destroy(Some(self.handle));
        }
    }
}

fn show_popup(window: HWND, focus: HWND, animate: bool) -> Result<(), String> {
    unsafe {
        let _ = RemovePropW(window, w!("AppStack.PopupReady"));
        if animate {
            ui::animate_in(window, focus);
        }
        let _ = ShowWindow(window, SW_SHOWNORMAL);
        let _ = RedrawWindow(Some(window), None, None, RDW_ALLCHILDREN | RDW_UPDATENOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(focus));
        SetPropW(
            window,
            w!("AppStack.PopupReady"),
            Some(HANDLE(std::ptr::without_provenance_mut(1))),
        )
        .map_err(failure)?;
        NotifyWinEvent(EVENT_OBJECT_STATECHANGE, window, 0, 0);
    }
    Ok(())
}

/// The icon list's left edge and width in the viewport: the right strip holds the page dots or
/// scroll bar, and a pill mirrors it on the left so the icons stay centered in its round shape.
fn list_span(viewport_width: i32, gutter: i32, overflow: bool, pill: bool) -> (i32, i32) {
    let left = if overflow && pill { gutter } else { 0 };
    (
        left,
        viewport_width - left - if overflow { gutter } else { 0 },
    )
}

pub(super) fn button_positions(
    left: i32,
    right: i32,
    widths: &[i32],
    gap: i32,
    alignment: ButtonAlignment,
) -> Vec<i32> {
    let total = widths.iter().sum::<i32>() + gap * widths.len().saturating_sub(1) as i32;
    let available = (right - left - total).max(0);
    let mut cursor = left
        + match alignment {
            ButtonAlignment::Left => 0,
            ButtonAlignment::Center => available / 2,
            ButtonAlignment::Right => available,
        };
    widths
        .iter()
        .map(|width| {
            let position = cursor;
            cursor += width + gap;
            position
        })
        .collect()
}

fn apply_group_values(group: &mut Group, values: &[String]) -> Result<(), String> {
    if values.len() != 30 {
        return Err("The group settings are incomplete".into());
    }
    group.name = values[0].trim().into();
    let appearance = &mut group.appearance;
    appearance.icon = values[1].trim().into();
    appearance.grid_icon = values[2] != "0";
    appearance.mosaic_icon = values[2] == "2";
    appearance.sort = if values[3] == "1" {
        SortMode::Alphabetical
    } else {
        SortMode::Manual
    };
    appearance.show_on_tray = values[4] == "true";
    let number = |index: usize| {
        values[index]
            .parse::<u32>()
            .map_err(|_| "Choose a valid size".to_string())
    };
    appearance.columns = number(5)? as usize;
    appearance.icon_size = number(6)?;
    appearance.gap_horizontal = number(7)?;
    appearance.gap_vertical = number(8)?;
    appearance.padding_horizontal = number(9)?;
    appearance.padding_vertical = number(10)?;
    appearance.corner_radius = number(11)?;
    appearance.tile_radius = number(12)?;
    appearance.card_layout = values[13] == "1";
    appearance.accent = values[14].trim().into();
    appearance.show_labels = values[15] == "true";
    appearance.label_position = if values[16] == "1" {
        LabelPosition::Right
    } else {
        LabelPosition::Bottom
    };
    appearance.label_size = number(17)?;
    appearance.show_header = values[18] == "true";
    appearance.header_bottom = values[19] == "1";
    appearance.show_edit_button = values[20] == "true";
    appearance.show_launch_all_button = values[21] == "true";
    appearance.button_position = if values[22] == "1" {
        ButtonPosition::Top
    } else {
        ButtonPosition::Bottom
    };
    appearance.button_alignment = match values[23].as_str() {
        "0" => ButtonAlignment::Left,
        "1" => ButtonAlignment::Center,
        _ => ButtonAlignment::Right,
    };
    appearance.scroll = values[24] == "true";
    appearance.visible_rows = number(25)?;
    appearance.minimum_width = number(26)?;
    appearance.border_width = number(27)?;
    appearance.border_color = values[28].trim().into();
    appearance.shadow = number(29)?;
    Ok(())
}

struct Manager {
    window: HWND,
    groups: HWND,
    entries: HWND,
    title: HWND,
    status: HWND,
    buttons: Vec<(u16, HWND)>,
    font: Font,
    title_font: Font,
    group_images: Images,
    entry_images: Images,
    group_ids: Vec<String>,
    entry_ids: Vec<String>,
    group_specs: Vec<Spec>,
    entry_specs: Vec<Spec>,
    dpi: u32,
    _drop_target: IDropTarget,
    _ole: shell::OleApartment,
}

impl Drop for Manager {
    fn drop(&mut self) {
        unsafe {
            let _ = RevokeDragDrop(self.entries);
            let _ = DestroyWindow(self.window);
        }
    }
}

struct Slot {
    entry: usize,
    key: u64,
}

struct Popup {
    window: HWND,
    viewport: HWND,
    list: HWND,
    images: Images,
    group_id: String,
    entries: Vec<Entry>,
    slots: Vec<Slot>,
    fonts: [Font; 4],
    look: ui::Look,
    dpi: u32,
    columns: usize,
    cell_width: i32,
    cell_height: i32,
    viewport_height: i32,
    scroll: i32,
    monitor: usize,
    work_area: RECT,
    /// Subfolders opened inside this popup, innermost last.
    path: Vec<String>,
    title: Option<HWND>,
    back: Option<HWND>,
    /// Where the folder title goes; None when the popup has no band, so subfolders add one.
    title_slot: Option<RECT>,
    show_title: bool,
    grown: bool,
    margin: i32,
    vertical_margin: i32,
    frame: ui::Frame,
    width: i32,
    band: i32,
    gutter: i32,
    page_rows: usize,
    /// The group pages by whole screens with dots instead of scrolling.
    pages: bool,
    paged: bool,
    target: i32,
    flipped: Option<std::time::Instant>,
}

impl Popup {
    fn current(&self) -> &str {
        self.path.last().unwrap_or(&self.group_id)
    }

    fn page_height(&self) -> i32 {
        self.page_rows as i32 * self.cell_height
    }

    fn content_height(&self) -> i32 {
        self.entries.len().div_ceil(self.columns).max(1) as i32 * self.cell_height - self.look.gap.1
    }

    fn page_count(&self) -> i32 {
        self.entries
            .len()
            .div_ceil(self.columns)
            .div_ceil(self.page_rows)
            .max(1) as i32
    }
}

impl Drop for Popup {
    fn drop(&mut self) {
        ui::forget_list(self.list);
        ui::set_pager(self.viewport, None, 0, 0);
        if let Some(back) = self.back {
            ui::forget_ghost(back);
        }
        unsafe {
            let _ = DestroyWindow(self.window);
        }
    }
}

pub struct Application {
    pub host: HWND,
    pub store: Store,
    pub config: Config,
    background: Background,
    manager: Option<Manager>,
    popups: Vec<Popup>,
    parked: VecDeque<Popup>,
    taskbar_buttons: HashMap<(String, usize), IUIAutomationElement>,
    deferred_results: VecDeque<Completed>,
    pub(super) selected: Option<String>,
    brand: Icon,
    icon_cache: HashMap<u64, Icon>,
    icon_order: VecDeque<u64>,
    pending: HashSet<u64>,
    generation: u64,
    // Dragged list, the row it started at, and the row it has been moved to so far.
    drag: Option<(HWND, usize, usize)>,
    tray_ids: Vec<(u32, Option<String>)>,
    pub taskbar_created: u32,
    pub quitting: bool,
}

impl Application {
    pub fn report_error(&self, error: &str) {
        use std::io::Write;
        if let Ok(mut log) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.store.root.join("errors.log"))
        {
            let _ = writeln!(log, "{error}");
        }
        shell::message(Some(self.owner()), "AppStack", error, true);
    }

    pub fn new(host: HWND, store: Store, config: Config) -> Result<Self, String> {
        ui::set_menu_theme(config.settings.main_theme);
        ui::set_pointer(config.settings.pointer_cursor);
        let background = Background::new(host, store.clone());
        Ok(Self {
            host,
            background,
            store,
            config,
            manager: None,
            popups: Vec::new(),
            parked: VecDeque::new(),
            taskbar_buttons: HashMap::new(),
            deferred_results: VecDeque::new(),
            selected: None,
            brand: icons::brand()?,
            icon_cache: HashMap::new(),
            icon_order: VecDeque::new(),
            pending: HashSet::new(),
            generation: 0,
            drag: None,
            tray_ids: Vec::new(),
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
            quitting: false,
        })
    }

    pub fn initialize_shell(&mut self) {
        self.update_tray();
        if !self.config.groups.is_empty() {
            self.job(Job::EnsureBrand(self.config.clone()));
        }
    }

    pub fn prewarm_popups(&mut self, point: POINT) -> Result<(), String> {
        let groups: Vec<_> = self
            .config
            .ordered_groups()
            .into_iter()
            .filter(|group| !group.entries.is_empty() && group.entries.len() <= 32)
            .take(4)
            .map(|group| group.id.clone())
            .collect();
        for id in groups {
            if let Some(anchor) = self.taskbar_point(&id, point) {
                self.build_popup(&id, anchor, None, false)?;
            }
        }
        Ok(())
    }

    fn taskbar_point(&mut self, id: &str, fallback: POINT) -> Option<POINT> {
        unsafe {
            let monitor = MonitorFromPoint(fallback, MONITOR_DEFAULTTONEAREST);
            let key = (id.to_string(), monitor.0 as usize);
            let point = |element: &IUIAutomationElement| -> Option<POINT> {
                let bounds = element.CurrentBoundingRectangle().ok()?;
                (bounds.right > bounds.left
                    && bounds.bottom > bounds.top
                    && MonitorFromRect(&bounds, MONITOR_DEFAULTTONEAREST) == monitor)
                    .then_some(POINT {
                        x: (bounds.left + bounds.right) / 2,
                        y: (bounds.top + bounds.bottom) / 2,
                    })
            };
            if let Some(position) = self.taskbar_buttons.get(&key).and_then(point) {
                return Some(position);
            }
            let find = || -> Option<IUIAutomationElement> {
                let group = self.config.group(id)?;
                let automation: IUIAutomation =
                    CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
                let value = windows::Win32::System::Variant::VARIANT::from(
                    format!("Appid: {}", shell::group_app_id(group)).as_str(),
                );
                let condition = automation
                    .CreatePropertyCondition(UIA_AutomationIdPropertyId, &value)
                    .ok()?;
                for class in [w!("Shell_TrayWnd"), w!("Shell_SecondaryTrayWnd")] {
                    let mut previous = None;
                    while let Ok(window) = FindWindowExW(None, previous, class, None) {
                        previous = Some(window);
                        if MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) == monitor {
                            let root = automation.ElementFromHandle(window).ok()?;
                            if let Ok(button) = root.FindFirst(TreeScope_Descendants, &condition) {
                                return Some(button);
                            }
                        }
                    }
                }
                None
            };
            if let Some(button) = find() {
                if let Some(position) = point(&button) {
                    self.taskbar_buttons.insert(key, button);
                    return Some(position);
                }
            }
        }
        None
    }

    fn owner(&self) -> HWND {
        self.manager
            .as_ref()
            .map(|manager| manager.window)
            .unwrap_or(self.host)
    }

    pub(super) fn selected_group(&self) -> Option<&Group> {
        self.selected
            .as_deref()
            .and_then(|id| self.config.group(id))
    }

    fn group_index(&self) -> Option<usize> {
        self.selected
            .as_ref()
            .and_then(|id| self.config.groups.iter().position(|group| &group.id == id))
    }

    /// Rows of a manager list stand for positions in the config only in manual order.
    fn manual_order(&self, list: HWND) -> Result<(), String> {
        if self
            .manager
            .as_ref()
            .is_some_and(|manager| manager.groups == list)
        {
            if self.config.settings.group_sort != SortMode::Manual {
                return Err("Switch group order to Manual before reordering".into());
            }
        } else if self
            .selected_group()
            .is_none_or(|group| group.appearance.sort != SortMode::Manual)
        {
            return Err("Switch application order to Manual before reordering".into());
        }
        Ok(())
    }

    /// Moves a dragged row in memory and on screen; the drop commits it.
    fn move_row(&mut self, list: HWND, from: usize, to: usize) {
        if from == to {
            return;
        }
        if self
            .manager
            .as_ref()
            .is_some_and(|manager| manager.groups == list)
        {
            move_item(&mut self.config.groups, from, to);
            self.refresh_manager();
        } else if let Some(group) = self.group_index() {
            move_item(&mut self.config.groups[group].entries, from, to);
            self.refresh_entries();
            if let Some(manager) = &self.manager {
                ui::select(manager.entries, to);
            }
        }
    }

    fn entry_index(&self) -> Option<usize> {
        let manager = self.manager.as_ref()?;
        let id = manager.entry_ids.get(ui::selection(manager.entries)?)?;
        self.selected_group()?
            .entries
            .iter()
            .position(|entry| &entry.id == id)
    }

    fn job(&self, job: Job) {
        let _ = self.background.jobs.send(job);
    }

    pub fn request(&mut self, request: Request) -> Result<(), String> {
        let mut point = POINT {
            x: request.x,
            y: request.y,
        };
        match request.action.clone() {
            Action::Manager => self.show_manager(),
            Action::Silent => Ok(()),
            Action::Quit => {
                self.quitting = true;
                Ok(())
            }
            Action::Open(value) | Action::Edit(value) | Action::LaunchAll(value) => {
                let id = self
                    .config
                    .groups
                    .iter()
                    .find(|group| group.id == value || group.name.eq_ignore_ascii_case(&value))
                    .map(|group| group.id.clone())
                    .ok_or_else(|| format!("Group not found: {value}"))?;
                match request.action {
                    Action::Open(_) => {
                        if let Some((x, y)) = request.taskbar {
                            point = self.taskbar_point(&id, POINT { x, y }).unwrap_or(point);
                        }
                        self.open_popup(&id, point, None)
                    }
                    Action::Edit(_) => {
                        self.selected = Some(id);
                        self.show_manager()?;
                        self.edit_group(false)
                    }
                    Action::LaunchAll(_) => self.launch_group(&id),
                    _ => unreachable!(),
                }
            }
        }
    }

    pub fn show_manager(&mut self) -> Result<(), String> {
        self.hide_popups(0);
        if self.manager.is_none() {
            let ole = shell::OleApartment::new()?;
            let window = ui::window(
                "AppStack.Manager",
                crate::APP_NAME,
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                WINDOW_EX_STYLE(0),
                None,
                920,
                600,
            )?;
            let dpi = unsafe { GetDpiForWindow(window) }.max(96);
            let font = Font::new(dpi, 14);
            let groups = ui::list(window, 10, &[("Groups", ui::scale(220, dpi))], &font)?;
            let entries = ui::list(
                window,
                11,
                &[
                    ("Applications", ui::scale(220, dpi)),
                    ("Target", ui::scale(300, dpi)),
                ],
                &font,
            )?;
            let title = ui::child(
                window,
                w!("STATIC"),
                "AppStack",
                12,
                WINDOW_STYLE(STATIC_END_ELLIPSIS | SS_CENTERIMAGE),
                &font,
            )?;
            let status = ui::child(window, w!("STATIC"), "", 13, WINDOW_STYLE(0), &font)?;
            let mut buttons = Vec::new();
            for (id, label) in [
                (NEW_GROUP, "New group"),
                (OPEN_GROUP, "Open"),
                (ADD_FILES, "Add apps"),
                (ADD_SUBGROUP, "Add subgroup"),
            ] {
                buttons.push((
                    id,
                    ui::child(window, w!("BUTTON"), label, id, WS_TABSTOP, &font)?,
                ));
            }
            let group_images = Images::new(ui::scale(24, dpi));
            let entry_images = Images::new(ui::scale(24, dpi));
            ui::send(
                groups,
                LVM_SETIMAGELIST,
                WPARAM(LVSIL_SMALL as usize),
                LPARAM(group_images.handle.0 as isize),
            );
            ui::send(
                entries,
                LVM_SETIMAGELIST,
                WPARAM(LVSIL_SMALL as usize),
                LPARAM(entry_images.handle.0 as isize),
            );
            let title_font = Font::with(dpi, 18, &self.config.settings.title_font, 600);
            ui::send(
                title,
                WM_SETFONT,
                WPARAM(title_font.0.0 as usize),
                LPARAM(0),
            );
            self.manager = Some(Manager {
                window,
                groups,
                entries,
                title,
                status,
                buttons,
                font,
                title_font,
                group_images,
                entry_images,
                group_ids: Vec::new(),
                entry_ids: Vec::new(),
                group_specs: Vec::new(),
                entry_specs: Vec::new(),
                dpi,
                _drop_target: drop_target::register(entries)?,
                _ole: ole,
            });
            self.install_menu(window)?;
            let _ = shell::window_app_id(window, crate::APP_NAME);
            self.refresh_manager();
            self.layout_manager();
            ui::apply_theme(
                window,
                self.config.settings.main_theme,
                &[groups, entries],
                None,
            );
            ui::send(
                window,
                WM_SETICON,
                WPARAM(ICON_BIG as usize),
                LPARAM(self.brand.handle().0 as isize),
            );
            ui::send(
                window,
                WM_SETICON,
                WPARAM(ICON_SMALL as usize),
                LPARAM(self.brand.handle().0 as isize),
            );
        }
        if let Some(manager) = &self.manager {
            unsafe {
                let _ = ShowWindow(manager.window, SW_RESTORE);
                let _ = SetForegroundWindow(manager.window);
            }
        }
        Ok(())
    }

    fn install_menu(&self, window: HWND) -> Result<(), String> {
        unsafe {
            let bar = CreateMenu().map_err(failure)?;
            for (name, commands) in [
                (
                    "&File",
                    vec![
                        (IMPORT, "Import backup / JSON..."),
                        (
                            IMPORT_FOLDER,
                            "Import configuration / TaskbarGroups folder...",
                        ),
                        (EXPORT, "Export backup..."),
                        (DATA_FOLDER, "Open data folder"),
                        (QUIT, "Exit AppStack"),
                    ],
                ),
                (
                    "&Group",
                    vec![
                        (NEW_GROUP, "New group..."),
                        (EDIT_GROUP, "Edit group..."),
                        (DUPLICATE_GROUP, "Duplicate"),
                        (DELETE_GROUP, "Delete"),
                        (OPEN_GROUP, "Open popup"),
                        (GROUP_ICON, "Choose group icon..."),
                        (SHORTCUT, "Create desktop shortcut"),
                        (SHORTCUT_FOLDER, "Open shortcut location"),
                        (GROUP_UP, "Move up"),
                        (GROUP_DOWN, "Move down"),
                        (LAUNCH_ALL, "Launch all"),
                    ],
                ),
                (
                    "&Application",
                    vec![
                        (ADD_FILES, "Add files..."),
                        (ADD_MANUAL, "Add path / Shell identifier..."),
                        (ADD_SUBGROUP, "Add subgroup..."),
                        (EDIT_ENTRY, "Edit..."),
                        (ENTRY_ICON, "Choose icon..."),
                        (DELETE_ENTRY, "Remove"),
                        (LAUNCH_ENTRY, "Launch"),
                        (LAUNCH_ADMIN, "Run as administrator"),
                        (ENTRY_UP, "Move up"),
                        (ENTRY_DOWN, "Move down"),
                    ],
                ),
                (
                    "&Settings",
                    vec![(SETTINGS, "Preferences..."), (ABOUT, "About AppStack")],
                ),
            ] {
                let menu = CreatePopupMenu().map_err(failure)?;
                for (id, label) in commands {
                    AppendMenuW(menu, MF_STRING, id as usize, PCWSTR(wide(label).as_ptr()))
                        .map_err(failure)?;
                }
                AppendMenuW(bar, MF_POPUP, menu.0 as usize, PCWSTR(wide(name).as_ptr()))
                    .map_err(failure)?;
            }
            SetMenu(window, Some(bar)).map_err(failure)?;
            Ok(())
        }
    }

    fn layout_manager(&self) {
        let Some(manager) = &self.manager else {
            return;
        };
        unsafe {
            let mut bounds = RECT::default();
            let _ = GetClientRect(manager.window, &mut bounds);
            let dpi = manager.dpi;
            let margin = ui::scale(14, dpi);
            let toolbar = ui::scale(50, dpi);
            let sidebar = ui::scale(240, dpi).min((bounds.right / 3).max(ui::scale(120, dpi)));
            ui::place(
                manager.title,
                margin,
                margin - 2,
                sidebar,
                ui::scale(30, dpi),
            );
            for (index, (_, button)) in manager.buttons.iter().enumerate() {
                let widths = [110, 75, 100, 120];
                let left: i32 = widths[..index].iter().sum();
                ui::place(
                    *button,
                    sidebar + margin * 2 + ui::scale(left, dpi),
                    margin - 2,
                    ui::scale(widths[index] - 8, dpi),
                    ui::scale(30, dpi),
                );
            }
            let height = bounds.bottom - toolbar - margin - ui::scale(26, dpi);
            ui::place(manager.groups, margin, toolbar, sidebar, height);
            ui::place(
                manager.entries,
                sidebar + margin * 2,
                toolbar,
                bounds.right - sidebar - margin * 3,
                height,
            );
            ui::place(
                manager.status,
                margin,
                bounds.bottom - ui::scale(25, dpi),
                bounds.right - margin * 2,
                ui::scale(22, dpi),
            );
            ui::send(
                manager.groups,
                LVM_SETCOLUMNWIDTH,
                WPARAM(0),
                LPARAM((sidebar - ui::scale(8, dpi)) as isize),
            );
            let entry_width = bounds.right - sidebar - margin * 3;
            ui::send(
                manager.entries,
                LVM_SETCOLUMNWIDTH,
                WPARAM(0),
                LPARAM((entry_width / 2).max(100) as isize),
            );
            ui::send(
                manager.entries,
                LVM_SETCOLUMNWIDTH,
                WPARAM(1),
                LPARAM((entry_width / 2 - ui::scale(24, dpi)).max(100) as isize),
            );
        }
    }

    fn request_icon(&mut self, spec: Spec) {
        let key = spec.key();
        if !self.icon_cache.contains_key(&key) && self.pending.insert(key) {
            let _ = self.background.icons.send((self.generation, spec));
        }
    }

    fn refresh_manager(&mut self) {
        if self
            .selected
            .as_deref()
            .is_none_or(|id| self.config.group(id).is_none())
        {
            self.selected = self
                .config
                .ordered_groups()
                .first()
                .map(|group| group.id.clone());
        }
        let mut requested = Vec::new();
        if let Some(manager) = &mut self.manager {
            ui::suppress(|| {
                ui::send(manager.groups, WM_SETREDRAW, WPARAM(0), LPARAM(0));
                ui::clear_list(manager.groups);
                manager.group_images.clear();
                manager.group_ids.clear();
                manager.group_specs.clear();
                for (index, group) in self.config.ordered_groups().iter().enumerate() {
                    let spec = jobs::group_spec(
                        &self.config,
                        group,
                        ui::scale(24, manager.dpi) as u32,
                        false,
                    );
                    let icon = self.icon_cache.get(&spec.key()).unwrap_or(&self.brand);
                    let image = manager.group_images.add(icon.handle());
                    ui::row(
                        manager.groups,
                        index,
                        std::slice::from_ref(&group.name),
                        image,
                    );
                    manager.group_ids.push(group.id.clone());
                    manager.group_specs.push(spec.clone());
                    requested.push(spec);
                    if self.selected.as_ref() == Some(&group.id) {
                        ui::select(manager.groups, index);
                    }
                }
                ui::send(manager.groups, WM_SETREDRAW, WPARAM(1), LPARAM(0));
                unsafe {
                    let _ = InvalidateRect(Some(manager.groups), None, true);
                }
            });
        }
        for spec in requested {
            self.request_icon(spec);
        }
        self.refresh_entries();
    }

    fn refresh_entries(&mut self) {
        let group = self.selected_group().cloned();
        let mut requested = Vec::new();
        if let Some(manager) = &mut self.manager {
            ui::suppress(|| {
                ui::send(manager.entries, WM_SETREDRAW, WPARAM(0), LPARAM(0));
                ui::clear_list(manager.entries);
                manager.entry_images.clear();
                manager.entry_ids.clear();
                manager.entry_specs.clear();
                if let Some(group) = &group {
                    ui::set_text(manager.title, &group.name);
                    for (index, entry) in group.ordered_entries().iter().enumerate() {
                        let spec = jobs::entry_spec(
                            &self.config,
                            entry,
                            ui::scale(24, manager.dpi) as u32,
                        );
                        let icon = self.icon_cache.get(&spec.key()).unwrap_or(&self.brand);
                        let image = manager.entry_images.add(icon.handle());
                        let target = match &entry.target {
                            Target::Application(path) => path.clone(),
                            Target::Group(id) => format!(
                                "Group: {}",
                                self.config
                                    .group(id)
                                    .map(|group| group.name.as_str())
                                    .unwrap_or("Missing")
                            ),
                        };
                        ui::row(manager.entries, index, &[entry.name.clone(), target], image);
                        manager.entry_ids.push(entry.id.clone());
                        manager.entry_specs.push(spec.clone());
                        requested.push(spec);
                    }
                    ui::set_text(
                        manager.status,
                        &format!(
                            "{} groups    {} applications",
                            self.config.groups.len(),
                            group.entries.len()
                        ),
                    );
                } else {
                    ui::set_text(manager.title, "AppStack");
                    ui::set_text(manager.status, "0 groups");
                }
                ui::send(manager.entries, WM_SETREDRAW, WPARAM(1), LPARAM(0));
                unsafe {
                    let _ = InvalidateRect(Some(manager.entries), None, true);
                }
            });
        }
        for spec in requested {
            self.request_icon(spec);
        }
    }

    pub(super) fn commit(&mut self, candidate: Config) -> Result<(), String> {
        candidate.validate()?;
        self.config = candidate;
        ui::set_pointer(self.config.settings.pointer_cursor);
        self.generation = self.generation.wrapping_add(1);
        self.popups.clear();
        self.parked.clear();
        self.icon_cache.clear();
        self.icon_order.clear();
        self.pending.clear();
        self.job(Job::Save(self.config.clone()));
        self.job(Job::Synchronize(self.config.clone()));
        self.refresh_manager();
        self.update_tray();
        Ok(())
    }

    fn edit_group(&mut self, create: bool) -> Result<(), String> {
        let mut group = if create {
            Group::default()
        } else {
            self.selected_group().ok_or("No group selected")?.clone()
        };
        let appearance = &group.appearance;
        let fields = [
            Field::page("General"),
            Field::text("Name", &group.name),
            Field::text("Icon path (.ico, .png, .exe)", &appearance.icon),
            Field::choice(
                "Group icon",
                &["Single", "Grid", "Mosaic"],
                usize::from(appearance.grid_icon) * (1 + usize::from(appearance.mosaic_icon)),
            ),
            Field::choice(
                "Application order",
                &["Manual", "Alphabetical"],
                usize::from(appearance.sort == SortMode::Alphabetical),
            ),
            Field::check(
                "Show this group in the system tray",
                appearance.show_on_tray,
            ),
            Field::page("Interface"),
            Field::preset(
                "Columns",
                &[
                    ("1", 1),
                    ("2", 2),
                    ("3", 3),
                    ("4", 4),
                    ("5", 5),
                    ("6", 6),
                    ("7", 7),
                    ("8", 8),
                    ("9", 9),
                    ("10", 10),
                    ("11", 11),
                    ("12", 12),
                    ("13", 13),
                    ("14", 14),
                    ("15", 15),
                    ("16", 16),
                ],
                appearance.columns as u32,
            ),
            Field::preset("Icon size", ICON_PRESETS, appearance.icon_size),
            Field::preset("Horizontal gap", SPACING_PRESETS, appearance.gap_horizontal),
            Field::preset("Vertical gap", SPACING_PRESETS, appearance.gap_vertical),
            Field::preset(
                "Side padding",
                SPACING_PRESETS,
                appearance.padding_horizontal,
            ),
            Field::preset(
                "Vertical padding",
                SPACING_PRESETS,
                appearance.padding_vertical,
            ),
            Field::preset("Popup rounding", ROUNDING_PRESETS, appearance.corner_radius),
            Field::preset("Tile rounding", ROUNDING_PRESETS, appearance.tile_radius),
            Field::choice(
                "Layout",
                &["Compact", "Card"],
                usize::from(appearance.card_layout),
            ),
            Field::color("Accent color (optional)", &appearance.accent),
            Field::check("Show icon labels", appearance.show_labels),
            Field::choice(
                "Labels",
                &["Bottom", "Right"],
                usize::from(appearance.label_position == LabelPosition::Right),
            ),
            Field::preset("Label size", LABEL_PRESETS, appearance.label_size),
            Field::check("Show group header", appearance.show_header),
            Field::choice(
                "Header",
                &["Top", "Bottom"],
                usize::from(appearance.header_bottom),
            ),
            Field::check("Show Edit button", appearance.show_edit_button),
            Field::check("Show Launch all button", appearance.show_launch_all_button),
            Field::choice(
                "Buttons",
                &["Bottom", "Top"],
                usize::from(appearance.button_position == ButtonPosition::Top),
            ),
            Field::choice(
                "Button alignment",
                &["Left", "Center", "Right"],
                match appearance.button_alignment {
                    ButtonAlignment::Left => 0,
                    ButtonAlignment::Center => 1,
                    ButtonAlignment::Right => 2,
                },
            ),
            Field::check("Scroll inside the group", appearance.scroll),
            Field::preset(
                "Visible rows",
                &[
                    ("1", 1),
                    ("2", 2),
                    ("3", 3),
                    ("4", 4),
                    ("5", 5),
                    ("6", 6),
                    ("8", 8),
                    ("12", 12),
                    ("16", 16),
                ],
                appearance.visible_rows,
            ),
            Field::preset("Minimum width", WIDTH_PRESETS, appearance.minimum_width),
            Field::preset("Border width", BORDER_PRESETS, appearance.border_width),
            Field::color("Border color (optional)", &appearance.border_color),
            Field::preset("Shadow", SHADOW_PRESETS, appearance.shadow),
        ];
        let mut preview_icons = Vec::new();
        let mut requested_icons = Vec::new();
        let mut loaded_icons = HashSet::new();
        for entry in group.ordered_entries().into_iter().take(8) {
            let spec = jobs::entry_spec(&self.config, entry, 64);
            let key = spec.key();
            self.request_icon(spec);
            requested_icons.push((entry.id.clone(), key));
            if self.icon_cache.contains_key(&key) {
                loaded_icons.insert(key);
            }
            let icon = self.icon_cache.get(&key).unwrap_or(&self.brand);
            let copied = unsafe { CopyIcon(icon.handle()) }.map_err(failure)?;
            preview_icons.push((entry.id.clone(), Icon(copied.0 as usize)));
        }
        if preview_icons.is_empty() {
            preview_icons.push((String::new(), icons::brand()?));
        }
        let preview = ui::GroupPreview {
            group: group.clone(),
            settings: self.config.settings.clone(),
            icons: preview_icons,
            update: apply_group_values,
        };
        let owner = self.owner();
        let theme = self.config.settings.main_theme;
        let mut live_icons = |preview: &mut ui::GroupPreview| {
            while let Ok(completed) = self.background.completed.try_recv() {
                match completed {
                    Completed::Icon {
                        generation,
                        key,
                        result,
                    } => self.accept_icon(generation, key, result),
                    other => self.deferred_results.push_back(other),
                }
            }
            let mut newly_loaded = Vec::new();
            for (id, key) in &requested_icons {
                if loaded_icons.contains(key) {
                    continue;
                }
                if let Some(icon) = self.icon_cache.get(key) {
                    if let Ok(copied) = unsafe { CopyIcon(icon.handle()) } {
                        if let Some((_, target)) =
                            preview.icons.iter_mut().find(|(entry, _)| entry == id)
                        {
                            *target = Icon(copied.0 as usize);
                            newly_loaded.push(*key);
                        }
                    }
                }
            }
            let changed = !newly_loaded.is_empty();
            loaded_icons.extend(newly_loaded);
            changed
        };
        let Some(values) = ui::form_with_preview(
            owner,
            if create { "New group" } else { "Edit group" },
            &fields,
            theme,
            Some(preview),
            Some(&mut live_icons),
        )?
        else {
            return Ok(());
        };
        apply_group_values(&mut group, &values)?;
        let mut candidate = self.config.clone();
        self.selected = Some(group.id.clone());
        if create {
            candidate.groups.push(group);
        } else if let Some(index) = self.group_index() {
            candidate.groups[index] = group;
        }
        if let Some(renamed) = self
            .selected
            .as_deref()
            .and_then(|id| candidate.group(id))
            .cloned()
        {
            for parent in &mut candidate.groups {
                for entry in &mut parent.entries {
                    if entry.target == Target::Group(renamed.id.clone()) {
                        entry.name = renamed.name.clone();
                    }
                }
            }
        }
        self.commit(candidate)
    }

    fn edit_entry(&mut self, create: bool) -> Result<(), String> {
        let group_index = self.group_index().ok_or("No group selected")?;
        let index = if create {
            None
        } else {
            Some(self.entry_index().ok_or("No application selected")?)
        };
        let mut entry = index
            .map(|index| self.config.groups[group_index].entries[index].clone())
            .unwrap_or_default();
        let path = match &entry.target {
            Target::Application(path) => path.clone(),
            Target::Group(_) => {
                return Err("Remove this subgroup and add another to change its target".into());
            }
        };
        let fields = [
            Field::text("Name", &entry.name),
            Field::text("Path, URL or shell:AppsFolder\\AppID", path),
            Field::text("Arguments", &entry.arguments),
            Field::text("Icon path (optional)", &entry.icon),
            Field::text("Tooltip (optional)", &entry.tooltip),
            Field::check("Run as administrator", entry.administrator),
        ];
        let Some(values) = self.form(
            if create {
                "Add application"
            } else {
                "Edit application"
            },
            &fields,
        )?
        else {
            return Ok(());
        };
        entry.name = values[0].trim().into();
        entry.target = Target::Application(values[1].trim().into());
        entry.arguments = values[2].clone();
        entry.icon = values[3].trim().into();
        entry.tooltip = values[4].clone();
        entry.administrator = values[5] == "true";
        let mut candidate = self.config.clone();
        if let Some(index) = index {
            candidate.groups[group_index].entries[index] = entry;
        } else {
            candidate.groups[group_index].entries.push(entry);
        }
        self.commit(candidate)
    }

    pub fn add_targets(&mut self, targets: Vec<(String, String)>) -> Result<(), String> {
        let index = self.group_index().ok_or("No group selected")?;
        let mut candidate = self.config.clone();
        for (path, name) in targets {
            if candidate.groups[index]
                .entries
                .iter()
                .any(|entry| entry.target == Target::Application(path.clone()))
            {
                continue;
            }
            candidate.groups[index].entries.push(Entry {
                name: if name.is_empty() {
                    display_name(&path)
                } else {
                    name
                },
                target: Target::Application(path),
                ..Entry::default()
            });
        }
        self.commit(candidate)
    }

    fn subgroup(&mut self) -> Result<(), String> {
        let index = self.group_index().ok_or("No group selected")?;
        let choices: Vec<_> = self
            .config
            .groups
            .iter()
            .filter(|group| group.id != self.config.groups[index].id)
            .cloned()
            .collect();
        if choices.is_empty() {
            return Err("There are no other groups to add".into());
        }
        let fields = [Field::Choice {
            label: "Group".into(),
            values: choices.iter().map(|group| group.name.clone()).collect(),
            selected: 0,
        }];
        if let Some(values) = self.form("Add subgroup", &fields)? {
            let selected: usize = values[0].parse().map_err(failure)?;
            let group = choices.get(selected).ok_or("No subgroup selected")?;
            let mut candidate = self.config.clone();
            candidate.groups[index].entries.push(Entry {
                name: group.name.clone(),
                target: Target::Group(group.id.clone()),
                ..Entry::default()
            });
            self.commit(candidate)?;
        }
        Ok(())
    }

    fn preferences(&mut self) -> Result<(), String> {
        let settings = &self.config.settings;
        let theme_index = |theme| match theme {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        };
        let fields = [
            Field::choice(
                "Manager theme",
                &["System", "Light", "Dark"],
                theme_index(settings.main_theme),
            ),
            Field::choice(
                "Popup theme",
                &["System", "Light", "Dark"],
                theme_index(settings.popup_theme),
            ),
            Field::choice(
                "Group order",
                &["Manual", "Alphabetical"],
                usize::from(settings.group_sort == SortMode::Alphabetical),
            ),
            Field::check("Start with Windows", settings.start_with_windows),
            Field::check("Show AppStack in the system tray", settings.show_tray),
            Field::check("Enable Launch all", settings.launch_all_enabled),
            Field::check(
                "Grayscale group shortcut icons",
                settings.grayscale_shortcuts,
            ),
            Field::check("Close popup after launching", settings.close_after_launch),
            Field::check("Popup animations", settings.animations),
            Field::check(
                "Hand cursor on clickable popup items",
                settings.pointer_cursor,
            ),
        ];
        let mut fonts = ui::font_families();
        for current in [&settings.title_font, &settings.label_font] {
            if !current.is_empty() && !fonts.contains(current) {
                fonts.push(current.clone());
            }
        }
        let font_field = |label: &str, current: &str| Field::Choice {
            label: label.into(),
            values: fonts.clone(),
            selected: fonts.iter().position(|font| font == current).unwrap_or(0),
        };
        let mut fields = fields.into_iter().collect::<Vec<_>>();
        fields.push(font_field("Folder title font", &settings.title_font));
        fields.push(font_field("Icon label font", &settings.label_font));
        if let Some(values) = self.form("Preferences", &fields)? {
            let themes = [Theme::System, Theme::Light, Theme::Dark];
            let mut candidate = self.config.clone();
            let settings = &mut candidate.settings;
            settings.main_theme = themes[values[0].parse::<usize>().unwrap_or(0).min(2)];
            settings.popup_theme = themes[values[1].parse::<usize>().unwrap_or(0).min(2)];
            settings.group_sort = if values[2] == "1" {
                SortMode::Alphabetical
            } else {
                SortMode::Manual
            };
            settings.start_with_windows = values[3] == "true";
            settings.show_tray = values[4] == "true";
            settings.launch_all_enabled = values[5] == "true";
            settings.grayscale_shortcuts = values[6] == "true";
            settings.close_after_launch = values[7] == "true";
            settings.animations = values[8] == "true";
            settings.pointer_cursor = values[9] == "true";
            let font = |value: &String| {
                value
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| fonts.get(index).cloned())
            };
            if let Some(title) = font(&values[10]) {
                settings.title_font = title;
            }
            if let Some(label) = font(&values[11]) {
                settings.label_font = label;
            }
            self.job(Job::Startup(settings.start_with_windows));
            self.commit(candidate)?;
            ui::set_menu_theme(self.config.settings.main_theme);
            if let Some(manager) = &mut self.manager {
                manager.title_font =
                    Font::with(manager.dpi, 18, &self.config.settings.title_font, 600);
                ui::send(
                    manager.title,
                    WM_SETFONT,
                    WPARAM(manager.title_font.0.0 as usize),
                    LPARAM(1),
                );
                ui::apply_theme(
                    manager.window,
                    self.config.settings.main_theme,
                    &[manager.groups, manager.entries],
                    None,
                );
            }
        }
        Ok(())
    }

    pub(super) fn launch_group(&self, id: &str) -> Result<(), String> {
        if !self.config.settings.launch_all_enabled {
            return Err("Launch all is disabled in Preferences".into());
        }
        self.job(Job::Launch(self.config.launch_plan(id)?));
        Ok(())
    }

    pub(super) fn launch_entry(
        &mut self,
        entry: Entry,
        point: POINT,
        parent: Option<usize>,
    ) -> Result<(), String> {
        match &entry.target {
            Target::Group(id) => self.open_popup(id, point, parent),
            Target::Application(_) => {
                self.job(Job::Launch(vec![entry]));
                if self.config.settings.close_after_launch {
                    self.hide_popups(0);
                }
                Ok(())
            }
        }
    }

    pub fn command(&mut self, id: u16) -> Result<(), String> {
        match id {
            1 => self.show_manager(),
            NEW_GROUP => self.edit_group(true),
            EDIT_GROUP => self.edit_group(false),
            ADD_MANUAL => self.edit_entry(true),
            EDIT_ENTRY => self.edit_entry(false),
            ADD_SUBGROUP => self.subgroup(),
            DUPLICATE_GROUP => {
                let mut group = self.selected_group().ok_or("No group selected")?.clone();
                group.id = new_id();
                group.name.push_str(" copy");
                for entry in &mut group.entries {
                    entry.id = new_id();
                }
                self.selected = Some(group.id.clone());
                let mut candidate = self.config.clone();
                candidate.groups.push(group);
                self.commit(candidate)
            }
            DELETE_GROUP => {
                let group = self.selected_group().ok_or("No group selected")?;
                if shell::confirm(
                    self.owner(),
                    &format!(
                        "Delete '{}' and remove its subgroup references?",
                        group.name
                    ),
                ) {
                    let mut candidate = self.config.clone();
                    candidate.remove_group(&group.id);
                    self.commit(candidate)?;
                }
                Ok(())
            }
            DELETE_ENTRY => {
                let group = self.group_index().ok_or("No group selected")?;
                let entry = self.entry_index().ok_or("No application selected")?;
                let mut candidate = self.config.clone();
                candidate.groups[group].entries.remove(entry);
                self.commit(candidate)
            }
            ADD_FILES => {
                let paths = shell::select_files(self.owner(), false, true)?;
                if paths.is_empty() {
                    return Ok(());
                }
                self.add_targets(
                    paths
                        .into_iter()
                        .map(|path| (path.to_string_lossy().into_owned(), String::new()))
                        .collect(),
                )
            }
            GROUP_ICON | ENTRY_ICON => {
                if let Some(path) = shell::select_files(self.owner(), false, false)?.first() {
                    let mut candidate = self.config.clone();
                    let group = self.group_index().ok_or("No group selected")?;
                    if id == GROUP_ICON {
                        candidate.groups[group].appearance.icon =
                            path.to_string_lossy().into_owned();
                        candidate.groups[group].appearance.grid_icon = false;
                    } else {
                        let entry = self.entry_index().ok_or("No application selected")?;
                        candidate.groups[group].entries[entry].icon =
                            path.to_string_lossy().into_owned();
                    }
                    self.commit(candidate)?;
                }
                Ok(())
            }
            OPEN_GROUP => {
                let group = self.selected_group().ok_or("No group selected")?.id.clone();
                let mut point = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut point);
                }
                self.open_popup(&group, point, None)
            }
            SHORTCUT => {
                let group = self.selected_group().ok_or("No group selected")?.clone();
                self.job(Job::Shortcut(group, true));
                Ok(())
            }
            SHORTCUT_FOLDER => {
                let group = self.selected_group().ok_or("No group selected")?.clone();
                let path = shell::shortcut(&self.store, &group, None)?;
                shell::open_path(path.parent().ok_or("Shortcut has no folder")?)
            }
            LAUNCH_ALL => self.launch_group(&self.selected_group().ok_or("No group selected")?.id),
            LAUNCH_ENTRY | LAUNCH_ADMIN => {
                let mut entry = self.selected_group().ok_or("No group selected")?.entries
                    [self.entry_index().ok_or("No application selected")?]
                .clone();
                if id == LAUNCH_ADMIN {
                    entry.administrator = true;
                }
                let mut point = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut point);
                }
                self.launch_entry(entry, point, None)
            }
            GROUP_UP | GROUP_DOWN | ENTRY_UP | ENTRY_DOWN => {
                let mut candidate = self.config.clone();
                if id == GROUP_UP || id == GROUP_DOWN {
                    if candidate.settings.group_sort != SortMode::Manual {
                        return Err("Switch group order to Manual before reordering".into());
                    }
                    let from = self.group_index().ok_or("No group selected")?;
                    let to = if id == GROUP_UP {
                        from.saturating_sub(1)
                    } else {
                        (from + 1).min(candidate.groups.len() - 1)
                    };
                    move_item(&mut candidate.groups, from, to);
                } else {
                    let group = self.group_index().ok_or("No group selected")?;
                    if candidate.groups[group].appearance.sort != SortMode::Manual {
                        return Err("Switch application order to Manual before reordering".into());
                    }
                    let from = self.entry_index().ok_or("No application selected")?;
                    let to = if id == ENTRY_UP {
                        from.saturating_sub(1)
                    } else {
                        (from + 1).min(candidate.groups[group].entries.len() - 1)
                    };
                    move_item(&mut candidate.groups[group].entries, from, to);
                }
                self.commit(candidate)
            }
            IMPORT | IMPORT_FOLDER => {
                if let Some(path) =
                    shell::select_files(self.owner(), id == IMPORT_FOLDER, false)?.first()
                {
                    self.job(Job::Import(path.clone()));
                }
                Ok(())
            }
            EXPORT => {
                if let Some(path) = shell::save_file(self.owner(), "AppStack-backup.agz")? {
                    self.job(Job::Export(self.config.clone(), path));
                }
                Ok(())
            }
            DATA_FOLDER => shell::open_path(&self.store.root),
            SETTINGS => self.preferences(),
            ABOUT => {
                shell::message(
                    Some(self.owner()),
                    "AppStack",
                    "AppStack 1.0.0\nNative application groups for Windows\n\nMIT license.",
                    false,
                );
                Ok(())
            }
            QUIT => {
                self.quitting = true;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn update_tray(&mut self) {
        for (id, _) in &self.tray_ids {
            shell::tray(
                self.host,
                *id,
                "",
                self.brand.handle(),
                ui::TRAY,
                NIM_DELETE,
            );
        }
        self.tray_ids.clear();
        if self.config.settings.show_tray {
            shell::tray(
                self.host,
                1,
                crate::APP_NAME,
                self.brand.handle(),
                ui::TRAY,
                NIM_ADD,
            );
            self.tray_ids.push((1, None));
        }
        let mut requested = Vec::new();
        for (index, group) in self.config.groups.iter().enumerate() {
            if group.appearance.show_on_tray {
                let id = index as u32 + 10;
                let spec = jobs::group_spec(&self.config, group, 32, false);
                let icon = self.icon_cache.get(&spec.key()).unwrap_or(&self.brand);
                shell::tray(self.host, id, &group.name, icon.handle(), ui::TRAY, NIM_ADD);
                requested.push(spec);
                self.tray_ids.push((id, Some(group.id.clone())));
            }
        }
        for spec in requested {
            self.request_icon(spec);
        }
    }

    pub(super) fn menu_at(
        &mut self,
        window: HWND,
        commands: &[(u16, &str)],
        point: POINT,
    ) -> Result<(), String> {
        unsafe {
            let menu = CreatePopupMenu().map_err(failure)?;
            for (id, title) in commands {
                AppendMenuW(menu, MF_STRING, *id as usize, PCWSTR(wide(title).as_ptr()))
                    .map_err(failure)?;
            }
            let _ = SetForegroundWindow(window);
            let chosen = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                None,
                window,
                None,
            )
            .0;
            let _ = DestroyMenu(menu);
            if chosen != 0 {
                self.command(chosen as u16)?;
            }
        }
        Ok(())
    }

    pub fn event(&mut self, event: ui::Event) -> Result<(), String> {
        match event.code {
            WM_COPYDATA => self.request(serde_json::from_str(&event.text).map_err(failure)?),
            ui::ASYNC => {
                self.completed()?;
                Ok(())
            }
            ui::SELECT => {
                if self
                    .manager
                    .as_ref()
                    .is_some_and(|manager| manager.groups == event.window)
                {
                    self.selected = self
                        .manager
                        .as_ref()
                        .and_then(|manager| manager.group_ids.get(event.first).cloned());
                    self.refresh_entries();
                }
                Ok(())
            }
            ui::INVOKE_ITEM | ui::ACTIVATE_ITEM => {
                if let Some(index) = self
                    .popups
                    .iter()
                    .position(|popup| popup.list == event.window)
                {
                    if let Some(slot) = self.popups[index].slots.get(event.first) {
                        let entry = self.popups[index].entries[slot.entry].clone();
                        let mut bounds = RECT::default();
                        let mut item = RECT {
                            left: LVIR_BOUNDS as i32,
                            ..Default::default()
                        };
                        unsafe {
                            let _ = GetWindowRect(event.window, &mut bounds);
                        }
                        ui::send(
                            event.window,
                            LVM_GETITEMRECT,
                            WPARAM(event.first),
                            LPARAM(&mut item as *mut _ as isize),
                        );
                        return self.launch_entry(
                            entry,
                            POINT {
                                x: bounds.left + item.right,
                                y: bounds.top + item.top,
                            },
                            Some(index),
                        );
                    }
                    Ok(())
                } else if event.code == ui::ACTIVATE_ITEM
                    && self
                        .manager
                        .as_ref()
                        .is_some_and(|manager| manager.groups == event.window)
                {
                    self.command(OPEN_GROUP)
                } else if event.code == ui::ACTIVATE_ITEM
                    && self
                        .manager
                        .as_ref()
                        .is_some_and(|manager| manager.entries == event.window)
                {
                    self.command(LAUNCH_ENTRY)
                } else {
                    Ok(())
                }
            }
            ui::DROP => self.add_targets(serde_json::from_str(&event.text).map_err(failure)?),
            ui::BEGIN_DRAG => {
                if !self.manager.as_ref().is_some_and(|manager| {
                    event.window == manager.groups || event.window == manager.entries
                }) {
                    return Ok(());
                }
                self.drag = Some((event.window, event.first, event.first));
                unsafe {
                    SetCapture(self.owner());
                    SetCursor(Some(ui::grab_cursor(self.owner())));
                }
                Ok(())
            }
            WM_CAPTURECHANGED => {
                // Capture taken away mid-drag (Alt+Tab, a dialog): put the row back.
                if let Some((list, from, at)) = self.drag.take() {
                    self.move_row(list, at, from);
                }
                Ok(())
            }
            WM_MOUSEMOVE => {
                let (Some((list, from, at)), Some(manager)) = (self.drag, &self.manager) else {
                    return Ok(());
                };
                let (window, groups) = (manager.window, manager.groups);
                let mut point = POINT::default();
                let mut bounds = RECT::default();
                unsafe {
                    SetCursor(Some(ui::grab_cursor(window)));
                    let _ = GetCursorPos(&mut point);
                    let _ = GetWindowRect(window, &mut bounds);
                }
                if list == groups
                    && (point.x < bounds.left
                        || point.x > bounds.right
                        || point.y < bounds.top
                        || point.y > bounds.bottom)
                {
                    // Leaving the window makes a desktop shortcut instead of reordering.
                    self.drag = None;
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    self.move_row(list, at, from);
                    let group = self
                        .manager
                        .as_ref()
                        .and_then(|manager| manager.group_ids.get(from))
                        .and_then(|id| self.config.group(id))
                        .cloned();
                    if let Some(group) = group {
                        let icon = jobs::group_icon_path(&self.store, &group);
                        let shortcut = shell::shortcut(
                            &self.store,
                            &group,
                            icon.is_file().then_some(icon.as_path()),
                        )?;
                        shell::drag_shortcut(self.owner(), &shortcut)?;
                    }
                    return Ok(());
                }
                if self.manual_order(list).is_err() {
                    return Ok(());
                }
                // Rows are equally tall: the cursor's offset from the dragged row is where it goes.
                let count = ui::send(list, LVM_GETITEMCOUNT, WPARAM(0), LPARAM(0)).0 as i32;
                let mut row = RECT {
                    left: LVIR_BOUNDS as i32,
                    ..Default::default()
                };
                ui::send(
                    list,
                    LVM_GETITEMRECT,
                    WPARAM(at),
                    LPARAM(&mut row as *mut _ as isize),
                );
                unsafe {
                    let _ = ScreenToClient(list, &mut point);
                }
                let height = row.bottom - row.top;
                if height > 0 && count > 0 {
                    let to = (at as i32 + (point.y - row.top).div_euclid(height))
                        .clamp(0, count - 1) as usize;
                    if to != at {
                        self.move_row(list, at, to);
                        self.drag = Some((list, from, to));
                    }
                }
                Ok(())
            }
            WM_LBUTTONUP => {
                if let Some(index) = self
                    .popups
                    .iter()
                    .position(|popup| popup.viewport == event.window && popup.paged)
                {
                    let point = POINT {
                        x: (event.second & 0xFFFF) as i16 as i32,
                        y: ((event.second >> 16) & 0xFFFF) as i16 as i32,
                    };
                    if let Some(page) = ui::pager_hit(event.window, point) {
                        self.go_page(index, page);
                    }
                    return Ok(());
                }
                if let Some((list, from, at)) = self.drag.take() {
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    // The rows already moved live; saving the order is all that is left.
                    if at != from {
                        return self.commit(self.config.clone());
                    }
                    // Sorted lists do not follow the cursor; dropping on another row says why.
                    let mut point = POINT::default();
                    unsafe {
                        let _ = GetCursorPos(&mut point);
                        let _ = ScreenToClient(list, &mut point);
                    }
                    let mut hit = LVHITTESTINFO {
                        pt: point,
                        ..Default::default()
                    };
                    let target = ui::send(
                        list,
                        LVM_HITTEST,
                        WPARAM(0),
                        LPARAM(&mut hit as *mut _ as isize),
                    )
                    .0;
                    if target >= 0 && target as usize != from {
                        self.manual_order(list)?;
                    }
                }
                Ok(())
            }
            WM_VSCROLL | WM_MOUSEWHEEL => {
                if let Some(index) = self.popups.iter().position(|popup| {
                    popup.window == event.window || popup.viewport == event.window
                }) {
                    self.hide_popups(index + 1);
                    let popup = &mut self.popups[index];
                    if popup.paged {
                        let delta = ((event.first >> 16) & 0xFFFF) as u16 as i16 as i32;
                        // Any notch turns a page; the rest of a fast spin or touchpad gesture is ignored.
                        let ready = popup
                            .flipped
                            .is_none_or(|at| at.elapsed() > std::time::Duration::from_millis(180));
                        if event.code == WM_MOUSEWHEEL && delta != 0 && ready {
                            popup.flipped = Some(std::time::Instant::now());
                            let page =
                                popup.target / popup.page_height() + if delta < 0 { 1 } else { -1 };
                            self.go_page(index, page);
                        }
                        return Ok(());
                    }
                    let total = popup.content_height();
                    let maximum = (total - popup.viewport_height).max(0);
                    let scroll = if event.code == WM_MOUSEWHEEL {
                        let delta = ((event.first >> 16) & 0xFFFF) as u16 as i16 as i32;
                        popup.scroll - delta * popup.cell_height / 120
                    } else {
                        match event.first & 0xFFFF {
                            0 => popup.scroll - popup.cell_height,
                            1 => popup.scroll + popup.cell_height,
                            2 => popup.scroll - popup.viewport_height,
                            3 => popup.scroll + popup.viewport_height,
                            4 | 5 => {
                                let mut info = SCROLLINFO {
                                    cbSize: size_of::<SCROLLINFO>() as u32,
                                    fMask: SIF_TRACKPOS,
                                    ..Default::default()
                                };
                                unsafe {
                                    let _ = GetScrollInfo(popup.viewport, SB_VERT, &mut info);
                                }
                                info.nTrackPos
                            }
                            6 => 0,
                            7 => maximum,
                            _ => popup.scroll,
                        }
                    };
                    popup.scroll = scroll.clamp(0, maximum);
                    self.render_popup(index)?;
                }
                Ok(())
            }
            WM_COMMAND => {
                if event.first >> 16 != BN_CLICKED as usize {
                    return Ok(());
                }
                let id = (event.first & 0xFFFF) as u16;
                if let Some(index) = self.popups.iter().position(|popup| {
                    popup.window == event.window || popup.viewport == event.window
                }) {
                    if id == BACK {
                        return self.navigate(index, None);
                    }
                    if id == EDIT_GROUP || id == LAUNCH_ALL {
                        self.selected = Some(self.popups[index].current().to_string());
                        if id == EDIT_GROUP {
                            self.show_manager()?;
                        }
                        return self.command(id);
                    }
                    Ok(())
                } else {
                    self.command(id)
                }
            }
            WM_TIMER => {
                if let Some(index) = self
                    .popups
                    .iter()
                    .position(|popup| popup.viewport == event.window)
                {
                    let popup = &mut self.popups[index];
                    // Ease out: every tick covers a third of the remaining distance.
                    let remaining = popup.target - popup.scroll;
                    popup.scroll += if remaining / 3 == 0 {
                        remaining
                    } else {
                        remaining / 3
                    };
                    if popup.scroll == popup.target {
                        unsafe {
                            let _ = KillTimer(Some(popup.viewport), ANIMATION);
                        }
                    }
                    self.render_popup(index)?;
                }
                Ok(())
            }
            WM_CLOSE => {
                if self
                    .manager
                    .as_ref()
                    .is_some_and(|manager| manager.window == event.window)
                {
                    if self.tray_ids.is_empty() {
                        self.quitting = true;
                    } else {
                        self.manager.take();
                    }
                } else {
                    self.hide_popups(0);
                }
                Ok(())
            }
            WM_SIZE => {
                if self
                    .manager
                    .as_ref()
                    .is_some_and(|manager| manager.window == event.window)
                {
                    self.layout_manager();
                }
                Ok(())
            }
            WM_ACTIVATE if event.first & 0xFFFF == WA_INACTIVE as usize => {
                if self.popups.iter().any(|popup| popup.window == event.window) {
                    let next = unsafe { GetForegroundWindow() };
                    let within = self.popups.iter().any(|popup| {
                        popup.window == next || unsafe { IsChild(popup.window, next).as_bool() }
                    });
                    if !within {
                        self.hide_popups(0);
                    }
                }
                Ok(())
            }
            WM_DPICHANGED => {
                if let Some(manager) = &mut self.manager {
                    if manager.window == event.window {
                        manager.dpi = unsafe { GetDpiForWindow(manager.window) }.max(96);
                        manager.font = Font::new(manager.dpi, 14);
                        manager.title_font =
                            Font::with(manager.dpi, 18, &self.config.settings.title_font, 600);
                        for window in [manager.groups, manager.entries, manager.status]
                            .into_iter()
                            .chain(manager.buttons.iter().map(|(_, button)| *button))
                        {
                            ui::send(
                                window,
                                WM_SETFONT,
                                WPARAM(manager.font.0.0 as usize),
                                LPARAM(1),
                            );
                        }
                        ui::send(
                            manager.title,
                            WM_SETFONT,
                            WPARAM(manager.title_font.0.0 as usize),
                            LPARAM(1),
                        );
                        self.layout_manager();
                        self.refresh_manager();
                    }
                }
                if let Some(index) = self.popups.iter().position(|popup| {
                    popup.window == event.window
                        && unsafe { GetDpiForWindow(popup.window) } != popup.dpi
                }) {
                    let id = self.popups[index].group_id.clone();
                    let mut bounds = RECT::default();
                    unsafe {
                        let _ = GetWindowRect(event.window, &mut bounds);
                    }
                    self.hide_popups(index);
                    self.parked.retain(|popup| popup.group_id != id);
                    self.open_popup(
                        &id,
                        POINT {
                            x: (bounds.left + bounds.right) / 2,
                            y: bounds.bottom,
                        },
                        index.checked_sub(1),
                    )?;
                }
                Ok(())
            }
            WM_DISPLAYCHANGE => {
                self.hide_popups(0);
                self.parked.clear();
                Ok(())
            }
            WM_SETTINGCHANGE => {
                ui::set_menu_theme(self.config.settings.main_theme);
                if let Some(manager) = &self.manager {
                    ui::apply_theme(
                        manager.window,
                        self.config.settings.main_theme,
                        &[manager.groups, manager.entries],
                        None,
                    );
                }
                for popup in self.popups.iter().chain(self.parked.iter()) {
                    let accent = self
                        .config
                        .group(&popup.group_id)
                        .and_then(|group| parse_color(&group.appearance.accent));
                    ui::apply_theme(popup.window, self.config.settings.popup_theme, &[], accent);
                    ui::apply_theme(
                        popup.viewport,
                        self.config.settings.popup_theme,
                        &[popup.list],
                        accent,
                    );
                }
                Ok(())
            }
            ui::TRAY => {
                let mut point = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut point);
                }
                if event.second as u32 == WM_LBUTTONUP {
                    if let Some(Some(id)) = self
                        .tray_ids
                        .iter()
                        .find(|(id, _)| *id == event.first as u32)
                        .map(|(_, id)| id.clone())
                    {
                        self.open_popup(&id, point, None)
                    } else {
                        self.show_manager()
                    }
                } else if event.second as u32 == WM_RBUTTONUP {
                    self.menu_at(
                        self.host,
                        &[
                            (1, "Open AppStack"),
                            (SETTINGS, "Preferences..."),
                            (QUIT, "Exit"),
                        ],
                        point,
                    )
                } else {
                    Ok(())
                }
            }
            WM_CONTEXTMENU => {
                let mut point = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut point);
                }
                let source = HWND(event.first as *mut _);
                if self
                    .manager
                    .as_ref()
                    .is_some_and(|manager| manager.groups == source)
                {
                    self.menu_at(
                        self.owner(),
                        &[
                            (OPEN_GROUP, "Open"),
                            (EDIT_GROUP, "Edit..."),
                            (DUPLICATE_GROUP, "Duplicate"),
                            (SHORTCUT, "Create desktop shortcut"),
                            (DELETE_GROUP, "Delete"),
                        ],
                        point,
                    )
                } else if self
                    .manager
                    .as_ref()
                    .is_some_and(|manager| manager.entries == source)
                {
                    self.menu_at(
                        self.owner(),
                        &[
                            (LAUNCH_ENTRY, "Launch"),
                            (LAUNCH_ADMIN, "Run as administrator"),
                            (EDIT_ENTRY, "Edit..."),
                            (DELETE_ENTRY, "Remove"),
                        ],
                        point,
                    )
                } else {
                    Ok(())
                }
            }
            _ if event.code == self.taskbar_created => {
                self.update_tray();
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn completed(&mut self) -> Result<(), String> {
        while let Some(completed) = self
            .deferred_results
            .pop_front()
            .or_else(|| self.background.completed.try_recv().ok())
        {
            match completed {
                Completed::Saved => {}
                Completed::Error(error) => {
                    shell::message(Some(self.owner()), "AppStack", &error, true)
                }
                Completed::Status(status) => {
                    if !status.is_empty() {
                        if let Some(manager) = &self.manager {
                            ui::set_text(manager.status, &status);
                        }
                    }
                }
                Completed::Exported(result) => {
                    result?;
                    shell::message(
                        Some(self.owner()),
                        "Backup exported",
                        "Your groups and custom icons have been exported.",
                        false,
                    );
                }
                Completed::Shortcut(result) => {
                    result?;
                }
                Completed::Imported(result) => {
                    let groups = result?;
                    self.show_manager()?;
                    let items: Vec<_> = groups
                        .iter()
                        .map(|group| (group.name.clone(), group.entries.len().to_string()))
                        .collect();
                    if let Some(indices) = ui::checklist(
                        self.owner(),
                        "Import groups",
                        &items,
                        self.config.settings.main_theme,
                    )? {
                        let selected = indices
                            .into_iter()
                            .filter_map(|index| groups.get(index))
                            .map(|group| group.id.clone())
                            .collect();
                        let groups = selected_with_dependencies(&groups, &selected);
                        if !groups.is_empty() {
                            let mut candidate = self.config.clone();
                            candidate.append_import(groups)?;
                            self.commit(candidate)?;
                        }
                    }
                }
                Completed::Icon {
                    generation,
                    key,
                    result,
                } => self.accept_icon(generation, key, result),
            }
        }
        Ok(())
    }

    fn accept_icon(
        &mut self,
        generation: u64,
        key: u64,
        result: Result<(Icon, std::path::PathBuf), String>,
    ) {
        if generation != self.generation {
            return;
        }
        self.pending.remove(&key);
        if let Ok((icon, _path)) = result {
            self.icon_cache.insert(key, icon);
            self.icon_order.push_back(key);
            while self.icon_cache.len() > 256 {
                if let Some(oldest) = self.icon_order.pop_front() {
                    self.icon_cache.remove(&oldest);
                } else {
                    break;
                }
            }
            self.refresh_icon(key);
        }
    }

    fn refresh_icon(&mut self, key: u64) {
        let Some(icon) = self.icon_cache.get(&key) else {
            return;
        };
        for (tray_id, group_id) in &self.tray_ids {
            if let Some(group) = group_id.as_deref().and_then(|id| self.config.group(id)) {
                if jobs::group_spec(&self.config, group, 32, false).key() == key {
                    shell::tray(
                        self.host,
                        *tray_id,
                        &group.name,
                        icon.handle(),
                        ui::TRAY,
                        NIM_MODIFY,
                    );
                }
            }
        }
        if let Some(manager) = &self.manager {
            for (index, spec) in manager.group_specs.iter().enumerate() {
                if spec.key() == key {
                    manager.group_images.replace(index, icon.handle());
                }
            }
            for (index, spec) in manager.entry_specs.iter().enumerate() {
                if spec.key() == key {
                    manager.entry_images.replace(index, icon.handle());
                }
            }
            unsafe {
                let _ = InvalidateRect(Some(manager.groups), None, false);
                let _ = InvalidateRect(Some(manager.entries), None, false);
            }
        }
        for popup in self.popups.iter_mut().chain(self.parked.iter_mut()) {
            if let Some(index) = popup.images.keys.get(&key) {
                popup.images.replace(*index as usize, icon.handle());
                unsafe {
                    let _ = InvalidateRect(Some(popup.list), None, false);
                }
            }
        }
    }

    pub fn open_popup(
        &mut self,
        id: &str,
        point: POINT,
        parent: Option<usize>,
    ) -> Result<(), String> {
        self.build_popup(id, point, parent, true)
    }

    fn build_popup(
        &mut self,
        id: &str,
        point: POINT,
        parent: Option<usize>,
        activate: bool,
    ) -> Result<(), String> {
        let popup_clock = std::time::Instant::now();
        let popup_trace = std::env::var_os("APPSTACK_STARTUP_TRACE");
        let mut popup_stages = Vec::new();
        macro_rules! popup_stage {
            ($name:literal) => {
                if popup_trace.is_some() {
                    popup_stages.push(($name, popup_clock.elapsed().as_micros()));
                }
            };
        }
        if let Some(parent) = parent {
            return self.navigate(parent, Some(id));
        }
        if self
            .popups
            .first()
            .is_some_and(|popup| popup.group_id == id)
        {
            self.hide_popups(0);
            return Ok(());
        }
        self.hide_popups(0);
        unsafe {
            let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return Err("Could not find the target monitor".into());
            }
            if let Some(cached) = self.parked.iter().position(|popup| {
                popup.group_id == id
                    && popup.monitor == monitor.0 as usize
                    && popup.work_area == info.rcWork
                    && GetDpiForWindow(popup.window) == popup.dpi
            }) {
                if !activate {
                    return Ok(());
                }
                let popup = self
                    .parked
                    .remove(cached)
                    .ok_or("Popup cache changed unexpectedly")?;
                let mut rectangle = RECT::default();
                let _ = GetWindowRect(popup.window, &mut rectangle);
                let width = rectangle.right - rectangle.left;
                let height = rectangle.bottom - rectangle.top;
                let (left, top) =
                    popup_position(point, info.rcWork, width, height, false, popup.dpi);
                let _ = SetWindowPos(
                    popup.window,
                    Some(HWND_TOPMOST),
                    left,
                    top,
                    width,
                    height,
                    SWP_NOACTIVATE,
                );
                let window = popup.window;
                let focus = popup.list;
                // Like the Start menu, a reopened popup starts at the first page of its root.
                let moved = !popup.path.is_empty() || popup.scroll != 0 || popup.target != 0;
                self.popups.push(popup);
                let index = self.popups.len() - 1;
                if moved {
                    self.popups[index].path.clear();
                    self.show_folder(index)?;
                }
                show_popup(window, focus, self.config.settings.animations)?;
                return Ok(());
            }
            let mut group = self.config.group(id).ok_or("Group not found")?.clone();
            let entries = if group.appearance.sort == SortMode::Manual {
                std::mem::take(&mut group.entries)
            } else {
                group.ordered_entries().into_iter().cloned().collect()
            };
            popup_stage!("entries");
            let window = ui::window(
                "AppStack.Popup",
                &group.name,
                WS_POPUP | WS_CLIPCHILDREN,
                // Layered only to fade in; a layered window stays hidden until given an opacity.
                WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | if self.config.settings.animations {
                        WS_EX_LAYERED
                    } else {
                        WINDOW_EX_STYLE(0)
                    },
                Some(self.host),
                1,
                1,
            )?;
            popup_stage!("window");
            let _ = shell::window_app_id(window, &shell::group_app_id(&group));
            popup_stage!("shell_identity");
            let _ = SetWindowPos(
                window,
                None,
                info.rcWork.left,
                info.rcWork.top,
                1,
                1,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let dpi = GetDpiForWindow(window).max(96);
            let settings = &self.config.settings;
            let appearance = &group.appearance;
            let right_labels = appearance.label_position == LabelPosition::Right;
            let cell = appearance.cell_size();
            let mut look = ui::Look {
                cell: (ui::scale(cell.0, dpi), ui::scale(cell.1, dpi)),
                gap: (
                    ui::scale(appearance.gap_horizontal as i32, dpi),
                    ui::scale(appearance.gap_vertical as i32, dpi),
                ),
                radius: ui::scale(appearance.tile_radius as i32, dpi),
                icon: ui::scale(appearance.icon_size as i32, dpi),
                dpi,
                labels: appearance.show_labels,
                right_labels,
                card: appearance.card_layout,
                accent: parse_color(&appearance.accent).map(ui::colorref),
                hot: -1,
                spawn: None,
            };
            let (mut cell_width, cell_height) = look.cell;
            let show_edit = appearance.show_edit_button;
            let show_all = appearance.show_launch_all_button && settings.launch_all_enabled;
            // Each band is Some(true) at the bottom of the popup and Some(false) at the top.
            let title_band = appearance.show_header.then_some(appearance.header_bottom);
            let button_band = (show_edit || show_all)
                .then_some(appearance.button_position == ButtonPosition::Bottom);
            let top_band = title_band == Some(false) || button_band == Some(false);
            let bottom_band = title_band == Some(true) || button_band == Some(true);
            let folders = self.config.folders(id);
            let widest = folders
                .iter()
                .map(|folder| folder.entries.len())
                .max()
                .unwrap_or(0);
            // Like Tailwind's border-box: the border sits inside, before the padding.
            let border = appearance.border_width as i32;
            let mut margin = ui::scale(appearance.padding_horizontal as i32 + border, dpi);
            let mut vertical_margin = ui::scale(appearance.padding_vertical as i32 + border, dpi);
            let band = ui::scale(40, dpi);
            let mut chrome =
                vertical_margin * 2 + band * (i32::from(top_band) + i32::from(bottom_band));
            let scrollbar = GetSystemMetricsForDpi(SM_CXVSCROLL, dpi);
            let maximum_width = info.rcWork.right - info.rcWork.left - ui::scale(16, dpi);
            let available_columns = ((maximum_width - margin * 2 - scrollbar + look.gap.0)
                / cell_width)
                .max(1) as usize;
            // Subfolders open in this popup, so it is sized once for every folder it can show.
            let columns = appearance.columns.min(widest.max(1)).min(available_columns);
            let rows = entries.len().div_ceil(columns).max(1);
            let mut viewport_height = (appearance.visible_rows(rows) as i32 * cell_height
                - look.gap.1)
                .min(info.rcWork.bottom - info.rcWork.top - chrome - ui::scale(20, dpi))
                .max(cell_height - look.gap.1);
            let overflow = rows as i32 * cell_height - look.gap.1 > viewport_height;
            // Subfolders reuse this width, so a popup that can open one keeps the side strip.
            // Paged groups only need room for the dots; scrolling ones need a scroll bar.
            let gutter = match (overflow || folders.len() > 1, appearance.scroll) {
                (false, _) => 0,
                (true, true) => ui::scale(14, dpi),
                (true, false) => scrollbar,
            };
            let fonts = [
                Font::with(
                    dpi,
                    appearance.label_size as i32 + 1,
                    &settings.label_font,
                    400,
                ),
                Font::with(
                    dpi,
                    appearance.label_size as i32 + 4,
                    &settings.title_font,
                    600,
                ),
                Font::new(dpi, 13),
                ui::icon_font(dpi, 16),
            ];
            // A single column hugs its longest name instead of the multi-column cell width.
            if columns == 1 && appearance.show_labels {
                let longest = folders
                    .iter()
                    .flat_map(|folder| &folder.entries)
                    .map(|entry| ui::text_width(&fonts[0], &entry.name))
                    .max()
                    .unwrap_or(0);
                let padding = if right_labels {
                    look.icon + ui::scale(28, dpi)
                } else {
                    ui::scale(16, dpi)
                };
                cell_width = (longest + padding + look.gap.0)
                    .clamp(look.icon + ui::scale(24, dpi) + look.gap.0, cell_width);
                look.cell.0 = cell_width;
            }
            let gap = ui::scale(6, dpi);
            let edit_width = ui::text_width(&fonts[2], "Edit") + ui::scale(24, dpi);
            let all_width = ui::text_width(&fonts[2], "Launch all") + ui::scale(24, dpi);
            let buttons_width = i32::from(show_edit) * edit_width
                + i32::from(show_all) * all_width
                + i32::from(show_edit && show_all) * gap;
            // As wide as the tiles, unless the longest folder title (with the back button in
            // subfolders) or the buttons need more; very long titles still end in an ellipsis.
            let back = ui::scale(30, dpi) + gap;
            let title_width = folders
                .iter()
                .enumerate()
                .filter(|(index, _)| *index > 0 || appearance.show_header)
                .map(|(index, folder)| {
                    ui::text_width(&fonts[1], &folder.name) + if index > 0 { back } else { 0 }
                })
                .max()
                .map(|width| width.min(ui::scale(240, dpi)) + gap)
                .unwrap_or(0);
            // Without a title band, subfolder titles share the buttons band.
            let shared =
                button_band.is_some() && (title_band.is_none() || title_band == button_band);
            let chrome_width = if shared {
                buttons_width + title_width
            } else {
                buttons_width.max(title_width)
            };
            let pill = appearance.corner_radius == ROUNDED_FULL;
            let mut width = (columns as i32 * cell_width - look.gap.0
                + margin * 2
                + gutter * (1 + i32::from(pill)))
            .max(chrome_width + margin * 2)
            .max(ui::scale(appearance.minimum_width as i32, dpi))
            .min(maximum_width);
            // A pill pads its round ends once the real content size is known.
            let logical = |pixels: i32| pixels * 96 / dpi as i32;
            let (inset_x, inset_y) = appearance.rounding_inset(
                logical(width - margin * 2) + border * 2,
                logical(viewport_height + chrome - vertical_margin * 2) + border * 2,
            );
            let (inset_x, inset_y) = (ui::scale(inset_x, dpi), ui::scale(inset_y, dpi));
            margin += inset_x;
            vertical_margin += inset_y;
            chrome += inset_y * 2;
            width = (width + inset_x * 2).min(maximum_width);
            // shortcut: a tall pill that reaches the work area loses rows here without
            // re-checking the gutter; it still scrolls, only the side strip may be missing.
            viewport_height = viewport_height
                .min(info.rcWork.bottom - info.rcWork.top - chrome - ui::scale(20, dpi))
                .max(cell_height - look.gap.1);
            let overflow = rows as i32 * cell_height - look.gap.1 > viewport_height;
            let height = viewport_height + chrome;
            let (left, top) = popup_position(point, info.rcWork, width, height, false, dpi);
            let _ = SetWindowPos(
                window,
                Some(HWND_TOPMOST),
                left,
                top,
                width,
                height,
                SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            ui::popup_corners(window, ui::Frame::of(appearance), dpi)?;
            let viewport = ui::window(
                "AppStack.Viewport",
                "Applications",
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_VSCROLL,
                WS_EX_CONTROLPARENT,
                Some(window),
                width,
                viewport_height,
            )?;
            let viewport_top = vertical_margin + if top_band { band } else { 0 };
            ui::place(
                viewport,
                margin,
                viewport_top,
                width - margin * 2,
                viewport_height,
            );
            let list = ui::child(
                viewport,
                WC_LISTVIEWW,
                "Applications",
                20,
                WS_TABSTOP
                    | WINDOW_STYLE(
                        LVS_NOSCROLL
                            | LVS_SINGLESEL
                            | LVS_SHOWSELALWAYS
                            | LVS_SHAREIMAGELISTS
                            | if right_labels && appearance.show_labels {
                                LVS_SMALLICON
                            } else {
                                LVS_ICON
                            },
                    ),
                &fonts[0],
            )?;
            let (list_left, list_width) = list_span(width - margin * 2, gutter, overflow, pill);
            ui::place(list, list_left, 0, list_width, viewport_height);
            ui::send(
                list,
                LVM_SETEXTENDEDLISTVIEWSTYLE,
                WPARAM(0),
                LPARAM(
                    (LVS_EX_DOUBLEBUFFER
                        | LVS_EX_INFOTIP
                        | if appearance.show_labels {
                            0
                        } else {
                            LVS_EX_HIDELABELS
                        }) as isize,
                ),
            );
            ui::send(
                list,
                LVM_SETICONSPACING,
                WPARAM(0),
                LPARAM(((cell_width as u32 & 0xFFFF) | ((cell_height as u32) << 16)) as isize),
            );
            let images = Images::new(look.icon);
            ui::send(
                list,
                LVM_SETIMAGELIST,
                WPARAM(LVSIL_NORMAL as usize),
                LPARAM(images.handle.0 as isize),
            );
            ui::send(
                list,
                LVM_SETIMAGELIST,
                WPARAM(LVSIL_SMALL as usize),
                LPARAM(images.handle.0 as isize),
            );
            ui::set_look(list, look);
            let band_top = |bottom: bool| {
                if bottom {
                    viewport_top + viewport_height
                } else {
                    vertical_margin
                }
            };
            let mut right_edge = [width - margin; 2];
            if let Some(bottom) = button_band {
                let height = ui::scale(30, dpi);
                let top = band_top(bottom) + (band - height) / 2;
                let buttons: Vec<_> = [
                    (show_edit, EDIT_GROUP, "Edit", edit_width),
                    (show_all, LAUNCH_ALL, "Launch all", all_width),
                ]
                .into_iter()
                .filter(|button| button.0)
                .collect();
                let widths: Vec<_> = buttons.iter().map(|button| button.3).collect();
                let positions = button_positions(
                    margin + if shared { title_width } else { 0 },
                    width - margin,
                    &widths,
                    gap,
                    appearance.button_alignment,
                );
                if let Some(left) = positions.first() {
                    right_edge[usize::from(bottom)] = *left - gap;
                }
                for ((_, id, label, button_width), left) in buttons.into_iter().zip(positions) {
                    let button = ui::child(window, w!("BUTTON"), label, id, WS_TABSTOP, &fonts[2])?;
                    ui::place(button, left, top, button_width, height);
                }
            }
            let title_slot = title_band.or(button_band).map(|bottom| RECT {
                left: margin + gap / 2,
                top: band_top(bottom),
                right: right_edge[usize::from(bottom)] - gap / 2,
                bottom: band_top(bottom) + band,
            });
            let mut title = None;
            if let Some(bottom) = title_band {
                let header_window = ui::child(
                    window,
                    w!("STATIC"),
                    &group.name,
                    5,
                    WINDOW_STYLE(STATIC_END_ELLIPSIS | SS_CENTERIMAGE),
                    &fonts[1],
                )?;
                ui::place(
                    header_window,
                    margin + gap / 2,
                    band_top(bottom),
                    right_edge[usize::from(bottom)] - margin - gap,
                    band,
                );
                title = Some(header_window);
            }
            popup_stage!("layout_controls");
            // AppStack animates popups itself, so DWM's own transition never doubles it.
            let transitions = BOOL::from(true);
            let _ = DwmSetWindowAttribute(
                window,
                DWMWA_TRANSITIONS_FORCEDISABLED,
                (&transitions as *const BOOL).cast(),
                size_of::<BOOL>() as u32,
            );
            let accent = parse_color(&appearance.accent);
            ui::apply_theme(window, settings.popup_theme, &[], accent);
            ui::apply_theme(viewport, settings.popup_theme, &[list], accent);
            if entries.is_empty() {
                let empty = ui::child(
                    viewport,
                    w!("STATIC"),
                    "No applications",
                    6,
                    WINDOW_STYLE(0),
                    &fonts[0],
                )?;
                ui::place(
                    empty,
                    ui::scale(12, dpi),
                    ui::scale(12, dpi),
                    width - margin * 2,
                    ui::scale(24, dpi),
                );
            }
            popup_stage!("theme");
            let index = self.popups.len();
            self.popups.push(Popup {
                window,
                viewport,
                list,
                images,
                group_id: id.into(),
                entries,
                slots: Vec::new(),
                fonts,
                look,
                dpi,
                columns,
                cell_width,
                cell_height,
                viewport_height,
                scroll: 0,
                monitor: monitor.0 as usize,
                work_area: info.rcWork,
                path: Vec::new(),
                title,
                back: None,
                title_slot,
                show_title: appearance.show_header,
                grown: false,
                margin,
                vertical_margin,
                frame: ui::Frame::of(appearance),
                width,
                band,
                gutter,
                page_rows: ((viewport_height + look.gap.1) / cell_height).max(1) as usize,
                pages: appearance.scroll,
                paged: appearance.scroll
                    && rows as i32 * cell_height - look.gap.1 > viewport_height,
                target: 0,
                flipped: None,
            });
            self.render_popup(index)?;
            popup_stage!("item_controls");
            if activate {
                show_popup(window, list, self.config.settings.animations)?;
            } else {
                self.hide_popups(index);
            }
            popup_stage!("visible");
        }
        if let Some(path) = popup_trace {
            let path = std::path::PathBuf::from(path).with_extension("popup.json");
            let report = serde_json::json!({ "stages_us": popup_stages });
            let _ = std::fs::write(path, report.to_string());
        }
        Ok(())
    }

    fn render_popup(&mut self, index: usize) -> Result<(), String> {
        let popup = &mut self.popups[index];
        let capacity = ((popup.viewport_height / popup.cell_height + 2) as usize * popup.columns)
            .min(popup.entries.len());
        let first = (popup.scroll / popup.cell_height) as usize * popup.columns;
        let icon_size = popup.look.icon;
        let (icon_left, icon_top) = popup.look.icon_offset();
        unsafe {
            let mut viewport_bounds = RECT::default();
            let _ = GetClientRect(popup.list, &mut viewport_bounds);
            let inset = ((viewport_bounds.right - popup.columns as i32 * popup.cell_width
                + popup.look.gap.0)
                / 2)
            .max(0);
            let count = capacity.min(popup.entries.len().saturating_sub(first));
            while popup.slots.len() > count {
                let last = popup.slots.len() - 1;
                ui::send(popup.list, LVM_DELETEITEM, WPARAM(last), LPARAM(0));
                popup.slots.pop();
            }
            ui::send(popup.list, WM_SETREDRAW, WPARAM(0), LPARAM(0));
            let reset_images = popup.images.keys.len() + count > 256;
            if reset_images {
                popup.images.clear();
            }
            let mut requested = Vec::new();
            let mut tips = Vec::with_capacity(count);
            for slot_index in 0..count {
                let entry_index = first + slot_index;
                let entry = &popup.entries[entry_index];
                let spec = jobs::entry_spec(&self.config, entry, icon_size as u32);
                let key = spec.key();
                let new = slot_index == popup.slots.len();
                if new
                    || reset_images
                    || popup.slots[slot_index].entry != entry_index
                    || popup.slots[slot_index].key != key
                {
                    let icon = self.icon_cache.get(&key).unwrap_or(&self.brand);
                    let image = popup.images.shared(key, icon.handle());
                    let mut name = wide(&entry.name);
                    let item = LVITEMW {
                        mask: LVIF_TEXT | LVIF_IMAGE,
                        iItem: slot_index as i32,
                        pszText: windows::core::PWSTR(name.as_mut_ptr()),
                        iImage: image,
                        ..Default::default()
                    };
                    ui::send(
                        popup.list,
                        if new { LVM_INSERTITEMW } else { LVM_SETITEMW },
                        WPARAM(0),
                        LPARAM(&item as *const _ as isize),
                    );
                    if new {
                        popup.slots.push(Slot {
                            entry: entry_index,
                            key,
                        });
                    } else {
                        popup.slots[slot_index] = Slot {
                            entry: entry_index,
                            key,
                        };
                    }
                }
                let position = POINT {
                    x: inset + (entry_index % popup.columns) as i32 * popup.cell_width + icon_left,
                    y: (entry_index / popup.columns) as i32 * popup.cell_height - popup.scroll
                        + icon_top,
                };
                ui::send(
                    popup.list,
                    LVM_SETITEMPOSITION32,
                    WPARAM(slot_index),
                    LPARAM(&position as *const _ as isize),
                );
                tips.push(wide(if entry.tooltip.is_empty() {
                    &entry.name
                } else {
                    &entry.tooltip
                }));
                requested.push(spec);
            }
            ui::item_tips(popup.list, tips);
            ui::send(popup.list, WM_SETREDRAW, WPARAM(1), LPARAM(0));
            let _ = InvalidateRect(Some(popup.list), None, false);
            if popup.paged {
                let _ = ShowScrollBar(popup.viewport, SB_VERT, false);
                ui::set_pager(
                    popup.viewport,
                    Some((popup.page_count(), popup.target / popup.page_height())),
                    popup.gutter,
                    popup.dpi,
                );
            } else {
                ui::set_pager(popup.viewport, None, 0, 0);
                let total = popup.content_height();
                let info = SCROLLINFO {
                    cbSize: size_of::<SCROLLINFO>() as u32,
                    fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
                    nMin: 0,
                    nMax: total.saturating_sub(1),
                    nPage: popup.viewport_height as u32,
                    nPos: popup.scroll,
                    ..Default::default()
                };
                SetScrollInfo(popup.viewport, SB_VERT, &info, true);
            }
            // Scrolling moves items under a still cursor, so the hovered item changes.
            let mut cursor = POINT::default();
            let _ = GetCursorPos(&mut cursor);
            let _ = ScreenToClient(popup.list, &mut cursor);
            ui::track_hover(popup.list, Some(cursor));
            for spec in requested {
                self.request_icon(spec);
            }
        }
        Ok(())
    }

    pub fn scroll(&self, message: &MSG) -> bool {
        if (message.message == WM_MOUSEMOVE || message.message == WM_MOUSELEAVE)
            && ui::track_ghost(message.hwnd, message.message == WM_MOUSEMOVE)
        {
            return false;
        }
        if message.message == WM_MOUSEMOVE || message.message == WM_MOUSELEAVE {
            if self.popups.iter().any(|popup| popup.list == message.hwnd) {
                let point = POINT {
                    x: (message.lParam.0 & 0xFFFF) as i16 as i32,
                    y: ((message.lParam.0 >> 16) & 0xFFFF) as i16 as i32,
                };
                ui::track_hover(
                    message.hwnd,
                    (message.message == WM_MOUSEMOVE).then_some(point),
                );
            }
            return false;
        }
        if message.message != WM_MOUSEWHEEL {
            return false;
        }
        let Some(popup) = self.popups.iter().find(|popup| popup.list == message.hwnd) else {
            return false;
        };
        ui::push(
            popup.viewport,
            WM_MOUSEWHEEL,
            message.wParam.0,
            message.lParam.0,
        );
        true
    }

    pub fn keyboard(&mut self, message: &MSG) -> Result<bool, String> {
        if message.message != WM_KEYDOWN {
            return Ok(false);
        }
        let key = message.wParam.0 as u16;
        let root = unsafe { GetAncestor(message.hwnd, GA_ROOT) };
        if let Some(index) = self.popups.iter().position(|popup| popup.window == root) {
            if !self.popups[index].path.is_empty()
                && (key == VK_ESCAPE.0
                    || (key == VK_BACK.0 && message.hwnd == self.popups[index].list))
            {
                self.navigate(index, None)?;
                return Ok(true);
            }
            if (key == VK_PRIOR.0 || key == VK_NEXT.0) && self.popups[index].paged {
                let popup = &self.popups[index];
                let page =
                    popup.target / popup.page_height() + if key == VK_NEXT.0 { 1 } else { -1 };
                self.go_page(index, page);
                return Ok(true);
            }
            if key == VK_ESCAPE.0 {
                self.hide_popups(index);
                if let Some(parent) = self.popups.last() {
                    unsafe {
                        let _ = SetForegroundWindow(parent.window);
                    }
                }
                return Ok(true);
            }
            if [VK_LEFT.0, VK_RIGHT.0, VK_UP.0, VK_DOWN.0, VK_RETURN.0].contains(&key) {
                let popup = &mut self.popups[index];
                if popup.entries.is_empty() {
                    return Ok(true);
                }
                if message.hwnd != popup.list {
                    return Ok(false);
                }
                let selected = ui::selection(popup.list).unwrap_or(0);
                let current = popup
                    .slots
                    .get(selected)
                    .map(|slot| slot.entry)
                    .unwrap_or(0);
                if key == VK_RETURN.0 {
                    ui::push(popup.list, ui::INVOKE_ITEM, selected, 0);
                    return Ok(true);
                }
                let delta = if key == VK_LEFT.0 {
                    -1
                } else if key == VK_RIGHT.0 {
                    1
                } else if key == VK_UP.0 {
                    -(popup.columns as i32)
                } else {
                    popup.columns as i32
                };
                let next =
                    (current as i32 + delta).clamp(0, popup.entries.len() as i32 - 1) as usize;
                let top = (next / popup.columns) as i32 * popup.cell_height;
                if popup.paged {
                    // The keyboard jumps straight to the page so the selection exists at once.
                    popup.scroll = top / popup.page_height() * popup.page_height();
                    popup.target = popup.scroll;
                    unsafe {
                        let _ = KillTimer(Some(popup.viewport), ANIMATION);
                    }
                } else {
                    if top < popup.scroll {
                        popup.scroll = top;
                    }
                    if top + popup.cell_height - popup.look.gap.1
                        > popup.scroll + popup.viewport_height
                    {
                        popup.scroll =
                            top + popup.cell_height - popup.look.gap.1 - popup.viewport_height;
                    }
                }
                self.render_popup(index)?;
                if let Some(selected) = self.popups[index]
                    .slots
                    .iter()
                    .position(|slot| slot.entry == next)
                {
                    ui::suppress(|| ui::select(self.popups[index].list, selected));
                }
                return Ok(true);
            }
        }
        if let Some(manager) = &self.manager {
            if root == manager.window {
                let command = if key == VK_F2.0 {
                    Some(if message.hwnd == manager.entries {
                        EDIT_ENTRY
                    } else {
                        EDIT_GROUP
                    })
                } else if key == VK_DELETE.0 {
                    Some(if message.hwnd == manager.entries {
                        DELETE_ENTRY
                    } else {
                        DELETE_GROUP
                    })
                } else if key == VK_RETURN.0 {
                    Some(if message.hwnd == manager.entries {
                        LAUNCH_ENTRY
                    } else {
                        OPEN_GROUP
                    })
                } else if key == b'N' as u16 && unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0 {
                    Some(NEW_GROUP)
                } else {
                    None
                };
                if let Some(command) = command {
                    self.command(command)?;
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Opens a subfolder inside its popup like the Start menu, or goes back with `None`.
    fn navigate(&mut self, index: usize, id: Option<&str>) -> Result<(), String> {
        let Some(popup) = self.popups.get_mut(index) else {
            return Ok(());
        };
        match id {
            Some(id) => popup.path.push(id.into()),
            None => {
                popup.path.pop();
            }
        }
        self.show_folder(index)
    }

    /// Loads the popup's current folder in place, keeping the root group's layout.
    fn show_folder(&mut self, index: usize) -> Result<(), String> {
        let popup = &self.popups[index];
        let group = self
            .config
            .group(popup.current())
            .ok_or("Group not found")?;
        let name = group.name.clone();
        let entries: Vec<Entry> = group.ordered_entries().into_iter().cloned().collect();
        let accent = self
            .config
            .group(&popup.group_id)
            .and_then(|root| parse_color(&root.appearance.accent));
        let theme = self.config.settings.popup_theme;
        let appearance = self
            .config
            .group(&popup.group_id)
            .map(|root| root.appearance.clone())
            .unwrap_or_default();
        let popup = &mut self.popups[index];
        let depth = popup.path.len();
        let gap = ui::scale(6, popup.dpi);
        // Moving and resizing would otherwise paint intermediate states and copy stale pixels.
        // Only a visible popup is frozen: re-enabling redraw marks a window visible.
        let visible = unsafe { IsWindowVisible(popup.window).as_bool() };
        if visible {
            ui::send(popup.window, WM_SETREDRAW, WPARAM(0), LPARAM(0));
        }
        unsafe {
            let _ = KillTimer(Some(popup.viewport), ANIMATION);
        }
        // Recycled list items would otherwise keep labels from the previous folder.
        ui::send(popup.list, LVM_DELETEALLITEMS, WPARAM(0), LPARAM(0));
        popup.slots.clear();
        popup.entries = entries;
        popup.scroll = 0;
        popup.target = 0;
        // A popup without bands grows one band upwards to hold the back arrow.
        if popup.title_slot.is_none() && popup.grown != (depth > 0) {
            popup.grown = depth > 0;
            let delta = if popup.grown { popup.band } else { -popup.band };
            let mut bounds = RECT::default();
            unsafe {
                let _ = GetWindowRect(popup.window, &mut bounds);
                let _ = SetWindowPos(
                    popup.window,
                    None,
                    bounds.left,
                    bounds.top - delta,
                    bounds.right - bounds.left,
                    bounds.bottom - bounds.top + delta,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            ui::place(
                popup.viewport,
                popup.margin,
                popup.vertical_margin + if popup.grown { popup.band } else { 0 },
                popup.width - popup.margin * 2,
                popup.viewport_height,
            );
        }
        // Each folder gets the rows it needs up to the group's visible rows; the popup grows or
        // shrinks upwards and whatever sits below the viewport follows its bottom edge.
        let rows = popup.entries.len().div_ceil(popup.columns).max(1);
        unsafe {
            let (mut frame, mut viewport) = (RECT::default(), RECT::default());
            let _ = GetWindowRect(popup.window, &mut frame);
            let _ = GetWindowRect(popup.viewport, &mut viewport);
            let chrome = frame.bottom - frame.top - popup.viewport_height;
            let height = (appearance.visible_rows(rows) as i32 * popup.cell_height
                - popup.look.gap.1)
                .min(
                    popup.work_area.bottom
                        - popup.work_area.top
                        - chrome
                        - ui::scale(20, popup.dpi),
                )
                .max(popup.cell_height - popup.look.gap.1);
            let delta = height - popup.viewport_height;
            if delta != 0 {
                let mut child = GetWindow(popup.window, GW_CHILD).ok();
                while let Some(current) = child {
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(current, &mut rect);
                    if current != popup.viewport && rect.top >= viewport.bottom {
                        let mut origin = POINT {
                            x: rect.left,
                            y: rect.top,
                        };
                        let _ = ScreenToClient(popup.window, &mut origin);
                        ui::place(
                            current,
                            origin.x,
                            origin.y + delta,
                            rect.right - rect.left,
                            rect.bottom - rect.top,
                        );
                    }
                    child = GetWindow(current, GW_HWNDNEXT).ok();
                }
                let mut top = POINT {
                    x: viewport.left,
                    y: viewport.top,
                };
                let _ = ScreenToClient(popup.window, &mut top);
                if let Some(slot) = &mut popup.title_slot {
                    if slot.top >= top.y + popup.viewport_height {
                        slot.top += delta;
                        slot.bottom += delta;
                    }
                }
                let _ = SetWindowPos(
                    popup.window,
                    None,
                    frame.left,
                    (frame.top - delta).max(popup.work_area.top),
                    frame.right - frame.left,
                    frame.bottom - frame.top + delta,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                ui::place(
                    popup.viewport,
                    top.x,
                    top.y,
                    popup.width - popup.margin * 2,
                    height,
                );
                popup.viewport_height = height;
                popup.page_rows = ((height + popup.look.gap.1) / popup.cell_height).max(1) as usize;
            }
        }
        let slot = popup.title_slot.or(popup.grown.then_some(RECT {
            left: popup.margin + gap / 2,
            top: popup.vertical_margin,
            right: popup.width - popup.margin - gap / 2,
            bottom: popup.vertical_margin + popup.band,
        }));
        if let Some(slot) = slot {
            let mut created = false;
            if depth > 0 && popup.back.is_none() {
                popup.back = Some(ui::ghost_button(
                    popup.window,
                    "Back",
                    ui::BACK_GLYPH,
                    BACK,
                    &popup.fonts[3],
                )?);
                created = true;
            }
            if depth > 0 && popup.title.is_none() {
                popup.title = Some(ui::child(
                    popup.window,
                    w!("STATIC"),
                    &name,
                    5,
                    WINDOW_STYLE(STATIC_END_ELLIPSIS | SS_CENTERIMAGE),
                    &popup.fonts[1],
                )?);
                created = true;
            }
            let size = ui::scale(30, popup.dpi);
            let mut left = slot.left;
            if let Some(back) = popup.back {
                ui::place(
                    back,
                    slot.left,
                    slot.top + (slot.bottom - slot.top - size) / 2,
                    size,
                    size,
                );
                unsafe {
                    let _ = ShowWindow(back, if depth > 0 { SW_SHOWNA } else { SW_HIDE });
                }
                if depth > 0 {
                    left += size + gap;
                }
            }
            if let Some(title) = popup.title {
                ui::place(
                    title,
                    left,
                    slot.top,
                    slot.right - left,
                    slot.bottom - slot.top,
                );
                ui::set_text(title, &name);
                unsafe {
                    let _ = ShowWindow(
                        title,
                        if depth > 0 || popup.show_title {
                            SW_SHOWNA
                        } else {
                            SW_HIDE
                        },
                    );
                }
            }
            if created {
                ui::apply_theme(popup.window, theme, &[], accent);
            }
        }
        ui::set_text(popup.window, &name);
        let rows = popup.entries.len().div_ceil(popup.columns) as i32;
        let overflow = rows * popup.cell_height - popup.look.gap.1 > popup.viewport_height;
        popup.paged = popup.pages && overflow;
        let (list_left, list_width) = list_span(
            popup.width - popup.margin * 2,
            popup.gutter,
            overflow,
            popup.frame.radius == ROUNDED_FULL,
        );
        ui::place(popup.list, list_left, 0, list_width, popup.viewport_height);
        ui::track_hover(popup.list, None);
        let corners = ui::popup_corners(popup.window, popup.frame, popup.dpi);
        let result = self.render_popup(index);
        let window = self.popups[index].window;
        if visible {
            ui::send(window, WM_SETREDRAW, WPARAM(1), LPARAM(0));
            unsafe {
                let _ = RedrawWindow(
                    Some(window),
                    None,
                    None,
                    RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN | RDW_UPDATENOW,
                );
            }
        }
        result.and(corners)
    }

    /// Turns to a page with a short slide driven by the viewport timer.
    fn go_page(&mut self, index: usize, page: i32) {
        let popup = &mut self.popups[index];
        popup.target = page.clamp(0, popup.page_count() - 1) * popup.page_height();
        if popup.target != popup.scroll {
            unsafe {
                SetTimer(Some(popup.viewport), ANIMATION, 15, None);
            }
        }
    }

    pub fn dialog_window(&self, window: HWND) -> Option<HWND> {
        let root = unsafe { GetAncestor(window, GA_ROOT) };
        if self
            .manager
            .as_ref()
            .is_some_and(|manager| manager.window == root)
            || self.popups.iter().any(|popup| popup.window == root)
        {
            Some(root)
        } else {
            None
        }
    }

    pub(super) fn hide_popups(&mut self, from: usize) {
        if from >= self.popups.len() {
            return;
        }
        for popup in self.popups.split_off(from).into_iter().rev() {
            ui::track_hover(popup.list, None);
            unsafe {
                let _ = KillTimer(Some(popup.viewport), ANIMATION);
            }
            let tooltip = ui::send(popup.list, LVM_GETTOOLTIPS, WPARAM(0), LPARAM(0));
            if tooltip.0 != 0 {
                ui::send(HWND(tooltip.0 as *mut _), TTM_POP, WPARAM(0), LPARAM(0));
            }
            unsafe {
                let _ = ShowWindow(popup.window, SW_HIDE);
            }
            self.parked.push_front(popup);
        }
        while self.parked.len() > 4 {
            self.parked.pop_back();
        }
    }
}

pub(super) fn popup_position(
    point: POINT,
    area: RECT,
    width: i32,
    height: i32,
    subgroup: bool,
    dpi: u32,
) -> (i32, i32) {
    let gap = ui::scale(8, dpi);
    let left = if subgroup {
        point.x + gap
    } else if point.x < area.left {
        area.left + gap
    } else if point.x >= area.right {
        area.right - width - gap
    } else {
        point.x - width / 2
    };
    let left = if subgroup && left + width > area.right {
        point.x - width - ui::scale(54, dpi)
    } else {
        left
    };
    let top = if subgroup {
        point.y
    } else if point.y >= area.bottom {
        area.bottom - height - gap
    } else if point.y < area.top {
        area.top + gap
    } else {
        point.y - height - gap
    };
    (
        left.clamp(area.left, (area.right - width).max(area.left)),
        top.clamp(area.top, (area.bottom - height).max(area.top)),
    )
}

impl Application {
    fn form(&mut self, title: &str, fields: &[Field]) -> Result<Option<Vec<String>>, String> {
        ui::form(self.owner(), title, fields, self.config.settings.main_theme)
    }
}

impl Drop for Application {
    fn drop(&mut self) {
        for (id, _) in &self.tray_ids {
            shell::tray(
                self.host,
                *id,
                "",
                self.brand.handle(),
                ui::TRAY,
                NIM_DELETE,
            );
        }
        self.popups.clear();
        self.manager.take();
        unsafe {
            let _ = DestroyWindow(self.host);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_selects_map_legacy_measurements_to_named_presets() {
        let ui::Field::Preset {
            values,
            measurements,
            selected,
            ..
        } = ui::Field::preset("Gap", SPACING_PRESETS, 11)
        else {
            panic!("Expected a preset select")
        };
        assert_eq!(
            (values[selected].as_str(), measurements[selected]),
            ("md", 12)
        );
        let ui::Field::Preset {
            values,
            measurements,
            selected,
            ..
        } = ui::Field::preset("Rounding", ROUNDING_PRESETS, ROUNDED_FULL)
        else {
            panic!("Expected a preset select")
        };
        assert_eq!(
            (values[selected].as_str(), measurements[selected]),
            ("full", ROUNDED_FULL)
        );
    }

    #[test]
    fn popup_buttons_align_consistently_with_one_or_two_actions() {
        for widths in [&[48][..], &[88][..], &[48, 88][..]] {
            let right = button_positions(10, 310, widths, 6, ButtonAlignment::Right);
            assert_eq!(right.last().unwrap() + widths.last().unwrap(), 310);
            let left = button_positions(10, 310, widths, 6, ButtonAlignment::Left);
            assert_eq!(left[0], 10);
            let center = button_positions(10, 310, widths, 6, ButtonAlignment::Center);
            let end = center.last().unwrap() + widths.last().unwrap();
            assert!((center[0] + end - 320).abs() <= 1);
        }
    }

    #[test]
    fn image_list_reuses_shared_icons_and_resets_keys_when_cleared() {
        let mut images = Images::new(32);
        let icon = icons::brand().unwrap();
        assert_eq!(images.shared(1, icon.handle()), 0);
        assert_eq!(images.shared(1, icon.handle()), 0);
        assert_eq!(images.shared(2, icon.handle()), 1);
        assert_eq!(unsafe { ImageList_GetImageCount(images.handle) }, 2);
        images.clear();
        assert_eq!(images.shared(2, icon.handle()), 0);
        assert_eq!(unsafe { ImageList_GetImageCount(images.handle) }, 1);
    }

    #[test]
    fn popup_placement_supports_negative_monitor_coordinates() {
        let area = RECT {
            left: -1920,
            top: -200,
            right: 0,
            bottom: 880,
        };
        let (left, top) = popup_position(POINT { x: -1800, y: 880 }, area, 400, 300, false, 144);
        assert!(left >= area.left && left + 400 <= area.right);
        assert!(top >= area.top && top + 300 <= area.bottom);
    }

    #[test]
    fn subgroup_flips_at_the_right_edge_and_never_uses_live_cursor_state() {
        let area = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let first = popup_position(POINT { x: 1900, y: 1000 }, area, 400, 300, true, 96);
        let second = popup_position(POINT { x: 1900, y: 1000 }, area, 400, 300, true, 96);
        assert_eq!(first, second);
        assert!(first.0 + 400 <= area.right && first.1 + 300 <= area.bottom);
    }
}
