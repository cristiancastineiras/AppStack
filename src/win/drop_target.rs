#![allow(unsafe_op_in_unsafe_fn)]

use super::{shell, ui};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use windows::{
    Win32::{
        Foundation::*,
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IDataObject},
            Ole::*,
            SystemServices::{MK_CONTROL, MODIFIERKEYS_FLAGS},
        },
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{Ref, implement},
};

/// What a drop delivers to the window it landed on.
#[derive(Serialize, Deserialize)]
pub struct Dropped {
    /// Parsing path and display name of each item.
    pub items: Vec<(String, String)>,
    /// Shortcut files leave their folder for the group, as a move between Explorer folders does.
    pub moving: bool,
    /// Screen position of the drop.
    pub point: (i32, i32),
}

thread_local! {
    // The folder window a drag is over right now; its drop is still to come.
    static OVER: Cell<usize> = const { Cell::new(0) };
}

/// Whether a drag is over `window`, so a popup must stay open for its drop.
pub fn dragging_over(window: HWND) -> bool {
    OVER.with(Cell::get) == window.0 as usize
}

/// The screen point a `ui::DRAG` event carries in its second value.
pub fn unpack(value: isize) -> POINT {
    POINT {
        x: value as i32,
        y: (value >> 32) as i32,
    }
}

#[implement(IDropTarget)]
struct DropTarget {
    window: usize,
    /// Popups take items like a folder: shortcuts move into them unless Ctrl copies them.
    folder: bool,
    /// Draws Explorer's drag image over the window, as Explorer's own folders do.
    helper: Option<IDropTargetHelper>,
    accepted: Cell<bool>,
    /// Every dragged item is a shortcut file, which can move without breaking anything.
    movable: Cell<bool>,
}

impl DropTarget {
    fn effect(&self, keys: MODIFIERKEYS_FLAGS, allowed: DROPEFFECT) -> DROPEFFECT {
        if !self.accepted.get() {
            return DROPEFFECT_NONE;
        }
        if !self.folder {
            return DROPEFFECT_COPY;
        }
        let has = |effect: DROPEFFECT| allowed.0 & effect.0 != 0;
        let copy = keys.0 & MK_CONTROL.0 != 0;
        if self.movable.get() && !copy && has(DROPEFFECT_MOVE) {
            DROPEFFECT_MOVE
        } else if (copy || !has(DROPEFFECT_LINK)) && has(DROPEFFECT_COPY) {
            DROPEFFECT_COPY
        } else if has(DROPEFFECT_LINK) {
            DROPEFFECT_LINK
        } else {
            DROPEFFECT_COPY
        }
    }

    /// Tells the popup where the drag is, so a subgroup under it can light up; None when it left.
    fn hover(&self, point: Option<&POINTL>) {
        if !self.folder {
            return;
        }
        let window = HWND(self.window as *mut _);
        OVER.with(|over| over.set(if point.is_some() { self.window } else { 0 }));
        match point {
            Some(point) => ui::push(
                window,
                ui::DRAG,
                1,
                ((point.y as isize) << 32) | point.x as u32 as isize,
            ),
            None => ui::push(window, ui::DRAG, 0, 0),
        }
    }
}

/// The parsing path and display name of each dragged Shell item.
unsafe fn items(data: &IDataObject) -> Option<Vec<(String, String)>> {
    let items: IShellItemArray = SHCreateShellItemArrayFromDataObject(data).ok()?;
    let mut paths = Vec::new();
    for index in 0..items.GetCount().ok()?.min(10_000) {
        let item = items.GetItemAt(index).ok()?;
        if let Ok(mut path) = shell::item_name(&item, SIGDN_DESKTOPABSOLUTEPARSING) {
            let prefix = "::{4234d49b-0245-4df3-b780-3893943456e1}\\";
            if path.to_lowercase().starts_with(prefix) {
                path = format!("shell:AppsFolder\\{}", &path[prefix.len()..]);
            }
            let name = shell::item_name(&item, SIGDN_NORMALDISPLAY).unwrap_or_default();
            paths.push((path, name));
        }
    }
    Some(paths)
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        unsafe {
            let items = data
                .as_ref()
                .and_then(|data| items(data))
                .unwrap_or_default();
            self.accepted.set(!items.is_empty());
            self.movable.set(
                self.folder
                    && !items.is_empty()
                    && items.iter().all(|(path, _)| shell::is_shortcut(path)),
            );
            *effect = self.effect(keys, *effect);
            if self.accepted.get() {
                self.hover(Some(point));
            }
            if let (Some(helper), Some(data)) = (&self.helper, data.as_ref()) {
                let _ = helper.DragEnter(
                    HWND(self.window as *mut _),
                    data,
                    &POINT {
                        x: point.x,
                        y: point.y,
                    },
                    *effect,
                );
            }
        }
        Ok(())
    }

    fn DragOver(
        &self,
        keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        unsafe {
            *effect = self.effect(keys, *effect);
            if self.accepted.get() {
                self.hover(Some(point));
            }
            if let Some(helper) = &self.helper {
                let _ = helper.DragOver(
                    &POINT {
                        x: point.x,
                        y: point.y,
                    },
                    *effect,
                );
            }
        }
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        if self.accepted.get() {
            self.hover(None);
        }
        if let Some(helper) = &self.helper {
            unsafe {
                let _ = helper.DragLeave();
            }
        }
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        unsafe {
            let chosen = self.effect(keys, *effect);
            *effect = DROPEFFECT_NONE;
            if self.accepted.get() {
                self.hover(None);
            }
            if let (Some(helper), Some(data)) = (&self.helper, data.as_ref()) {
                let _ = helper.Drop(
                    data,
                    &POINT {
                        x: point.x,
                        y: point.y,
                    },
                    chosen,
                );
            }
            if chosen == DROPEFFECT_NONE {
                return Ok(());
            }
            let items = data
                .as_ref()
                .and_then(|data| items(data))
                .unwrap_or_default();
            if !items.is_empty() {
                let window = HWND(self.window as *mut _);
                let dropped = Dropped {
                    items,
                    moving: chosen == DROPEFFECT_MOVE,
                    point: (point.x, point.y),
                };
                ui::push_text(
                    window,
                    ui::DROP,
                    serde_json::to_string(&dropped).unwrap_or_default(),
                );
                let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
                // AppStack moves the shortcuts itself once the drag is over, so the source must
                // never delete them as it would after a plain move.
                *effect = if chosen == DROPEFFECT_MOVE {
                    DROPEFFECT_COPY
                } else {
                    chosen
                };
            }
        }
        Ok(())
    }
}

/// Accepts Shell items dropped on `window`; a `folder` window moves shortcuts into itself.
pub fn register(window: HWND, folder: bool) -> Result<IDropTarget, String> {
    let helper =
        unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER) }.ok();
    let target: IDropTarget = DropTarget {
        window: window.0 as usize,
        folder,
        helper,
        accepted: Cell::new(false),
        movable: Cell::new(false),
    }
    .into();
    unsafe {
        RegisterDragDrop(window, &target).map_err(shell::failure)?;
    }
    Ok(target)
}
