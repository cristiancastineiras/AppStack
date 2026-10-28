#![allow(unsafe_op_in_unsafe_fn)]

use super::{shell, ui};
use windows::{
    Win32::{
        Foundation::*,
        System::{Com::IDataObject, Ole::*, SystemServices::MODIFIERKEYS_FLAGS},
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{Ref, implement},
};

#[implement(IDropTarget)]
struct DropTarget {
    window: usize,
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        _: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        unsafe {
            *effect = if data.as_ref().is_some_and(|data| {
                SHCreateShellItemArrayFromDataObject::<_, IShellItemArray>(data).is_ok()
            }) {
                DROPEFFECT_COPY
            } else {
                DROPEFFECT_NONE
            };
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _: MODIFIERKEYS_FLAGS,
        _: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        unsafe {
            if *effect != DROPEFFECT_NONE {
                *effect = DROPEFFECT_COPY;
            }
        }
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        _: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        unsafe {
            *effect = DROPEFFECT_NONE;
            if let Some(data) = data.as_ref() {
                if let Ok(items) = SHCreateShellItemArrayFromDataObject::<_, IShellItemArray>(data)
                {
                    let mut paths = Vec::new();
                    for index in 0..items.GetCount()?.min(10_000) {
                        let item = items.GetItemAt(index)?;
                        if let Ok(mut path) = shell::item_name(&item, SIGDN_DESKTOPABSOLUTEPARSING)
                        {
                            let prefix = "::{4234d49b-0245-4df3-b780-3893943456e1}\\";
                            if path.to_lowercase().starts_with(prefix) {
                                path = format!("shell:AppsFolder\\{}", &path[prefix.len()..]);
                            }
                            let name =
                                shell::item_name(&item, SIGDN_NORMALDISPLAY).unwrap_or_default();
                            paths.push((path, name));
                        }
                    }
                    if !paths.is_empty() {
                        let window = HWND(self.window as *mut _);
                        ui::push_text(
                            window,
                            ui::DROP,
                            serde_json::to_string(&paths).unwrap_or_default(),
                        );
                        let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
                        *effect = DROPEFFECT_COPY;
                    }
                }
            }
        }
        Ok(())
    }
}

pub fn register(window: HWND) -> Result<IDropTarget, String> {
    let target: IDropTarget = DropTarget {
        window: window.0 as usize,
    }
    .into();
    unsafe {
        RegisterDragDrop(window, &target).map_err(shell::failure)?;
    }
    Ok(target)
}
