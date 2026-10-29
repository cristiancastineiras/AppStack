use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortMode {
    #[default]
    Manual,
    Alphabetical,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LabelPosition {
    #[default]
    Bottom,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ButtonPosition {
    Top,
    #[default]
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ButtonAlignment {
    Left,
    Center,
    #[default]
    Right,
}

pub const ROUNDED_FULL: u32 = 9999;
pub const SPACING_PRESETS: &[(&str, u32)] = &[
    ("none", 0),
    ("xs", 4),
    ("sm", 8),
    ("md", 12),
    ("lg", 16),
    ("xl", 24),
    ("2xl", 32),
    ("3xl", 48),
];
pub const ICON_PRESETS: &[(&str, u32)] = &[
    ("xs", 16),
    ("sm", 20),
    ("md", 24),
    ("lg", 32),
    ("xl", 48),
    ("2xl", 64),
    ("3xl", 96),
    ("4xl", 128),
];
pub const ROUNDING_PRESETS: &[(&str, u32)] = &[
    ("none", 0),
    ("xs", 2),
    ("sm", 4),
    ("md", 6),
    ("lg", 8),
    ("xl", 12),
    ("2xl", 16),
    ("3xl", 24),
    ("4xl", 32),
    ("full", ROUNDED_FULL),
];
pub const LABEL_PRESETS: &[(&str, u32)] = &[
    ("xs", 12),
    ("sm", 14),
    ("base", 16),
    ("lg", 18),
    ("xl", 20),
    ("2xl", 24),
];
pub const WIDTH_PRESETS: &[(&str, u32)] = &[
    ("auto", 0),
    ("xs", 320),
    ("sm", 384),
    ("md", 448),
    ("lg", 512),
    ("xl", 576),
    ("2xl", 672),
    ("3xl", 768),
];
/// Tailwind's `border-0`, `border`, `border-2`, `border-4` and `border-8`.
pub const BORDER_PRESETS: &[(&str, u32)] = &[("0", 0), ("1", 1), ("2", 2), ("4", 4), ("8", 8)];
/// Tailwind's shadow scale; the value is the level the popup renderer looks up.
pub const SHADOW_PRESETS: &[(&str, u32)] = &[
    ("none", 0),
    ("sm", 1),
    ("md", 2),
    ("lg", 3),
    ("xl", 4),
    ("2xl", 5),
];

/// Embedded in both executables, so they work without being installed.
pub const TITLE_FONT: &str = "Space Grotesk";
pub const LABEL_FONT: &str = "Inter";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub main_theme: Theme,
    pub popup_theme: Theme,
    pub group_sort: SortMode,
    pub start_with_windows: bool,
    pub show_tray: bool,
    pub launch_all_enabled: bool,
    pub grayscale_shortcuts: bool,
    pub close_after_launch: bool,
    pub animations: bool,
    pub pointer_cursor: bool,
    pub check_updates: bool,
    pub title_font: String,
    pub label_font: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            main_theme: Theme::System,
            popup_theme: Theme::System,
            group_sort: SortMode::Manual,
            start_with_windows: false,
            show_tray: true,
            launch_all_enabled: true,
            grayscale_shortcuts: false,
            close_after_launch: true,
            animations: true,
            pointer_cursor: true,
            check_updates: true,
            title_font: TITLE_FONT.into(),
            label_font: LABEL_FONT.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub icon: String,
    pub grid_icon: bool,
    /// With `grid_icon`, alternates large and small tiles; a separate flag so older versions
    /// still read the configuration and show a plain grid.
    pub mosaic_icon: bool,
    pub columns: usize,
    pub show_labels: bool,
    pub label_position: LabelPosition,
    pub label_size: u32,
    pub show_header: bool,
    pub header_bottom: bool,
    pub card_layout: bool,
    pub accent: String,
    pub sort: SortMode,
    pub show_on_tray: bool,
    pub show_edit_button: bool,
    pub show_launch_all_button: bool,
    pub button_position: ButtonPosition,
    pub scroll: bool,
    pub visible_rows: u32,
    pub padding_horizontal: u32,
    pub padding_vertical: u32,
    pub gap_horizontal: u32,
    pub gap_vertical: u32,
    pub icon_size: u32,
    pub corner_radius: u32,
    pub tile_radius: u32,
    pub button_alignment: ButtonAlignment,
    pub minimum_width: u32,
    pub border_width: u32,
    /// Empty follows the theme.
    pub border_color: String,
    pub shadow: u32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            icon: String::new(),
            grid_icon: true,
            mosaic_icon: false,
            columns: 4,
            show_labels: true,
            label_position: LabelPosition::Bottom,
            label_size: 12,
            show_header: true,
            header_bottom: false,
            card_layout: false,
            accent: String::new(),
            sort: SortMode::Manual,
            show_on_tray: false,
            show_edit_button: true,
            show_launch_all_button: true,
            button_position: ButtonPosition::Bottom,
            scroll: false,
            visible_rows: 3,
            padding_horizontal: 10,
            padding_vertical: 10,
            gap_horizontal: 0,
            gap_vertical: 0,
            icon_size: 32,
            corner_radius: 8,
            tile_radius: 4,
            button_alignment: ButtonAlignment::Right,
            minimum_width: 0,
            border_width: 1,
            border_color: String::new(),
            shadow: 2,
        }
    }
}

impl Appearance {
    /// Extra (horizontal, vertical) padding for a `full` pill, like CSS `rounded-full`: the
    /// straight sides need none, and the long axis grows only until the content corners sit
    /// inside the round ends, the regular padding away from the arc.
    pub fn rounding_inset(&self, content_width: i32, content_height: i32) -> (i32, i32) {
        if self.corner_radius != ROUNDED_FULL {
            return (0, 0);
        }
        let width = content_width.max(0) + self.padding_horizontal as i32 * 2;
        let height = content_height.max(0) + self.padding_vertical as i32 * 2;
        let (short, content) = if width >= height {
            (height, content_height.max(0))
        } else {
            (width, content_width.max(0))
        };
        let radius = short as f64 / 2.0;
        let half = content as f64 / 2.0;
        // Where the arc crosses the content corner's line, measured from the pill's end.
        let inset = (radius - (radius * radius - half * half).max(0.0).sqrt()).ceil() as i32;
        if width >= height {
            (inset, 0)
        } else {
            (0, inset)
        }
    }

    pub fn cell_size(&self) -> (i32, i32) {
        let icon_delta = self.icon_size as i32 - 32;
        let width = if self.show_labels && self.label_position == LabelPosition::Right {
            200
        } else if self.show_labels {
            92
        } else {
            56
        };
        let height = if self.show_labels && self.label_position == LabelPosition::Bottom {
            86
        } else {
            52
        };
        (
            width + icon_delta + self.gap_horizontal as i32,
            height + icon_delta + (self.label_size as i32 - 12).max(0) + self.gap_vertical as i32,
        )
    }

    /// Rows the popup shows before scrolling; the caller still caps it to the screen.
    pub fn visible_rows(&self, rows: usize) -> usize {
        if self.scroll {
            rows.min(self.visible_rows as usize).max(1)
        } else {
            rows.max(1)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum Target {
    Application(String),
    Group(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub target: Target,
    pub arguments: String,
    pub icon: String,
    pub tooltip: String,
    pub administrator: bool,
}

impl Default for Entry {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: String::new(),
            target: Target::Application(String::new()),
            arguments: String::new(),
            icon: String::new(),
            tooltip: String::new(),
            administrator: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub appearance: Appearance,
    pub entries: Vec<Entry>,
}

impl Default for Group {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: "New group".into(),
            appearance: Appearance::default(),
            entries: Vec::new(),
        }
    }
}

impl Group {
    pub fn ordered_entries(&self) -> Vec<&Entry> {
        let mut entries: Vec<_> = self.entries.iter().collect();
        if self.appearance.sort == SortMode::Alphabetical {
            entries.sort_by_cached_key(|entry| entry.name.to_lowercase());
        }
        entries
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub schema_version: u32,
    pub groups: Vec<Group>,
    pub settings: Settings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 2,
            groups: Vec::new(),
            settings: Settings::default(),
        }
    }
}

impl Config {
    pub fn group(&self, id: &str) -> Option<&Group> {
        self.groups.iter().find(|group| group.id == id)
    }

    pub fn ordered_groups(&self) -> Vec<&Group> {
        let mut groups: Vec<_> = self.groups.iter().collect();
        if self.settings.group_sort == SortMode::Alphabetical {
            groups.sort_by_cached_key(|group| group.name.to_lowercase());
        }
        groups
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 2 {
            return Err(format!(
                "Unsupported configuration version: {}",
                self.schema_version
            ));
        }
        if self.groups.len() > 10_000 {
            return Err("A configuration can contain at most 10,000 groups".into());
        }
        for font in [&self.settings.title_font, &self.settings.label_font] {
            // Windows font face names hold at most 31 UTF-16 units.
            if font.contains('\0') || font.encode_utf16().count() > 31 {
                return Err(format!("Invalid font name: {font}"));
            }
        }
        let by_id: HashMap<_, _> = self
            .groups
            .iter()
            .map(|group| (group.id.as_str(), group))
            .collect();
        let mut ids = HashSet::new();
        let mut entry_ids = HashSet::new();
        for group in &self.groups {
            if uuid::Uuid::parse_str(&group.id).is_err() || !ids.insert(group.id.as_str()) {
                return Err("Group identifiers must be unique and nonempty".into());
            }
            if group.name.trim().is_empty() || group.name.contains('\0') {
                return Err("A group needs a name".into());
            }
            if group.entries.len() > 10_000 {
                return Err("A group can contain at most 10,000 applications".into());
            }
            if !(1..=16).contains(&group.appearance.columns) {
                return Err("Columns must be between 1 and 16".into());
            }
            if !(8..=24).contains(&group.appearance.label_size) {
                return Err("Label size must be between 8 and 24".into());
            }
            if !(1..=50).contains(&group.appearance.visible_rows) {
                return Err("Visible rows must be between 1 and 50".into());
            }
            let appearance = &group.appearance;
            if appearance.minimum_width > 2000 {
                return Err("Minimum popup width must be between 0 and 2000".into());
            }
            if !(16..=128).contains(&appearance.icon_size) {
                return Err("Icon size must be between 16 and 128".into());
            }
            if appearance.padding_horizontal > 100 || appearance.padding_vertical > 100 {
                return Err("Popup padding must be between 0 and 100".into());
            }
            if appearance.gap_horizontal > 100 || appearance.gap_vertical > 100 {
                return Err("Application spacing must be between 0 and 100".into());
            }
            if (appearance.corner_radius > 48 && appearance.corner_radius != ROUNDED_FULL)
                || (appearance.tile_radius > 32 && appearance.tile_radius != ROUNDED_FULL)
            {
                return Err("Popup corners must be 0-48 and tile corners 0-32".into());
            }
            if appearance.border_width > 8 || appearance.shadow > 5 {
                return Err("Border width must be 0-8 and shadow 0-5".into());
            }
            for (name, color) in [
                ("Accent", &appearance.accent),
                ("Border color", &appearance.border_color),
            ] {
                if !color.is_empty() && parse_color(color).is_none() {
                    return Err(format!(
                        "{name} must be a hexadecimal color, for example #16856B"
                    ));
                }
            }
            for entry in &group.entries {
                if uuid::Uuid::parse_str(&entry.id).is_err() || !entry_ids.insert(entry.id.as_str())
                {
                    return Err("Application identifiers must be unique and nonempty".into());
                }
                if entry.name.trim().is_empty()
                    || entry.name.contains('\0')
                    || entry.arguments.contains('\0')
                    || entry.icon.contains('\0')
                {
                    return Err("An application needs a name".into());
                }
                match &entry.target {
                    Target::Application(path) if path.trim().is_empty() || path.contains('\0') => {
                        return Err("An application needs a path or Shell identifier".into());
                    }
                    Target::Group(id) if !by_id.contains_key(id.as_str()) => {
                        return Err(format!("Missing subgroup: {}", entry.name));
                    }
                    _ => {}
                }
            }
        }
        let mut completed = HashSet::new();
        for group in &self.groups {
            let mut active = HashSet::new();
            let mut stack = vec![(group.id.as_str(), false)];
            while let Some((id, leaving)) = stack.pop() {
                if leaving {
                    active.remove(id);
                    completed.insert(id);
                    continue;
                }
                if active.contains(id) {
                    return Err("Groups cannot contain circular subgroup references".into());
                }
                if completed.contains(id) {
                    continue;
                }
                active.insert(id);
                stack.push((id, true));
                if let Some(current) = by_id.get(id) {
                    for entry in &current.entries {
                        if let Target::Group(child) = &entry.target {
                            stack.push((child.as_str(), false));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// A group followed by every subgroup it can open, so a popup can size itself once for
    /// all the folders it shows in place.
    pub fn folders(&self, id: &str) -> Vec<&Group> {
        let mut seen = HashSet::from([id]);
        let mut stack = vec![id];
        let mut folders = Vec::new();
        while let Some(id) = stack.pop() {
            let Some(group) = self.group(id) else {
                continue;
            };
            folders.push(group);
            for entry in group.entries.iter().rev() {
                if let Target::Group(child) = &entry.target {
                    if seen.insert(child.as_str()) {
                        stack.push(child.as_str());
                    }
                }
            }
        }
        folders
    }

    pub fn launch_plan(&self, id: &str) -> Result<Vec<Entry>, String> {
        self.validate()?;
        let root = self.group(id).ok_or("Group not found")?;
        let mut result = Vec::new();
        let mut visited = HashSet::from([id]);
        let mut applications = HashSet::new();
        let mut stack: Vec<_> = root.ordered_entries().into_iter().rev().collect();
        while let Some(entry) = stack.pop() {
            match &entry.target {
                Target::Group(child) => {
                    if visited.insert(child.as_str()) {
                        let group = self.group(child).ok_or("Group not found")?;
                        stack.extend(group.ordered_entries().into_iter().rev());
                    }
                }
                Target::Application(path) => {
                    let key = (path.clone(), entry.arguments.clone(), entry.administrator);
                    if applications.insert(key) {
                        result.push(entry.clone());
                    }
                }
            }
        }
        Ok(result)
    }

    pub fn remove_group(&mut self, id: &str) {
        self.groups.retain(|group| group.id != id);
        for group in &mut self.groups {
            group
                .entries
                .retain(|entry| entry.target != Target::Group(id.to_string()));
        }
    }

    pub fn append_import(&mut self, mut imported: Vec<Group>) -> Result<usize, String> {
        let remap: HashMap<_, _> = imported
            .iter()
            .map(|group| (group.id.clone(), new_id()))
            .collect();
        let mut candidate = self.clone();
        for group in &mut imported {
            group.id = remap[&group.id].clone();
            for entry in &mut group.entries {
                entry.id = new_id();
                if let Target::Group(id) = &mut entry.target {
                    *id = remap
                        .get(id)
                        .ok_or("Import includes a missing subgroup")?
                        .clone();
                }
            }
        }
        let count = imported.len();
        candidate.groups.extend(imported);
        candidate.validate()?;
        *self = candidate;
        Ok(count)
    }
}

pub fn parse_color(value: &str) -> Option<u32> {
    let value = value.strip_prefix('#').unwrap_or(value);
    (value.len() == 6)
        .then(|| u32::from_str_radix(value, 16).ok())
        .flatten()
}

pub fn move_item<T>(items: &mut Vec<T>, from: usize, to: usize) {
    if from < items.len() && to < items.len() && from != to {
        let item = items.remove(from);
        items.insert(to, item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_rounding_is_valid_and_reserves_safe_inner_space() {
        let appearance = Appearance {
            corner_radius: ROUNDED_FULL,
            tile_radius: ROUNDED_FULL,
            ..Appearance::default()
        };
        let config = Config {
            groups: vec![Group {
                appearance: appearance.clone(),
                ..Group::default()
            }],
            ..Config::default()
        };
        config.validate().unwrap();
        let padding = appearance.padding_horizontal as i32;
        // Wide pills only pad their round ends; tall ones only their top and bottom.
        let (inset, vertical) = appearance.rounding_inset(320, 160);
        assert_eq!(vertical, 0);
        let radius = (160 + padding * 2) as f64 / 2.0;
        let from_center = radius - (padding + inset) as f64;
        assert!(from_center.hypot(80.0) <= radius);
        assert!(
            inset < radius as i32,
            "the inset should stay tight: {inset}"
        );
        let (horizontal, inset) = appearance.rounding_inset(60, 400);
        assert_eq!(horizontal, 0);
        let radius = (60 + padding * 2) as f64 / 2.0;
        assert!((radius - (padding + inset) as f64).hypot(30.0) <= radius);
        assert_eq!(Appearance::default().rounding_inset(320, 160), (0, 0));
        assert_eq!(ROUNDING_PRESETS.last(), Some(&("full", ROUNDED_FULL)));
    }

    #[test]
    fn custom_cells_keep_icons_inside_the_tile_and_gaps_outside() {
        assert_eq!(Appearance::default().cell_size(), (92, 86));
        for icon_size in [16, 32, 64, 128] {
            for label_position in [LabelPosition::Bottom, LabelPosition::Right] {
                for show_labels in [false, true] {
                    let look = Appearance {
                        icon_size,
                        label_position,
                        show_labels,
                        gap_horizontal: 25,
                        gap_vertical: 30,
                        ..Appearance::default()
                    };
                    let (width, height) = look.cell_size();
                    assert!(width - 25 > icon_size as i32);
                    assert!(height - 30 > icon_size as i32);
                }
            }
        }
    }

    #[test]
    fn custom_popup_settings_round_trip_and_validate() {
        let appearance = Appearance {
            minimum_width: 720,
            padding_horizontal: 24,
            padding_vertical: 18,
            gap_horizontal: 12,
            gap_vertical: 16,
            icon_size: 64,
            corner_radius: 24,
            tile_radius: 12,
            button_alignment: ButtonAlignment::Center,
            ..Appearance::default()
        };
        let mut config = Config {
            groups: vec![Group {
                appearance,
                ..Group::default()
            }],
            ..Config::default()
        };
        config.validate().unwrap();
        let restored: Config =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        let look = &restored.groups[0].appearance;
        assert_eq!(look.minimum_width, 720);
        assert_eq!(
            (
                look.padding_horizontal,
                look.padding_vertical,
                look.gap_horizontal,
                look.gap_vertical,
                look.icon_size,
                look.corner_radius,
                look.tile_radius,
                look.button_alignment
            ),
            (24, 18, 12, 16, 64, 24, 12, ButtonAlignment::Center)
        );
        for invalid in [
            Appearance {
                minimum_width: 2001,
                ..Appearance::default()
            },
            Appearance {
                icon_size: 15,
                ..Appearance::default()
            },
            Appearance {
                icon_size: 129,
                ..Appearance::default()
            },
            Appearance {
                padding_horizontal: 101,
                ..Appearance::default()
            },
            Appearance {
                padding_vertical: 101,
                ..Appearance::default()
            },
            Appearance {
                gap_horizontal: 101,
                ..Appearance::default()
            },
            Appearance {
                gap_vertical: 101,
                ..Appearance::default()
            },
            Appearance {
                corner_radius: 49,
                ..Appearance::default()
            },
            Appearance {
                tile_radius: 33,
                ..Appearance::default()
            },
            Appearance {
                border_width: 9,
                ..Appearance::default()
            },
            Appearance {
                border_color: "blue".into(),
                ..Appearance::default()
            },
            Appearance {
                shadow: 6,
                ..Appearance::default()
            },
        ] {
            config.groups[0].appearance = invalid;
            assert!(config.validate().is_err());
        }
    }

    fn application(name: &str) -> Entry {
        Entry {
            name: name.into(),
            target: Target::Application(format!("{name}.exe")),
            ..Entry::default()
        }
    }

    #[test]
    fn configuration_round_trips_without_changing_identifiers() {
        let mut config = Config::default();
        config.groups.push(Group {
            entries: vec![application("Editor")],
            ..Group::default()
        });
        let json = serde_json::to_string(&config).unwrap();
        let restored: Config = serde_json::from_str(&json).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.groups[0].id, config.groups[0].id);
        assert_eq!(
            restored.groups[0].entries[0].id,
            config.groups[0].entries[0].id
        );
    }

    #[test]
    fn cycles_are_rejected_and_shared_subgroups_are_allowed() {
        let mut root = Group::default();
        let mut child = Group::default();
        root.entries.push(Entry {
            name: "Child".into(),
            target: Target::Group(child.id.clone()),
            ..Entry::default()
        });
        root.entries.push(root.entries[0].clone());
        root.entries[1].id = new_id();
        let mut config = Config {
            groups: vec![root.clone(), child.clone()],
            ..Config::default()
        };
        config.validate().unwrap();
        child.entries.push(Entry {
            name: "Root".into(),
            target: Target::Group(root.id.clone()),
            ..Entry::default()
        });
        config.groups[1] = child;
        assert!(config.validate().unwrap_err().contains("circular"));
    }

    #[test]
    fn launch_all_deduplicates_apps_but_preserves_distinct_arguments() {
        let first = application("Editor");
        let mut second = first.clone();
        second.id = new_id();
        let mut third = first.clone();
        third.id = new_id();
        third.arguments = "--new-window".into();
        let group = Group {
            entries: vec![first, second, third],
            ..Group::default()
        };
        let id = group.id.clone();
        let config = Config {
            groups: vec![group],
            ..Config::default()
        };
        assert_eq!(config.launch_plan(&id).unwrap().len(), 2);
    }

    #[test]
    fn imports_remap_identifiers_and_never_overwrite_existing_groups() {
        let child = Group::default();
        let parent = Group {
            entries: vec![Entry {
                name: "Child".into(),
                target: Target::Group(child.id.clone()),
                ..Entry::default()
            }],
            ..Group::default()
        };
        let mut config = Config {
            groups: vec![child.clone()],
            ..Config::default()
        };
        config.append_import(vec![parent, child.clone()]).unwrap();
        assert_eq!(config.groups.len(), 3);
        assert_eq!(config.groups[0].id, child.id);
        assert_ne!(config.groups[2].id, child.id);
        assert_eq!(
            config.groups[1].entries[0].target,
            Target::Group(config.groups[2].id.clone())
        );
    }

    #[test]
    fn moving_and_sorting_are_deterministic() {
        let mut group = Group {
            entries: vec![application("Zulu"), application("Alpha")],
            ..Group::default()
        };
        move_item(&mut group.entries, 0, 1);
        assert_eq!(group.entries[0].name, "Alpha");
        group.appearance.sort = SortMode::Alphabetical;
        assert_eq!(group.ordered_entries()[0].name, "Alpha");
        assert_eq!(parse_color("#16856B"), Some(0x16856B));
        assert_eq!(parse_color("bad"), None);
    }

    #[test]
    fn launch_order_follows_subgroups_at_their_visible_position() {
        let child = Group {
            entries: vec![application("Second")],
            ..Group::default()
        };
        let parent = Group {
            entries: vec![
                application("First"),
                Entry {
                    name: "Tools".into(),
                    target: Target::Group(child.id.clone()),
                    ..Entry::default()
                },
                application("Third"),
            ],
            ..Group::default()
        };
        let id = parent.id.clone();
        let config = Config {
            groups: vec![parent, child],
            ..Config::default()
        };
        let names: Vec<_> = config
            .launch_plan(&id)
            .unwrap()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, ["First", "Second", "Third"]);
    }

    #[test]
    fn older_configurations_receive_new_presentation_defaults() {
        let config: Config =
            serde_json::from_str(r#"{"schema_version":2,"groups":[{"id":"6f1c1fd8-0c8e-4a8e-9c38-5ee1d1b7a0e1","name":"Old","appearance":{"columns":3}}]}"#)
                .unwrap();
        config.validate().unwrap();
        let appearance = &config.groups[0].appearance;
        assert!(appearance.show_edit_button && appearance.show_launch_all_button);
        assert_eq!(appearance.button_position, ButtonPosition::Bottom);
        assert!(!appearance.scroll);
        assert_eq!(config.settings.title_font, TITLE_FONT);
        assert_eq!(config.settings.label_font, LABEL_FONT);
    }

    #[test]
    fn scrolling_limits_visible_rows_and_invalid_values_are_rejected() {
        let mut appearance = Appearance::default();
        assert_eq!(appearance.visible_rows(10), 10);
        assert_eq!(appearance.visible_rows(0), 1);
        appearance.scroll = true;
        assert_eq!(appearance.visible_rows(10), 3);
        assert_eq!(appearance.visible_rows(2), 2);
        let mut config = Config {
            groups: vec![Group::default()],
            ..Config::default()
        };
        config.groups[0].appearance.visible_rows = 0;
        assert!(config.validate().is_err());
        config.groups[0].appearance.visible_rows = 3;
        config.settings.title_font = "x".repeat(32);
        assert!(config.validate().is_err());
        config.settings.title_font = "Segoe UI".into();
        config.validate().unwrap();
    }

    #[test]
    fn folders_cover_nested_subgroups_once_root_first() {
        let leaf = Group {
            entries: (0..7)
                .map(|index| application(&index.to_string()))
                .collect(),
            ..Group::default()
        };
        let middle = Group {
            entries: vec![Entry {
                name: "Leaf".into(),
                target: Target::Group(leaf.id.clone()),
                ..Entry::default()
            }],
            ..Group::default()
        };
        let root = Group {
            entries: vec![
                application("Editor"),
                Entry {
                    name: "Middle".into(),
                    target: Target::Group(middle.id.clone()),
                    ..Entry::default()
                },
            ],
            ..Group::default()
        };
        let id = root.id.clone();
        let config = Config {
            groups: vec![root, middle, leaf],
            ..Config::default()
        };
        let folders = config.folders(&id);
        assert_eq!(folders.len(), 3);
        assert_eq!(folders[0].id, id);
        assert_eq!(
            folders.iter().map(|folder| folder.entries.len()).max(),
            Some(7)
        );
        assert!(config.folders("missing").is_empty());
    }

    #[test]
    fn unsafe_identifiers_and_nul_characters_are_rejected() {
        let group = Group {
            id: "../outside".into(),
            ..Group::default()
        };
        assert!(
            Config {
                groups: vec![group],
                ..Config::default()
            }
            .validate()
            .is_err()
        );
        let group = Group {
            entries: vec![Entry {
                name: "App".into(),
                target: Target::Application("safe.exe\0bad".into()),
                ..Entry::default()
            }],
            ..Group::default()
        };
        assert!(
            Config {
                groups: vec![group],
                ..Config::default()
            }
            .validate()
            .is_err()
        );
    }
}
