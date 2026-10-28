#![allow(unsafe_op_in_unsafe_fn)]

use super::shell::{failure, wide};
use appstack::model::{
    Appearance, ButtonPosition, Group, LabelPosition, ROUNDED_FULL, Settings, TITLE_FONT, Theme,
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    mem::size_of,
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::{DataExchange::COPYDATASTRUCT, LibraryLoader::*, Registry::*},
        UI::{
            Accessibility::*,
            Controls::{
                Dialogs::{CC_ANYCOLOR, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW},
                *,
            },
            HiDpi::*,
            Input::KeyboardAndMouse::*,
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
    core::{BOOL, PCSTR, PCWSTR, PWSTR, w},
};

pub const SELECT: u32 = WM_APP + 10;
pub const ACTIVATE_ITEM: u32 = WM_APP + 11;
pub const BEGIN_DRAG: u32 = WM_APP + 12;
pub const ASYNC: u32 = WM_APP + 13;
pub const TRAY: u32 = WM_APP + 14;
pub const DROP: u32 = WM_APP + 15;
pub const INVOKE_ITEM: u32 = WM_APP + 16;

#[derive(Debug)]
pub struct Event {
    pub window: HWND,
    pub code: u32,
    pub first: usize,
    pub second: isize,
    pub text: String,
}

thread_local! {
    static EVENTS: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
    static SUPPRESSED: Cell<bool> = const { Cell::new(false) };
    static COLORS: RefCell<HashMap<usize, (u32, u32)>> = RefCell::new(HashMap::new());
    static ITEM_TIPS: RefCell<HashMap<usize, Vec<Vec<u16>>>> = RefCell::new(HashMap::new());
    static LOOKS: RefCell<HashMap<usize, Look>> = RefCell::new(HashMap::new());
    static PAGERS: RefCell<HashMap<usize, Pager>> = RefCell::new(HashMap::new());
    // Edge windows and the popup frame each one draws.
    static EDGES: RefCell<HashMap<usize, (Frame, u8)>> = RefCell::new(HashMap::new());
    // Opening popups: start, resting position and icon list.
    static POPS: RefCell<HashMap<usize, (std::time::Instant, POINT, HWND)>> = RefCell::new(HashMap::new());
    // Hand cursor over clickable popup items, like CSS `cursor: pointer`.
    static POINTER: Cell<bool> = const { Cell::new(false) };
    // Ghost buttons: hovered flag and the icon-font glyph each one draws.
    static GHOSTS: RefCell<HashMap<usize, (bool, u16)>> = RefCell::new(HashMap::new());
    // Icon lists act on the first click; this remembers it so the double-click is not acted on again.
    static LAST_CLICK: Cell<Option<(usize, std::time::Instant)>> = const { Cell::new(None) };
    // Closed-hand drag cursor and the cursor size it was built for.
    static GRAB: Cell<Option<(i32, HCURSOR)>> = const { Cell::new(None) };
    static CONTRAST: Cell<bool> = const { Cell::new(false) };
    static ACCENT: Cell<u32> = const { Cell::new(0x00D77800) };
    // The color picker's custom swatches, kept for the session.
    static CUSTOM_COLORS: Cell<[COLORREF; 16]> = const { Cell::new([COLORREF(0xFFFFFF); 16]) };
}

pub fn push(window: HWND, code: u32, first: usize, second: isize) {
    EVENTS.with_borrow_mut(|events| {
        events.push_back(Event {
            window,
            code,
            first,
            second,
            text: String::new(),
        })
    });
}

pub fn pop() -> Option<Event> {
    EVENTS.with_borrow_mut(VecDeque::pop_front)
}

pub fn push_text(window: HWND, code: u32, text: String) {
    EVENTS.with_borrow_mut(|events| {
        events.push_back(Event {
            window,
            code,
            first: 0,
            second: 0,
            text,
        })
    });
}

pub fn suppress(action: impl FnOnce()) {
    SUPPRESSED.with(|suppressed| {
        let previous = suppressed.replace(true);
        action();
        suppressed.set(previous);
    });
}

pub unsafe extern "system" fn procedure(
    window: HWND,
    code: u32,
    first: WPARAM,
    second: LPARAM,
) -> LRESULT {
    match code {
        WM_TIMER if first.0 == POP_TIMER => {
            step_pop(window);
            return LRESULT(0);
        }
        WM_CLOSE | WM_SIZE | WM_ACTIVATE | WM_DPICHANGED | WM_SETTINGCHANGE | WM_DISPLAYCHANGE
        | WM_CONTEXTMENU | WM_LBUTTONUP | WM_MOUSEMOVE | WM_VSCROLL | WM_MOUSEWHEEL | WM_TIMER
        | WM_CAPTURECHANGED | ASYNC | TRAY => {
            if code == WM_DPICHANGED {
                let rectangle = *(second.0 as *const RECT);
                let _ = SetWindowPos(
                    window,
                    None,
                    rectangle.left,
                    rectangle.top,
                    rectangle.right - rectangle.left,
                    rectangle.bottom - rectangle.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            push(
                window,
                code,
                first.0,
                if code == WM_DPICHANGED { 0 } else { second.0 },
            );
            if code != WM_ACTIVATE && code != WM_SETTINGCHANGE {
                return LRESULT(0);
            }
        }
        WM_COMMAND => {
            push(window, code, first.0, second.0);
            return LRESULT(0);
        }
        WM_DRAWITEM if second.0 != 0 => {
            let item = &*(second.0 as *const DRAWITEMSTRUCT);
            if let Some(ghost) =
                GHOSTS.with_borrow(|ghosts| ghosts.get(&(item.hwndItem.0 as usize)).copied())
            {
                paint_ghost(window, item, ghost);
                return LRESULT(1);
            }
        }
        WM_PAINT if PAGERS.with_borrow(|pagers| pagers.contains_key(&(window.0 as usize))) => {
            paint_pager(window);
            return LRESULT(0);
        }
        WM_NOTIFY if second.0 != 0 && (*(second.0 as *const NMHDR)).code == NM_CUSTOMDRAW => {
            if let Some(result) = custom_draw(&*(second.0 as *const NMLVCUSTOMDRAW)) {
                return result;
            }
        }
        WM_GETMINMAXINFO if second.0 != 0 => {
            let style = GetWindowLongPtrW(window, GWL_STYLE) as u32;
            if style & WS_THICKFRAME.0 != 0 {
                let info = &mut *(second.0 as *mut MINMAXINFO);
                let dpi = GetDpiForWindow(window).max(96);
                info.ptMinTrackSize = POINT {
                    x: scale(720, dpi),
                    y: scale(420, dpi),
                };
                return LRESULT(0);
            }
        }
        WM_NOTIFY if second.0 != 0 && !SUPPRESSED.with(Cell::get) => {
            let header = &*(second.0 as *const NMHDR);
            match header.code {
                NM_CLICK
                    if GetWindowLongPtrW(header.hwndFrom, GWL_STYLE) as u32 & LVS_TYPEMASK
                        != LVS_REPORT =>
                {
                    let item = &*(second.0 as *const NMITEMACTIVATE);
                    if let Some(index) = hit_item(header.hwndFrom, item.ptAction)
                        .or((item.iItem >= 0).then_some(item.iItem as usize))
                    {
                        LAST_CLICK.with(|last| {
                            last.set(Some((
                                header.hwndFrom.0 as usize,
                                std::time::Instant::now(),
                            )))
                        });
                        push(header.hwndFrom, INVOKE_ITEM, index, 0);
                    }
                }
                LVN_ITEMACTIVATE => {
                    let item = &*(second.0 as *const NMITEMACTIVATE);
                    if item.iItem >= 0 && !double_click(header.hwndFrom) {
                        push(header.hwndFrom, INVOKE_ITEM, item.iItem as usize, 0);
                    }
                }
                LVN_GETINFOTIPW => {
                    let info = &mut *(second.0 as *mut NMLVGETINFOTIPW);
                    if info.iItem >= 0 && info.cchTextMax > 0 && !info.pszText.is_null() {
                        ITEM_TIPS.with_borrow(|tips| {
                            if let Some(text) = tips
                                .get(&(header.hwndFrom.0 as usize))
                                .and_then(|items| items.get(info.iItem as usize))
                            {
                                let length = text
                                    .len()
                                    .saturating_sub(1)
                                    .min(info.cchTextMax as usize - 1);
                                std::ptr::copy_nonoverlapping(
                                    text.as_ptr(),
                                    info.pszText.0,
                                    length,
                                );
                                *info.pszText.0.add(length) = 0;
                            }
                        });
                    }
                }
                LVN_ITEMCHANGED => {
                    let item = &*(second.0 as *const NMLISTVIEW);
                    if item.uNewState & LVIS_SELECTED.0 != 0 {
                        push(header.hwndFrom, SELECT, item.iItem.max(0) as usize, 0);
                    }
                }
                NM_DBLCLK => {
                    let item = &*(second.0 as *const NMITEMACTIVATE);
                    if item.iItem >= 0 && !double_click(header.hwndFrom) {
                        push(header.hwndFrom, ACTIVATE_ITEM, item.iItem as usize, 0);
                    }
                }
                LVN_BEGINDRAG => {
                    let item = &*(second.0 as *const NMLISTVIEW);
                    if item.iItem >= 0 {
                        push(header.hwndFrom, BEGIN_DRAG, item.iItem as usize, 0);
                    }
                }
                _ => {}
            }
        }
        WM_SETCURSOR if POINTER.with(Cell::get) && second.0 as u32 & 0xFFFF == HTCLIENT => {
            // Controls forward this to their parent, so a popup sees its icons, dots and buttons.
            let target = HWND(first.0 as *mut _);
            let mut point = POINT::default();
            let _ = GetCursorPos(&mut point);
            let _ = ScreenToClient(target, &mut point);
            let clickable = if LOOKS.with_borrow(|looks| looks.contains_key(&(target.0 as usize))) {
                hit_item(target, point).is_some()
            } else if target == window {
                pager_hit(window, point).is_some()
            } else {
                let mut class = [0u16; 16];
                let length = GetClassNameW(target, &mut class).max(0) as usize;
                !GetPropW(window, w!("AppStack.Edge")).is_invalid()
                    && String::from_utf16_lossy(&class[..length]).eq_ignore_ascii_case("button")
            };
            if clickable {
                if let Ok(hand) = LoadCursorW(None, IDC_HAND) {
                    SetCursor(Some(hand));
                    return LRESULT(1);
                }
            }
        }
        WM_WINDOWPOSCHANGED if second.0 != 0 => {
            let edge = GetPropW(window, w!("AppStack.Edge"));
            if !edge.is_invalid() {
                sync_edge(
                    window,
                    HWND(edge.0),
                    (*(second.0 as *const WINDOWPOS)).flags,
                );
            }
        }
        WM_COPYDATA if second.0 != 0 => {
            let data = &*(second.0 as *const COPYDATASTRUCT);
            if data.dwData != 0x41505354 || data.cbData > 16_384 || data.lpData.is_null() {
                return LRESULT(0);
            }
            let bytes = std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize);
            if let Ok(text) = std::str::from_utf8(bytes) {
                push_text(window, code, text.to_string());
                let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
                return LRESULT(1);
            }
            return LRESULT(0);
        }
        _ if code >= 0xC000 => {
            push(window, code, first.0, second.0);
        }
        _ => {
            if let Some(result) = paint_colors(window, code, first) {
                return result;
            }
        }
    }
    DefWindowProcW(window, code, first, second)
}

/// Whether an icon list already acted on the first click of this double-click: opening a
/// folder must not also launch whatever is under the cursor inside it.
fn double_click(list: HWND) -> bool {
    LAST_CLICK.with(Cell::get).is_some_and(|(clicked, at)| {
        clicked == list.0 as usize
            && at.elapsed().as_millis() <= u128::from(unsafe { GetDoubleClickTime() })
    })
}

/// Paints themed backgrounds and child control colors for windows registered by `apply_theme`.
unsafe fn paint_colors(window: HWND, code: u32, first: WPARAM) -> Option<LRESULT> {
    if code == WM_NCDESTROY {
        COLORS.with_borrow_mut(|colors| colors.remove(&(window.0 as usize)));
        return None;
    }
    if !matches!(
        code,
        WM_ERASEBKGND | WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX | WM_CTLCOLORBTN
    ) {
        return None;
    }
    let (background, foreground) =
        COLORS.with_borrow(|colors| colors.get(&(window.0 as usize)).copied())?;
    let dc = HDC(first.0 as *mut _);
    SetDCBrushColor(dc, COLORREF(background));
    if code == WM_ERASEBKGND {
        let mut rectangle = RECT::default();
        let _ = GetClientRect(window, &mut rectangle);
        FillRect(dc, &rectangle, HBRUSH(GetStockObject(DC_BRUSH).0));
        return Some(LRESULT(1));
    }
    SetTextColor(dc, COLORREF(foreground));
    SetBkColor(dc, COLORREF(background));
    Some(LRESULT(GetStockObject(DC_BRUSH).0 as isize))
}

/// Geometry and style of an icon-view list that AppStack paints itself.
#[derive(Clone, Copy)]
pub struct Look {
    pub cell: (i32, i32),
    pub gap: (i32, i32),
    pub radius: i32,
    pub icon: i32,
    pub dpi: u32,
    pub labels: bool,
    pub right_labels: bool,
    pub card: bool,
    pub accent: Option<u32>,
    pub hot: i32,
    /// When the icons started popping in; None once they all have.
    pub spawn: Option<std::time::Instant>,
}

impl Look {
    /// Offset of the icon inside its cell; item positions are icon positions.
    pub fn icon_offset(&self) -> (i32, i32) {
        let tile = (self.cell.0 - self.gap.0, self.cell.1 - self.gap.1);
        if self.right_labels && self.labels {
            (scale(10, self.dpi), (tile.1 - self.icon) / 2)
        } else if self.labels {
            ((tile.0 - self.icon) / 2, scale(12, self.dpi))
        } else {
            ((tile.0 - self.icon) / 2, (tile.1 - self.icon) / 2)
        }
    }
}

/// The painted cell of an item; ListView item positions are icon positions.
fn cell(list: HWND, look: &Look, item: usize) -> RECT {
    let mut position = POINT::default();
    send(
        list,
        LVM_GETITEMPOSITION,
        WPARAM(item),
        LPARAM(&mut position as *mut _ as isize),
    );
    let (offset_x, offset_y) = look.icon_offset();
    RECT {
        left: position.x - offset_x,
        top: position.y - offset_y,
        right: position.x - offset_x + look.cell.0 - look.gap.0,
        bottom: position.y - offset_y + look.cell.1 - look.gap.1,
    }
}

/// Page dots drawn in the right strip of a paged popup viewport.
#[derive(Clone, Copy, PartialEq)]
struct Pager {
    pages: i32,
    page: i32,
    strip: i32,
    dpi: u32,
}

/// Segoe Fluent Icons' "Back" glyph; Windows 10 has the same code point in Segoe MDL2 Assets.
pub const BACK_GLYPH: u16 = 0xE72B;

/// The vector icon font Windows uses for its own command buttons.
pub fn icon_font(dpi: u32, pixels: i32) -> Font {
    let family = if windows_build() >= 22000 {
        "Segoe Fluent Icons"
    } else {
        "Segoe MDL2 Assets"
    };
    Font::with(dpi, pixels, family, 400)
}

/// A flat icon button with no fill until it is hovered or pressed. `label` names it for screen
/// readers; `glyph` is drawn with the button's icon font.
pub fn ghost_button(
    parent: HWND,
    label: &str,
    glyph: u16,
    id: u16,
    font: &Font,
) -> Result<HWND, String> {
    let button = child(
        parent,
        w!("BUTTON"),
        label,
        id,
        WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
        font,
    )?;
    GHOSTS.with_borrow_mut(|ghosts| ghosts.insert(button.0 as usize, (false, glyph)));
    Ok(button)
}

pub fn forget_ghost(button: HWND) {
    GHOSTS.with_borrow_mut(|ghosts| ghosts.remove(&(button.0 as usize)));
}

/// Owner-drawn buttons get no hover state from Windows, so the message loop reports it here.
pub fn track_ghost(button: HWND, inside: bool) -> bool {
    let changed = GHOSTS.with_borrow_mut(|ghosts| {
        ghosts
            .get_mut(&(button.0 as usize))
            .map(|(hot, _)| std::mem::replace(hot, inside) != inside)
    });
    let Some(changed) = changed else {
        return false;
    };
    unsafe {
        if changed {
            let _ = InvalidateRect(Some(button), None, false);
        }
        if inside {
            let mut leave = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: button,
                ..Default::default()
            };
            let _ = TrackMouseEvent(&mut leave);
        }
    }
    true
}

unsafe fn paint_ghost(parent: HWND, item: &DRAWITEMSTRUCT, (hot, glyph): (bool, u16)) {
    let (background, foreground) = COLORS
        .with_borrow(|colors| colors.get(&(parent.0 as usize)).copied())
        .unwrap_or((GetSysColor(COLOR_WINDOW), GetSysColor(COLOR_WINDOWTEXT)));
    let dc = item.hDC;
    let bounds = item.rcItem;
    let dpi = GetDpiForWindow(parent).max(96);
    SetDCBrushColor(dc, COLORREF(background));
    FillRect(dc, &bounds, HBRUSH(GetStockObject(DC_BRUSH).0));
    let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
    let focused = item.itemState.0 & ODS_FOCUS.0 != 0 && item.itemState.0 & ODS_NOFOCUSRECT.0 == 0;
    if hot || pressed || focused {
        let fill = if pressed || hot {
            blend(background, foreground, if pressed { 14 } else { 8 })
        } else {
            background
        };
        SetDCBrushColor(dc, COLORREF(fill));
        SetDCPenColor(
            dc,
            COLORREF(if focused {
                blend(background, ACCENT.with(Cell::get), 80)
            } else {
                fill
            }),
        );
        let brush = SelectObject(dc, GetStockObject(DC_BRUSH));
        let pen = SelectObject(dc, GetStockObject(DC_PEN));
        let radius = scale(8, dpi);
        let _ = RoundRect(
            dc,
            bounds.left,
            bounds.top,
            bounds.right,
            bounds.bottom,
            radius,
            radius,
        );
        SelectObject(dc, brush);
        SelectObject(dc, pen);
    }
    let font = SelectObject(
        dc,
        HGDIOBJ(send(item.hwndItem, WM_GETFONT, WPARAM(0), LPARAM(0)).0 as *mut _),
    );
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, COLORREF(foreground));
    let mut text = [glyph];
    let mut area = RECT {
        top: bounds.top + i32::from(pressed),
        ..bounds
    };
    DrawTextW(
        dc,
        &mut text,
        &mut area,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
    SelectObject(dc, font);
}

pub fn set_pager(viewport: HWND, pager: Option<(i32, i32)>, strip: i32, dpi: u32) {
    let pager = pager.map(|(pages, page)| Pager {
        pages,
        page,
        strip,
        dpi,
    });
    let changed = PAGERS.with_borrow_mut(|pagers| match pager {
        Some(pager) => pagers.insert(viewport.0 as usize, pager) != Some(pager),
        None => pagers.remove(&(viewport.0 as usize)).is_some(),
    });
    if changed {
        unsafe {
            let _ = InvalidateRect(Some(viewport), None, true);
        }
    }
}

/// Dot rectangles, or a track and its thumb when there are more pages than room for dots.
fn pager_shapes(viewport: HWND, pager: &Pager) -> (Vec<RECT>, bool) {
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(viewport, &mut client);
    }
    let center = client.right - pager.strip / 2;
    let (width, small, large, spacing) = (
        scale(6, pager.dpi),
        scale(6, pager.dpi),
        scale(16, pager.dpi),
        scale(6, pager.dpi),
    );
    let total = pager.pages * (small + spacing) - spacing + large - small;
    let room = client.bottom - scale(8, pager.dpi);
    if total > room {
        let track = RECT {
            left: center - scale(2, pager.dpi),
            top: scale(4, pager.dpi),
            right: center + scale(2, pager.dpi),
            bottom: client.bottom - scale(4, pager.dpi),
        };
        let height = ((track.bottom - track.top) / pager.pages).max(scale(16, pager.dpi));
        let top =
            track.top + (track.bottom - track.top - height) * pager.page / (pager.pages - 1).max(1);
        let thumb = RECT {
            top,
            bottom: top + height,
            ..track
        };
        return (vec![track, thumb], true);
    }
    let mut top = (client.bottom - total) / 2;
    let dots = (0..pager.pages)
        .map(|page| {
            let height = if page == pager.page { large } else { small };
            let dot = RECT {
                left: center - width / 2,
                top,
                right: center + width / 2,
                bottom: top + height,
            };
            top += height + spacing;
            dot
        })
        .collect();
    (dots, false)
}

unsafe fn paint_pager(viewport: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let dc = BeginPaint(viewport, &mut paint);
    let pager = PAGERS.with_borrow(|pagers| pagers.get(&(viewport.0 as usize)).copied());
    let colors = COLORS.with_borrow(|colors| colors.get(&(viewport.0 as usize)).copied());
    if let (Some(pager), Some((background, foreground))) = (pager, colors) {
        let (shapes, track) = pager_shapes(viewport, &pager);
        let brush = SelectObject(dc, GetStockObject(DC_BRUSH));
        let pen = SelectObject(dc, GetStockObject(DC_PEN));
        for (index, shape) in shapes.iter().enumerate() {
            let strength = if track {
                if index == 0 { 12 } else { 60 }
            } else if index as i32 == pager.page {
                75
            } else {
                28
            };
            let color = COLORREF(blend(background, foreground, strength));
            SetDCBrushColor(dc, color);
            SetDCPenColor(dc, color);
            let radius = shape.right - shape.left;
            let _ = RoundRect(
                dc,
                shape.left,
                shape.top,
                shape.right,
                shape.bottom,
                radius,
                radius,
            );
        }
        SelectObject(dc, brush);
        SelectObject(dc, pen);
    }
    let _ = EndPaint(viewport, &paint);
}

/// The page whose dot (or track position) is under a viewport client point.
pub fn pager_hit(viewport: HWND, point: POINT) -> Option<i32> {
    let pager = PAGERS.with_borrow(|pagers| pagers.get(&(viewport.0 as usize)).copied())?;
    let (shapes, track) = pager_shapes(viewport, &pager);
    let left = shapes.first()?.left - scale(6, pager.dpi);
    if point.x < left {
        return None;
    }
    if track {
        let track = shapes[0];
        let height = (track.bottom - track.top).max(1);
        return Some(((point.y - track.top) * pager.pages / height).clamp(0, pager.pages - 1));
    }
    let spacing = scale(6, pager.dpi);
    shapes
        .iter()
        .position(|dot| point.y >= dot.top - spacing / 2 && point.y < dot.bottom + spacing / 2)
        .map(|page| page as i32)
}

pub fn set_look(list: HWND, look: Look) {
    LOOKS.with_borrow_mut(|looks| looks.insert(list.0 as usize, look));
}

/// The item whose painted cell contains a client point; ListView's own hit test only knows
/// its label boxes, which disagree with the cells after hovering.
pub fn hit_item(list: HWND, point: POINT) -> Option<usize> {
    let look = LOOKS.with_borrow(|looks| looks.get(&(list.0 as usize)).copied())?;
    let count = send(list, LVM_GETITEMCOUNT, WPARAM(0), LPARAM(0)).0.max(0) as usize;
    (0..count).find(|item| {
        let cell = cell(list, &look, *item);
        (cell.left..cell.right).contains(&point.x) && (cell.top..cell.bottom).contains(&point.y)
    })
}

/// Hover highlight for self-drawn lists; repaints only the items that change.
pub fn track_hover(list: HWND, point: Option<POINT>) {
    let hot = point
        .and_then(|point| hit_item(list, point))
        .map_or(-1, |item| item as i32);
    let previous = LOOKS.with_borrow_mut(|looks| {
        looks
            .get_mut(&(list.0 as usize))
            .map(|look| (std::mem::replace(&mut look.hot, hot), *look))
    });
    let Some((previous, look)) = previous.filter(|(previous, _)| *previous != hot) else {
        return;
    };
    for item in [previous, hot] {
        if item >= 0 {
            unsafe {
                let _ = InvalidateRect(Some(list), Some(&cell(list, &look, item as usize)), true);
            }
        }
    }
    if hot >= 0 {
        let mut leave = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: list,
            ..Default::default()
        };
        unsafe {
            let _ = TrackMouseEvent(&mut leave);
        }
    }
}

fn blend(from: u32, to: u32, percent: u32) -> u32 {
    [0, 8, 16].iter().fold(0, |color, shift| {
        let a = (from >> shift) & 0xFF;
        let b = (to >> shift) & 0xFF;
        color | (((a * (100 - percent) + b * percent) / 100) << shift)
    })
}

/// Single-line ellipsized labels and rounded highlights, like `text-overflow: ellipsis`.
unsafe fn custom_draw(draw: &NMLVCUSTOMDRAW) -> Option<LRESULT> {
    let list = draw.nmcd.hdr.hwndFrom;
    let look = LOOKS.with_borrow(|looks| looks.get(&(list.0 as usize)).copied())?;
    if CONTRAST.with(Cell::get) {
        return None;
    }
    if draw.nmcd.dwDrawStage == CDDS_PREPAINT {
        return Some(LRESULT(CDRF_NOTIFYITEMDRAW as isize));
    }
    if draw.nmcd.dwDrawStage != CDDS_ITEMPREPAINT {
        return Some(LRESULT(CDRF_DODEFAULT as isize));
    }
    let dc = draw.nmcd.hdc;
    let index = draw.nmcd.dwItemSpec;
    let cell = cell(list, &look, index);
    let (offset_x, offset_y) = look.icon_offset();
    let position = POINT {
        x: cell.left + offset_x,
        y: cell.top + offset_y,
    };
    let background = send(list, LVM_GETBKCOLOR, WPARAM(0), LPARAM(0)).0 as u32;
    let foreground = send(list, LVM_GETTEXTCOLOR, WPARAM(0), LPARAM(0)).0 as u32;
    let accent = look.accent.unwrap_or_else(|| ACCENT.with(Cell::get));
    let state = send(
        list,
        LVM_GETITEMSTATE,
        WPARAM(index),
        LPARAM(LVIS_SELECTED.0 as isize),
    )
    .0 as u32;
    let selected = state & LVIS_SELECTED.0 != 0;
    let hot = look.hot == index as i32;
    // While the popup opens, each icon fades and scales in after the previous one.
    let progress = look
        .spawn
        .map_or(1.0, |start| spawn_progress(start, index));
    if progress <= 0.0 {
        return Some(LRESULT(CDRF_SKIPDEFAULT as isize));
    }
    let appear = |color: u32| blend(background, color, (progress * 100.0) as u32);
    let fill = if selected {
        Some(blend(background, accent, if hot { 26 } else { 18 }))
    } else if hot {
        Some(blend(background, foreground, 9))
    } else if look.card {
        Some(blend(background, foreground, 4))
    } else {
        None
    }
    .map(appear);
    if let Some(fill) = fill {
        let inset = scale(2, look.dpi);
        let radius = (look.radius * 2)
            .min(cell.right - cell.left)
            .min(cell.bottom - cell.top);
        SetDCBrushColor(dc, COLORREF(fill));
        SetDCPenColor(
            dc,
            COLORREF(if selected {
                appear(blend(background, accent, 55))
            } else {
                fill
            }),
        );
        let brush = SelectObject(dc, GetStockObject(DC_BRUSH));
        let pen = SelectObject(dc, GetStockObject(DC_PEN));
        let _ = RoundRect(
            dc,
            cell.left + inset,
            cell.top + inset,
            cell.right - inset,
            cell.bottom - inset,
            radius,
            radius,
        );
        SelectObject(dc, brush);
        SelectObject(dc, pen);
    }
    let mut item = LVITEMW {
        mask: LVIF_IMAGE,
        iItem: index as i32,
        ..Default::default()
    };
    send(
        list,
        LVM_GETITEMW,
        WPARAM(0),
        LPARAM(&mut item as *mut _ as isize),
    );
    let images = HIMAGELIST(
        send(
            list,
            LVM_GETIMAGELIST,
            WPARAM(LVSIL_NORMAL as usize),
            LPARAM(0),
        )
        .0,
    );
    if progress < 1.0 {
        let size = (look.icon as f32 * pop_scale(progress)).round() as i32;
        let _ = ImageList_DrawIndirect(&IMAGELISTDRAWPARAMS {
            cbSize: size_of::<IMAGELISTDRAWPARAMS>() as u32,
            himl: images,
            i: item.iImage,
            hdcDst: dc,
            x: position.x + (look.icon - size) / 2,
            y: position.y + (look.icon - size) / 2,
            cx: size,
            cy: size,
            rgbBk: COLORREF(CLR_NONE as u32),
            rgbFg: COLORREF(CLR_NONE as u32),
            fStyle: (ILD_TRANSPARENT | ILD_SCALE).0,
            fState: ILS_ALPHA,
            Frame: (progress * 255.0) as u32,
            ..Default::default()
        });
    } else {
        let _ = ImageList_Draw(
            images,
            item.iImage,
            dc,
            position.x,
            position.y,
            ILD_TRANSPARENT,
        );
    }
    if look.labels {
        let mut text = [0u16; 260];
        let mut item = LVITEMW {
            pszText: PWSTR(text.as_mut_ptr()),
            cchTextMax: text.len() as i32,
            ..Default::default()
        };
        let length = send(
            list,
            LVM_GETITEMTEXTW,
            WPARAM(index),
            LPARAM(&mut item as *mut _ as isize),
        )
        .0 as usize;
        let length = length.min(text.len());
        let pad = scale(6, look.dpi);
        let (mut bounds, format) = if look.right_labels {
            (
                RECT {
                    left: position.x + look.icon + scale(10, look.dpi),
                    right: cell.right - pad,
                    ..cell
                },
                DT_LEFT | DT_VCENTER,
            )
        } else {
            (
                RECT {
                    left: cell.left + pad,
                    top: position.y + look.icon + scale(6, look.dpi),
                    right: cell.right - pad,
                    bottom: cell.bottom - scale(4, look.dpi),
                },
                DT_CENTER | DT_TOP,
            )
        };
        let font = SelectObject(
            dc,
            HGDIOBJ(send(list, WM_GETFONT, WPARAM(0), LPARAM(0)).0 as *mut _),
        );
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(appear(foreground)));
        DrawTextW(
            dc,
            &mut text[..length],
            &mut bounds,
            format | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(dc, font);
    }
    Some(LRESULT(CDRF_SKIPDEFAULT as isize))
}

/// A popup's outline: rounding, border and shadow, drawn by its edge window.
#[derive(Clone, Copy, PartialEq)]
pub struct Frame {
    pub radius: u32,
    pub border: u32,
    /// COLORREF; None blends the theme's text into the background.
    pub border_color: Option<u32>,
    pub shadow: u32,
}

impl Frame {
    pub fn of(appearance: &Appearance) -> Self {
        Self {
            radius: appearance.corner_radius,
            border: appearance.border_width,
            border_color: appstack::model::parse_color(&appearance.border_color).map(colorref),
            shadow: appearance.shadow,
        }
    }
}

/// Tailwind v4 `shadow-sm` to `shadow-2xl` layers: y offset, blur, spread and black opacity.
const SHADOWS: [&[(f32, f32, f32, f32)]; 6] = [
    &[],
    &[(1.0, 3.0, 0.0, 0.1), (1.0, 2.0, -1.0, 0.1)],
    &[(4.0, 6.0, -1.0, 0.1), (2.0, 4.0, -2.0, 0.1)],
    &[(10.0, 15.0, -3.0, 0.1), (4.0, 6.0, -4.0, 0.1)],
    &[(20.0, 25.0, -5.0, 0.1), (8.0, 10.0, -6.0, 0.1)],
    &[(25.0, 50.0, -12.0, 0.25)],
];

/// Room a frame's shadow needs around the popup.
fn shadow_margin(shadow: u32, dpi: u32) -> i32 {
    SHADOWS[shadow.min(5) as usize]
        .iter()
        .map(|(y, blur, spread, _)| scale((y + blur + spread.max(0.0)).ceil() as i32 + 1, dpi))
        .max()
        .unwrap_or(0)
}

/// Abramowitz and Stegun 7.1.26, plenty for a soft shadow edge.
fn erf(x: f32) -> f32 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let y = 1.0
        - ((((1.061_405_4 * t - 1.453_152) * t + 1.421_413_7) * t - 0.284_496_7) * t + 0.254_829_6)
            * t
            * (-x * x).exp();
    y.copysign(x)
}

/// Premultiplied BGRA pixels of a frame drawn `margin` pixels inside a `width` x `height`
/// canvas: an anti-aliased fill and border over its blurred shadow, like CSS. `hollow`
/// leaves the inside clear for the edge window, which sits above the popup's content.
fn frame_pixels(
    width: i32,
    height: i32,
    margin: i32,
    frame: Frame,
    (background, foreground): (u32, u32),
    dpi: u32,
    hollow: bool,
) -> Vec<u32> {
    let border = frame
        .border_color
        .unwrap_or_else(|| blend(background, foreground, 14));
    let (center_x, center_y) = (width as f32 / 2.0, height as f32 / 2.0);
    let (half_width, half_height) = (center_x - margin as f32, center_y - margin as f32);
    let radius =
        corner_diameter(frame.radius, dpi, width - margin * 2, height - margin * 2) as f32 / 2.0;
    let ring = scale(frame.border as i32, dpi) as f32;
    let pixel = dpi as f32 / 96.0;
    let layers: Vec<_> = SHADOWS[frame.shadow.min(5) as usize]
        .iter()
        .map(|(y, blur, spread, opacity)| (y * pixel, blur * pixel / 2.0, spread * pixel, *opacity))
        .collect();
    // Signed distance to the outline grown by `grow`, negative inside.
    let distance = |x: f32, y: f32, grow: f32| {
        let radius = (radius + grow).max(0.0);
        let qx = x.abs() - half_width - grow + radius;
        let qy = y.abs() - half_height - grow + radius;
        qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
    };
    let channel = |shift: u32, rim: f32, fill: f32| {
        ((border >> shift & 0xFF) as f32 * rim + (background >> shift & 0xFF) as f32 * fill + 0.5)
            as u32
    };
    (0..width * height)
        .map(|index| {
            let x = (index % width) as f32 + 0.5 - center_x;
            let y = (index / width) as f32 + 0.5 - center_y;
            let edge = distance(x, y, 0.0);
            // GDI regions stray about a pixel, so the rim covers two more inside the border.
            if hollow && edge < -(ring + 2.0) {
                return 0;
            }
            let shape = (0.5 - edge).clamp(0.0, 1.0);
            let fill = (0.5 - edge - ring).clamp(0.0, 1.0);
            let shadow = if shape < 1.0 {
                1.0 - layers
                    .iter()
                    .fold(1.0, |clear, (offset, sigma, spread, opacity)| {
                        let edge = distance(x, y - offset, *spread);
                        let cover = if *sigma > 0.0 {
                            0.5 - 0.5 * erf(edge / (sigma * std::f32::consts::SQRT_2))
                        } else {
                            (0.5 - edge).clamp(0.0, 1.0)
                        };
                        clear * (1.0 - opacity * cover)
                    })
            } else {
                0.0
            };
            let alpha = shape + shadow * (1.0 - shape);
            ((alpha * 255.0 + 0.5) as u32) << 24
                | channel(0, shape - fill, fill) << 16
                | channel(8, shape - fill, fill) << 8
                | channel(16, shape - fill, fill)
        })
        .collect()
}

/// Selects a 32-bit DIB holding `pixels` into a memory DC for `draw`.
unsafe fn with_pixels(width: i32, height: i32, pixels: &[u32], draw: impl FnOnce(HDC)) {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let dc = CreateCompatibleDC(None);
    let mut bits = std::ptr::null_mut();
    if let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u32>(), pixels.len());
        let previous = SelectObject(dc, bitmap.into());
        draw(dc);
        SelectObject(dc, previous);
        let _ = DeleteObject(bitmap.into());
    }
    let _ = DeleteDC(dc);
}

fn premultiplied(opacity: u8) -> BLENDFUNCTION {
    BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        SourceConstantAlpha: opacity,
        AlphaFormat: AC_SRC_ALPHA as u8,
        ..Default::default()
    }
}

/// Draws a frame and its shadow around `bounds`, as the popup's edge window would.
unsafe fn draw_frame(dc: HDC, bounds: RECT, frame: Frame, colors: (u32, u32), dpi: u32) {
    let margin = shadow_margin(frame.shadow, dpi);
    let width = bounds.right - bounds.left + margin * 2;
    let height = bounds.bottom - bounds.top + margin * 2;
    let pixels = frame_pixels(width, height, margin, frame, colors, dpi, false);
    with_pixels(width, height, &pixels, |source| {
        let _ = AlphaBlend(
            dc,
            bounds.left - margin,
            bounds.top - margin,
            width,
            height,
            source,
            0,
            0,
            width,
            height,
            premultiplied(255),
        );
    });
}

pub fn popup_corners(window: HWND, frame: Frame, dpi: u32) -> Result<(), String> {
    unsafe {
        // The edge window draws every corner, border and shadow; DWM would add its own.
        let _ = DwmSetWindowAttribute(
            window,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&DWMWCP_DONOTROUND as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
            size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
        let edge = attach_edge(window, frame);
        if frame.radius == 0 {
            SetWindowRgn(window, None, false);
            return Ok(());
        }
        let mut bounds = RECT::default();
        GetWindowRect(window, &mut bounds).map_err(failure)?;
        let (width, height) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
        let diameter = corner_diameter(frame.radius, dpi, width, height);
        // The edge window paints the outermost pixel, so the jagged region starts inside it.
        let inset = i32::from(edge);
        let region = CreateRoundRectRgn(
            inset,
            inset,
            width + 1 - inset,
            height + 1 - inset,
            diameter - inset * 2,
            diameter - inset * 2,
        );
        if region.is_invalid() {
            return Err("Could not create the popup corner region".into());
        }
        if SetWindowRgn(window, Some(region), false) == 0 {
            let _ = DeleteObject(region.into());
            return Err("Could not apply the popup corner region".into());
        }
    }
    Ok(())
}

fn corner_diameter(radius: u32, dpi: u32, width: i32, height: i32) -> i32 {
    scale(radius as i32 * 2, dpi).min(width).min(height)
}

pub unsafe extern "system" fn edge_procedure(
    window: HWND,
    code: u32,
    first: WPARAM,
    second: LPARAM,
) -> LRESULT {
    if code == WM_NCDESTROY {
        EDGES.with_borrow_mut(|edges| edges.remove(&(window.0 as usize)));
    }
    DefWindowProcW(window, code, first, second)
}

/// GDI regions cut stair-stepped corners and Win32 has no CSS border or shadow, so each popup
/// owns a click-through layered window that paints its rim, border and shadow over it.
unsafe fn attach_edge(popup: HWND, frame: Frame) -> bool {
    let mut edge = HWND(GetPropW(popup, w!("AppStack.Edge")).0);
    if edge.is_invalid() {
        let Ok(created) = window(
            "AppStack.Edge",
            "",
            WS_POPUP,
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            Some(popup),
            1,
            1,
        ) else {
            return false;
        };
        if SetPropW(popup, w!("AppStack.Edge"), Some(HANDLE(created.0))).is_err() {
            let _ = DestroyWindow(created);
            return false;
        }
        edge = created;
    }
    EDGES.with_borrow_mut(|edges| {
        edges
            .entry(edge.0 as usize)
            .and_modify(|(stored, _)| *stored = frame)
            .or_insert((frame, 255));
    });
    true
}

/// Fades the edge window along with its popup without drawing it again.
unsafe fn set_edge_opacity(popup: HWND, opacity: u8) {
    let edge = HWND(GetPropW(popup, w!("AppStack.Edge")).0);
    let known = EDGES.with_borrow_mut(|edges| {
        edges
            .get_mut(&(edge.0 as usize))
            .map(|(_, stored)| *stored = opacity)
            .is_some()
    });
    if known && IsWindowVisible(edge).as_bool() {
        let _ = UpdateLayeredWindow(
            edge,
            None,
            None,
            None,
            None,
            None,
            COLORREF(0),
            Some(&premultiplied(opacity)),
            ULW_ALPHA,
        );
    }
}

/// Follows the popup's position and visibility; owned windows only follow it in z-order.
unsafe fn sync_edge(popup: HWND, edge: HWND, flags: SET_WINDOW_POS_FLAGS) {
    let shown = flags.0 & (SWP_SHOWWINDOW.0 | SWP_HIDEWINDOW.0) != 0;
    if !shown && flags.0 & SWP_NOMOVE.0 != 0 && flags.0 & SWP_NOSIZE.0 != 0 {
        return;
    }
    let visible = flags.0 & SWP_HIDEWINDOW.0 == 0
        && (flags.0 & SWP_SHOWWINDOW.0 != 0 || IsWindowVisible(edge).as_bool());
    let stored = EDGES.with_borrow(|edges| edges.get(&(edge.0 as usize)).copied());
    let mut bounds = RECT::default();
    let (Some((frame, opacity)), true, Ok(())) =
        (stored, visible, GetWindowRect(popup, &mut bounds))
    else {
        let _ = ShowWindow(edge, SW_HIDE);
        return;
    };
    let dpi = GetDpiForWindow(popup).max(96);
    let margin = shadow_margin(frame.shadow, dpi);
    // A move keeps the drawing, as when the opening animation lifts the popup.
    if !shown && flags.0 & SWP_NOSIZE.0 != 0 {
        let _ = SetWindowPos(
            edge,
            None,
            bounds.left - margin,
            bounds.top - margin,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        return;
    }
    let colors = COLORS
        .with_borrow(|colors| colors.get(&(popup.0 as usize)).copied())
        .unwrap_or((GetSysColor(COLOR_WINDOW), GetSysColor(COLOR_WINDOWTEXT)));
    let width = bounds.right - bounds.left + margin * 2;
    let height = bounds.bottom - bounds.top + margin * 2;
    let pixels = frame_pixels(width, height, margin, frame, colors, dpi, true);
    with_pixels(width, height, &pixels, |source| {
        let _ = UpdateLayeredWindow(
            edge,
            None,
            Some(&POINT {
                x: bounds.left - margin,
                y: bounds.top - margin,
            }),
            Some(&SIZE {
                cx: width,
                cy: height,
            }),
            Some(source),
            Some(&POINT::default()),
            COLORREF(0),
            Some(&premultiplied(opacity)),
            ULW_ALPHA,
        );
    });
    let _ = ShowWindow(edge, SW_SHOWNA);
}

pub fn set_pointer(enabled: bool) {
    POINTER.with(|pointer| pointer.set(enabled));
}

const POP_TIMER: usize = 0x504F;
/// Seconds the popup takes to fade in, each icon takes to pop in, and between two icons.
const POP_SECONDS: f32 = 0.18;
const ICON_SECONDS: f32 = 0.26;
const STAGGER_SECONDS: f32 = 0.03;
const STAGGERED: usize = 16;

/// How far an icon's pop-in has come, 0 before it starts and 1 when it rests.
fn spawn_progress(start: std::time::Instant, index: usize) -> f32 {
    let delay = index.min(STAGGERED) as f32 * STAGGER_SECONDS;
    ((start.elapsed().as_secs_f32() - delay) / ICON_SECONDS).clamp(0.0, 1.0)
}

/// Icon scale while popping in: grows from 40% and overshoots a little, an ease-out-back.
fn pop_scale(progress: f32) -> f32 {
    let t = progress - 1.0;
    0.4 + 0.6 * (1.0 + 2.701_58 * t * t * t + 1.701_58 * t * t)
}

/// Opens a layered popup like a Tailwind transition on opacity and translate: it fades in while
/// rising a few pixels, and its icons pop in one after another.
pub fn animate_in(window: HWND, list: HWND) {
    unsafe {
        let mut bounds = RECT::default();
        if GetWindowRect(window, &mut bounds).is_err() {
            return;
        }
        let _ = SetLayeredWindowAttributes(window, COLORREF(0), 0, LWA_ALPHA);
        set_edge_opacity(window, 0);
        let start = std::time::Instant::now();
        LOOKS.with_borrow_mut(|looks| {
            if let Some(look) = looks.get_mut(&(list.0 as usize)) {
                look.spawn = Some(start);
            }
        });
        let resting = POINT {
            x: bounds.left,
            y: bounds.top,
        };
        POPS.with_borrow_mut(|pops| pops.insert(window.0 as usize, (start, resting, list)));
        let _ = SetWindowPos(
            window,
            None,
            resting.x,
            resting.y + scale(8, GetDpiForWindow(window).max(96)),
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        SetTimer(Some(window), POP_TIMER, 15, None);
    }
}

unsafe fn step_pop(window: HWND) {
    let Some((start, resting, list)) =
        POPS.with_borrow(|pops| pops.get(&(window.0 as usize)).copied())
    else {
        let _ = KillTimer(Some(window), POP_TIMER);
        return;
    };
    let elapsed = start.elapsed().as_secs_f32();
    let eased = 1.0 - (1.0 - (elapsed / POP_SECONDS).min(1.0)).powi(3);
    let opacity = (eased * 255.0).round() as u8;
    let _ = SetLayeredWindowAttributes(window, COLORREF(0), opacity, LWA_ALPHA);
    set_edge_opacity(window, opacity);
    let rise = scale(8, GetDpiForWindow(window).max(96)) as f32 * (1.0 - eased);
    let _ = SetWindowPos(
        window,
        None,
        resting.x,
        resting.y + rise.round() as i32,
        0,
        0,
        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
    );
    let _ = InvalidateRect(Some(list), None, false);
    if elapsed >= POP_SECONDS.max(STAGGERED as f32 * STAGGER_SECONDS + ICON_SECONDS) {
        let _ = KillTimer(Some(window), POP_TIMER);
        POPS.with_borrow_mut(|pops| pops.remove(&(window.0 as usize)));
        LOOKS.with_borrow_mut(|looks| {
            if let Some(look) = looks.get_mut(&(list.0 as usize)) {
                look.spawn = None;
            }
        });
    }
}

pub fn register(class: &str, procedure: WNDPROC) -> Result<(), String> {
    unsafe {
        let instance = GetModuleHandleW(None).map_err(failure)?;
        let class = wide(class);
        let definition = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: procedure,
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(failure)?,
            hIcon: LoadIconW(
                Some(instance.into()),
                PCWSTR(std::ptr::without_provenance(1)),
            )
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .map_err(failure)?,
            hbrBackground: GetSysColorBrush(COLOR_WINDOW),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        if RegisterClassExW(&definition) == 0 {
            return Err(failure(windows::core::Error::from_thread()));
        }
    }
    Ok(())
}

pub fn window(
    class: &str,
    title: &str,
    style: WINDOW_STYLE,
    extended: WINDOW_EX_STYLE,
    owner: Option<HWND>,
    width: i32,
    height: i32,
) -> Result<HWND, String> {
    unsafe {
        CreateWindowExW(
            extended,
            PCWSTR(wide(class).as_ptr()),
            PCWSTR(wide(title).as_ptr()),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            width,
            height,
            owner,
            None,
            Some(GetModuleHandleW(None).map_err(failure)?.into()),
            None,
        )
        .map_err(failure)
    }
}

pub struct Font(pub HFONT);

impl Font {
    pub fn new(dpi: u32, pixels: i32) -> Self {
        Self::with(dpi, pixels, "Segoe UI", 400)
    }

    pub fn with(dpi: u32, pixels: i32, family: &str, weight: i32) -> Self {
        // GDI names the embedded static SemiBold by its legacy family name.
        let family = match family {
            "" => "Segoe UI",
            TITLE_FONT => "Space Grotesk SemiBold",
            family => family,
        };
        let mut font = LOGFONTW {
            lfHeight: -scale(pixels, dpi),
            lfWeight: weight,
            lfCharSet: DEFAULT_CHARSET,
            lfQuality: CLEARTYPE_QUALITY,
            ..Default::default()
        };
        let family = wide(family);
        let length = family.len().min(font.lfFaceName.len() - 1);
        font.lfFaceName[..length].copy_from_slice(&family[..length]);
        Self(unsafe { CreateFontIndirectW(&font) })
    }
}

/// Registers the default fonts for this process only; Windows never sees them installed.
pub fn load_fonts() {
    for data in [
        include_bytes!("../../fonts/Inter-Regular.ttf").as_slice(),
        include_bytes!("../../fonts/SpaceGrotesk-SemiBold.ttf").as_slice(),
    ] {
        let mut count = 0;
        unsafe {
            AddFontMemResourceEx(
                data.as_ptr().cast(),
                data.len() as u32,
                None,
                std::ptr::addr_of_mut!(count),
            );
        }
    }
}

unsafe extern "system" fn collect_font(
    font: *const LOGFONTW,
    _: *const TEXTMETRICW,
    _: u32,
    names: LPARAM,
) -> i32 {
    let names = &mut *(names.0 as *mut Vec<String>);
    let face = &(*font).lfFaceName;
    let name = String::from_utf16_lossy(&face[..face.iter().position(|c| *c == 0).unwrap_or(32)]);
    if !name.starts_with('@') {
        names.push(name);
    }
    1
}

/// Installed font families, with the embedded defaults first.
pub fn font_families() -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    unsafe {
        let dc = GetDC(None);
        let filter = LOGFONTW {
            lfCharSet: DEFAULT_CHARSET,
            ..Default::default()
        };
        EnumFontFamiliesExW(
            dc,
            &filter,
            Some(collect_font),
            LPARAM(&mut names as *mut Vec<String> as isize),
            0,
        );
        ReleaseDC(None, dc);
    }
    names.sort_by_cached_key(|name| name.to_lowercase());
    names.dedup();
    names
        .retain(|name| name.as_str() != TITLE_FONT && name.as_str() != appstack::model::LABEL_FONT);
    [TITLE_FONT.to_string(), appstack::model::LABEL_FONT.into()]
        .into_iter()
        .chain(names)
        .collect()
}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

pub fn scale(value: i32, dpi: u32) -> i32 {
    ((value as i64 * dpi as i64 + 48) / 96) as i32
}

pub fn send(window: HWND, code: u32, first: WPARAM, second: LPARAM) -> LRESULT {
    unsafe { SendMessageW(window, code, Some(first), Some(second)) }
}

pub fn child(
    parent: HWND,
    class: PCWSTR,
    text: &str,
    id: u16,
    style: WINDOW_STYLE,
    font: &Font,
) -> Result<HWND, String> {
    unsafe {
        let child = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR(wide(text).as_ptr()),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            1,
            1,
            Some(parent),
            Some(HMENU(id as usize as *mut _)),
            Some(GetModuleHandleW(None).map_err(failure)?.into()),
            None,
        )
        .map_err(failure)?;
        send(child, WM_SETFONT, WPARAM(font.0.0 as usize), LPARAM(0));
        Ok(child)
    }
}

pub fn place(window: HWND, left: i32, top: i32, width: i32, height: i32) {
    unsafe {
        let _ = SetWindowPos(
            window,
            None,
            left,
            top,
            width.max(1),
            height.max(1),
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

pub fn set_text(window: HWND, text: &str) {
    unsafe {
        let _ = SetWindowTextW(window, PCWSTR(wide(text).as_ptr()));
    }
}

pub fn text_width(font: &Font, text: &str) -> i32 {
    let text: Vec<u16> = text.encode_utf16().collect();
    let mut size = SIZE::default();
    unsafe {
        let dc = GetDC(None);
        let previous = SelectObject(dc, font.0.into());
        let _ = GetTextExtentPoint32W(dc, &text, &mut size);
        SelectObject(dc, previous);
        ReleaseDC(None, dc);
    }
    size.cx
}

pub fn text(window: HWND) -> String {
    unsafe {
        let mut text = vec![0u16; GetWindowTextLengthW(window).max(0) as usize + 1];
        let count = GetWindowTextW(window, &mut text);
        String::from_utf16_lossy(&text[..count.max(0) as usize])
    }
}

pub fn list(parent: HWND, id: u16, columns: &[(&str, i32)], font: &Font) -> Result<HWND, String> {
    let list = child(
        parent,
        WC_LISTVIEWW,
        "",
        id,
        WS_TABSTOP
            | WS_BORDER
            | WINDOW_STYLE(LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | LVS_SHAREIMAGELISTS),
        font,
    )?;
    send(
        list,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        WPARAM(0),
        LPARAM((LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_INFOTIP) as isize),
    );
    for (index, (name, width)) in columns.iter().enumerate() {
        let mut name = wide(name);
        let column = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH,
            cx: *width,
            pszText: PWSTR(name.as_mut_ptr()),
            ..Default::default()
        };
        send(
            list,
            LVM_INSERTCOLUMNW,
            WPARAM(index),
            LPARAM(&column as *const _ as isize),
        );
    }
    Ok(list)
}

pub fn clear_list(list: HWND) {
    send(list, LVM_DELETEALLITEMS, WPARAM(0), LPARAM(0));
}

pub fn item_tips(list: HWND, tips: Vec<Vec<u16>>) {
    ITEM_TIPS.with_borrow_mut(|items| items.insert(list.0 as usize, tips));
}

pub fn forget_list(list: HWND) {
    ITEM_TIPS.with_borrow_mut(|items| items.remove(&(list.0 as usize)));
    LOOKS.with_borrow_mut(|looks| looks.remove(&(list.0 as usize)));
}

pub fn row(list: HWND, index: usize, columns: &[String], image: i32) {
    for (column, value) in columns.iter().enumerate() {
        let mut value = wide(value);
        let item = LVITEMW {
            mask: LVIF_TEXT
                | if column == 0 {
                    LVIF_IMAGE
                } else {
                    LIST_VIEW_ITEM_FLAGS(0)
                },
            iItem: index as i32,
            iSubItem: column as i32,
            pszText: PWSTR(value.as_mut_ptr()),
            iImage: image,
            ..Default::default()
        };
        send(
            list,
            if column == 0 {
                LVM_INSERTITEMW
            } else {
                LVM_SETITEMW
            },
            WPARAM(0),
            LPARAM(&item as *const _ as isize),
        );
    }
}

pub fn select(list: HWND, index: usize) {
    let flags = LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0);
    let item = LVITEMW {
        state: flags,
        stateMask: flags,
        ..Default::default()
    };
    send(
        list,
        LVM_SETITEMSTATE,
        WPARAM(index),
        LPARAM(&item as *const _ as isize),
    );
    send(list, LVM_ENSUREVISIBLE, WPARAM(index), LPARAM(0));
}

/// A closed hand for dragging rows, like CSS `cursor: grabbing`; Windows has no stock one.
pub fn grab_cursor(window: HWND) -> HCURSOR {
    // '#' outline, '.' fill, ' ' transparent; scaled up whole pixels with the cursor size.
    const ART: [&str; 16] = [
        "                ",
        "                ",
        "                ",
        "                ",
        "    ## ## ##    ",
        "   #..#..#..##  ",
        "   #........#.# ",
        "    #.........# ",
        "   ##.........# ",
        "  #.#.........# ",
        "  #..........#  ",
        "   #.........#  ",
        "    #.......#   ",
        "     #......#   ",
        "     #......#   ",
        "     ########   ",
    ];
    unsafe {
        let size = GetSystemMetricsForDpi(SM_CXCURSOR, GetDpiForWindow(window).max(96)).max(16);
        if let Some((_, cursor)) = GRAB.get().filter(|(built, _)| *built == size) {
            return cursor;
        }
        let side = size as usize;
        let scale = ((side + 16) / 32).clamp(1, side / 16);
        let stride = side.div_ceil(16) * 2;
        // Monochrome planes: AND 1 keeps the screen, otherwise XOR picks black (0) or white (1).
        let mut and = vec![0xFFu8; stride * side];
        let mut xor = vec![0u8; stride * side];
        for y in 0..side {
            for x in 0..side {
                let pixel = ART
                    .get(y / scale)
                    .and_then(|row| row.as_bytes().get(x / scale));
                if let Some(&pixel @ (b'#' | b'.')) = pixel {
                    let (byte, bit) = (y * stride + x / 8, 0x80u8 >> (x % 8));
                    and[byte] &= !bit;
                    if pixel == b'.' {
                        xor[byte] |= bit;
                    }
                }
            }
        }
        let hotspot = (8 * scale) as i32;
        let cursor = GetModuleHandleW(None)
            .and_then(|module| {
                CreateCursor(
                    Some(module.into()),
                    hotspot,
                    hotspot,
                    size,
                    size,
                    and.as_ptr().cast(),
                    xor.as_ptr().cast(),
                )
            })
            .or_else(|_| LoadCursorW(None, IDC_SIZEALL))
            .unwrap_or_default();
        GRAB.set(Some((size, cursor)));
        cursor
    }
}

pub fn selection(list: HWND) -> Option<usize> {
    let selected = send(
        list,
        LVM_GETNEXTITEM,
        WPARAM(usize::MAX),
        LPARAM(LVNI_SELECTED as isize),
    )
    .0;
    (selected >= 0).then_some(selected as usize)
}

pub fn work_area(owner: HWND) -> RECT {
    unsafe {
        let monitor = MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        info.rcWork
    }
}

fn high_contrast() -> bool {
    let mut contrast = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            size_of::<HIGHCONTRASTW>() as u32,
            Some((&mut contrast as *mut HIGHCONTRASTW).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    contrast.dwFlags & HCF_HIGHCONTRASTON != HIGHCONTRASTW_FLAGS(0)
}

fn registry_dword(root: HKEY, key: PCWSTR, name: PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut length = size_of::<u32>() as u32;
    unsafe {
        RegGetValueW(
            root,
            key,
            name,
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut length),
        )
        .is_ok()
        .then_some(value)
    }
}

pub fn is_dark(theme: Theme) -> bool {
    match theme {
        Theme::Light => false,
        Theme::Dark => true,
        Theme::System => {
            registry_dword(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                w!("AppsUseLightTheme"),
            ) == Some(0)
        }
    }
}

/// Converts 0xRRGGBB into a GDI COLORREF.
pub fn colorref(rgb: u32) -> u32 {
    ((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF)
}

/// Colors a window, its native children and lists; a group accent tints the background.
pub fn apply_theme(window: HWND, theme: Theme, lists: &[HWND], accent: Option<u32>) {
    let contrast = high_contrast();
    CONTRAST.with(|value| value.set(contrast));
    // DWM stores the user's accent as 0xAABBGGRR, which is already COLORREF order.
    if let Some(color) = registry_dword(
        HKEY_CURRENT_USER,
        w!("Software\\Microsoft\\Windows\\DWM"),
        w!("AccentColor"),
    ) {
        ACCENT.with(|value| value.set(color & 0xFFFFFF));
    }
    let dark = !contrast && is_dark(theme);
    unsafe {
        let (mut background, foreground) = if contrast {
            (GetSysColor(COLOR_WINDOW), GetSysColor(COLOR_WINDOWTEXT))
        } else if dark {
            (0x00202020, 0x00F3F3F3)
        } else {
            (0x00F9F9F9, 0x001A1A1A)
        };
        if let (false, Some(rgb)) = (contrast, accent) {
            background = blend(background, colorref(rgb), 12);
        }
        COLORS.with_borrow_mut(|colors| colors.insert(window.0 as usize, (background, foreground)));
        let enabled = BOOL::from(dark);
        let _ = DwmSetWindowAttribute(
            window,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&enabled as *const BOOL).cast(),
            size_of::<BOOL>() as u32,
        );
        for list in lists {
            for message in [LVM_SETBKCOLOR, LVM_SETTEXTBKCOLOR] {
                send(*list, message, WPARAM(0), LPARAM(background as isize));
            }
            send(
                *list,
                LVM_SETTEXTCOLOR,
                WPARAM(0),
                LPARAM(foreground as isize),
            );
        }
        let _ = EnumChildWindows(Some(window), Some(theme_child), LPARAM(dark as isize));
        let _ = RedrawWindow(
            Some(window),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN,
        );
    }
}

/// Gives native controls the dark or light Explorer visuals (buttons, scroll bars, lists).
unsafe extern "system" fn theme_child(child: HWND, dark: LPARAM) -> BOOL {
    // Self-drawn lists stay unthemed: themed hot items are alpha-composited, which smears GDI text.
    if LOOKS.with_borrow(|looks| looks.contains_key(&(child.0 as usize))) {
        return BOOL(1);
    }
    let mut class = [0u16; 32];
    let length = GetClassNameW(child, &mut class) as usize;
    let class = String::from_utf16_lossy(&class[..length]);
    let dark = dark.0 != 0;
    let kind = GetWindowLongPtrW(child, GWL_STYLE) as u32 & 0xF;
    let checkbox =
        class == "Button" && (kind == BS_CHECKBOX as u32 || kind == BS_AUTOCHECKBOX as u32);
    let none = PCWSTR::null();
    let (application, identifiers) = match class.as_str() {
        // Themed check boxes ignore text colors, so dark mode draws them unthemed.
        _ if checkbox && dark => (w!(""), w!("")),
        "Button" | "SysListView32" | "AppStack.Viewport" if dark => (w!("DarkMode_Explorer"), none),
        "Button" | "SysListView32" | "AppStack.Viewport" => (w!("Explorer"), none),
        "Edit" | "ComboBox" if dark => (w!("DarkMode_CFD"), none),
        "SysHeader32" if dark => (w!("DarkMode_ItemsView"), none),
        "Edit" | "ComboBox" | "SysHeader32" => (none, none),
        _ => return BOOL(1),
    };
    let _ = SetWindowTheme(child, application, identifiers);
    BOOL(1)
}

pub fn windows_build() -> u32 {
    let mut text = [0u16; 16];
    let mut length = size_of::<[u16; 16]>() as u32;
    unsafe {
        let _ = RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(text.as_mut_ptr().cast()),
            Some(&mut length),
        );
    }
    String::from_utf16_lossy(&text[..text.iter().position(|c| *c == 0).unwrap_or(0)])
        .parse()
        .unwrap_or(0)
}

/// Popup and context menus follow the manager theme where Windows supports it (1903+).
pub fn set_menu_theme(theme: Theme) {
    let build = windows_build();
    if build < 18362 {
        return;
    }
    unsafe {
        let Ok(module) = GetModuleHandleW(w!("uxtheme.dll")) else {
            return;
        };
        // Undocumented uxtheme ordinals: 135 SetPreferredAppMode, 136 FlushMenuThemes.
        if let Some(set) = GetProcAddress(module, PCSTR(135 as *const u8)) {
            let set: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set);
            set(match theme {
                Theme::System => 1,
                Theme::Dark => 2,
                Theme::Light => 3,
            });
        }
        if let Some(flush) = GetProcAddress(module, PCSTR(136 as *const u8)) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
            flush();
        }
    }
}

pub enum Field {
    Preset {
        label: String,
        values: Vec<String>,
        measurements: Vec<u32>,
        selected: usize,
    },
    Page {
        label: String,
    },
    Text {
        label: String,
        value: String,
    },
    /// A #RRGGBB text box with a swatch that opens the Windows color picker.
    Color {
        label: String,
        value: String,
    },
    Check {
        label: String,
        value: bool,
    },
    Choice {
        label: String,
        values: Vec<String>,
        selected: usize,
    },
}

impl Field {
    pub fn preset(label: &str, presets: &[(&str, u32)], value: u32) -> Self {
        Self::Preset {
            label: label.into(),
            values: presets.iter().map(|(name, _)| (*name).into()).collect(),
            measurements: presets
                .iter()
                .map(|(_, measurement)| *measurement)
                .collect(),
            selected: presets
                .iter()
                .enumerate()
                .min_by_key(|(_, (_, measurement))| measurement.abs_diff(value))
                .map_or(0, |(index, _)| index),
        }
    }
    pub fn page(label: &str) -> Self {
        Self::Page {
            label: label.into(),
        }
    }
    pub fn text(label: &str, value: impl ToString) -> Self {
        Self::Text {
            label: label.into(),
            value: value.to_string(),
        }
    }
    pub fn color(label: &str, value: &str) -> Self {
        Self::Color {
            label: label.into(),
            value: value.into(),
        }
    }
    pub fn check(label: &str, value: bool) -> Self {
        Self::Check {
            label: label.into(),
            value,
        }
    }
    pub fn choice(label: &str, values: &[&str], selected: usize) -> Self {
        Self::Choice {
            label: label.into(),
            values: values.iter().map(|value| value.to_string()).collect(),
            selected,
        }
    }
}

pub struct GroupPreview {
    pub group: Group,
    pub settings: Settings,
    pub icons: Vec<(String, super::icons::Icon)>,
    pub update: fn(&mut Group, &[String]) -> Result<(), String>,
}

unsafe fn preview_shape(dc: HDC, bounds: RECT, radius: i32, fill: u32, border: u32) {
    SetDCBrushColor(dc, COLORREF(fill));
    SetDCPenColor(dc, COLORREF(border));
    let brush = SelectObject(dc, GetStockObject(DC_BRUSH));
    let pen = SelectObject(dc, GetStockObject(DC_PEN));
    let diameter = (radius * 2)
        .min(bounds.right - bounds.left)
        .min(bounds.bottom - bounds.top)
        .max(0);
    if diameter == 0 {
        let _ = Rectangle(dc, bounds.left, bounds.top, bounds.right, bounds.bottom);
    } else {
        let _ = RoundRect(
            dc,
            bounds.left,
            bounds.top,
            bounds.right,
            bounds.bottom,
            diameter,
            diameter,
        );
    }
    SelectObject(dc, brush);
    SelectObject(dc, pen);
}

unsafe fn preview_text(
    dc: HDC,
    value: &str,
    bounds: RECT,
    font: &Font,
    color: u32,
    flags: DRAW_TEXT_FORMAT,
) {
    let previous = SelectObject(dc, font.0.into());
    SetTextColor(dc, COLORREF(color));
    SetBkMode(dc, TRANSPARENT);
    let mut bounds = bounds;
    let mut value = wide(value);
    if value.last() == Some(&0) {
        value.pop();
    }
    DrawTextW(
        dc,
        &mut value,
        &mut bounds,
        flags | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
    SelectObject(dc, previous);
}

unsafe fn paint_group_preview(window: HWND, item: &DRAWITEMSTRUCT, preview: &GroupPreview) {
    let dc = item.hDC;
    let outer = item.rcItem;
    let (surface, ink) = COLORS
        .with_borrow(|colors| colors.get(&(window.0 as usize)).copied())
        .unwrap_or((GetSysColor(COLOR_WINDOW), GetSysColor(COLOR_WINDOWTEXT)));
    preview_shape(dc, outer, 0, surface, surface);
    let look = &preview.group.appearance;
    let settings = &preview.settings;
    let entries = preview.group.ordered_entries();
    let count = if entries.is_empty() {
        4
    } else {
        entries.len().min(8)
    };
    let columns = look.columns.min(count).max(1);
    let rows = look.visible_rows(count.div_ceil(columns));
    let cell = look.cell_size();
    let grid_width = columns as i32 * cell.0 - look.gap_horizontal as i32;
    let grid_height = rows as i32 * cell.1 - look.gap_vertical as i32;
    let show_all = look.show_launch_all_button && settings.launch_all_enabled;
    let show_buttons = look.show_edit_button || show_all;
    let header_band = look.show_header.then_some(look.header_bottom);
    let button_band = show_buttons.then_some(look.button_position == ButtonPosition::Bottom);
    let top_band = header_band == Some(false) || button_band == Some(false);
    let bottom_band = header_band == Some(true) || button_band == Some(true);
    let shared = show_buttons && header_band == button_band;
    let title_font = Font::with(96, look.label_size as i32 + 4, &settings.title_font, 600);
    let action_font = Font::new(96, 13);
    let action_widths: Vec<_> = [(look.show_edit_button, "Edit"), (show_all, "Launch all")]
        .into_iter()
        .filter(|(show, _)| *show)
        .map(|(_, name)| text_width(&action_font, name) + 24)
        .collect();
    let actions_width =
        action_widths.iter().sum::<i32>() + 6 * action_widths.len().saturating_sub(1) as i32;
    let title_width = if look.show_header {
        text_width(&title_font, &preview.group.name).min(240) + 6
    } else {
        0
    };
    let pages = if look.scroll {
        entries.len().max(1).div_ceil(columns).div_ceil(rows) as i32
    } else {
        1
    };
    // Paged popups keep a strip for their dots; a pill mirrors it to stay centered.
    let strip = if pages > 1 { 14 } else { 0 };
    let mirror = if look.corner_radius == ROUNDED_FULL {
        strip
    } else {
        0
    };
    let body_width = (grid_width + strip + mirror).max(if shared {
        title_width + actions_width
    } else {
        title_width.max(actions_width)
    });
    let bands = 40 * (i32::from(top_band) + i32::from(bottom_band));
    // Like Tailwind's border-box: the border sits inside the popup, before the padding.
    let border = look.border_width as i32;
    let (inset_x, inset_y) =
        look.rounding_inset(body_width + border * 2, grid_height + bands + border * 2);
    let padding_x = look.padding_horizontal as i32 + inset_x + border;
    let padding_y = look.padding_vertical as i32 + inset_y + border;
    let width = (body_width + padding_x * 2).max(look.minimum_width as i32);
    let height = grid_height + bands + padding_y * 2;
    let shadow = shadow_margin(look.shadow, 96);
    let available_width = (outer.right - outer.left - 32 - shadow * 2).max(1);
    let available_height = (outer.bottom - outer.top - 52 - shadow * 2).max(1);
    let zoom = (available_width as f64 / width.max(1) as f64)
        .min(available_height as f64 / height.max(1) as f64)
        .min(GetDpiForWindow(window).max(96) as f64 / 96.0);
    let pixels = |value: i32| (value as f64 * zoom).round() as i32;
    let left = outer.left + (outer.right - outer.left - pixels(width)) / 2;
    let top = outer.top + 32 + shadow + (available_height - pixels(height)) / 2;
    let frame = RECT {
        left,
        top,
        right: left + pixels(width),
        bottom: top + pixels(height),
    };
    let dpi = (zoom * 96.0).round().max(24.0) as u32;
    let font = Font::with(dpi, look.label_size as i32 + 1, &settings.label_font, 400);
    let title_font = Font::with(dpi, look.label_size as i32 + 4, &settings.title_font, 600);
    let button_font = Font::new(dpi, 13);
    let heading_font = Font::new(GetDpiForWindow(window).max(96), 14);
    preview_text(
        dc,
        "Preview",
        RECT {
            left: outer.left + 12,
            top: outer.top,
            right: outer.right - 12,
            bottom: outer.top + 28,
        },
        &heading_font,
        ink,
        DT_LEFT | DT_VCENTER,
    );
    let dark = is_dark(settings.popup_theme);
    let mut background = if dark { 0x202020 } else { 0xF9F9F9 };
    let foreground = if dark { 0xF2F2F2 } else { 0x181818 };
    let accent = appstack::model::parse_color(&look.accent)
        .map(colorref)
        .unwrap_or(ACCENT.with(Cell::get));
    if !look.accent.is_empty() {
        background = blend(background, accent, 12);
    }
    let radius = corner_diameter(
        look.corner_radius,
        dpi,
        frame.right - frame.left,
        frame.bottom - frame.top,
    ) / 2;
    draw_frame(dc, frame, Frame::of(look), (background, foreground), dpi);
    let saved = SaveDC(dc);
    let region = CreateRoundRectRgn(
        frame.left,
        frame.top,
        frame.right + 1,
        frame.bottom + 1,
        radius * 2,
        radius * 2,
    );
    if !region.is_invalid() {
        SelectClipRgn(dc, Some(region));
        let _ = DeleteObject(region.into());
    }
    let tile_look = Look {
        cell: (pixels(cell.0), pixels(cell.1)),
        gap: (
            pixels(look.gap_horizontal as i32),
            pixels(look.gap_vertical as i32),
        ),
        radius: pixels(look.tile_radius.min(ROUNDED_FULL) as i32),
        icon: pixels(look.icon_size as i32),
        dpi,
        labels: look.show_labels,
        right_labels: look.label_position == LabelPosition::Right,
        card: look.card_layout,
        accent: Some(accent),
        hot: -1,
        spawn: None,
    };
    let offset = tile_look.icon_offset();
    let grid_left = left + (pixels(width) - pixels(grid_width) + pixels(mirror - strip)) / 2;
    let grid_top = top + pixels(padding_y + if top_band { 40 } else { 0 });
    for index in 0..count.min(rows * columns) {
        let tile = RECT {
            left: grid_left + pixels((index % columns) as i32 * cell.0),
            top: grid_top + pixels((index / columns) as i32 * cell.1),
            right: grid_left
                + pixels((index % columns + 1) as i32 * cell.0 - look.gap_horizontal as i32),
            bottom: grid_top
                + pixels((index / columns + 1) as i32 * cell.1 - look.gap_vertical as i32),
        };
        if look.card_layout || index == 0 {
            let fill = blend(
                background,
                if index == 0 { accent } else { foreground },
                if index == 0 { 14 } else { 4 },
            );
            let tile_radius = if look.tile_radius == ROUNDED_FULL {
                (tile.right - tile.left).min(tile.bottom - tile.top) / 2
            } else {
                pixels(look.tile_radius as i32)
            };
            let tile = RECT {
                left: tile.left + pixels(2),
                top: tile.top + pixels(2),
                right: tile.right - pixels(2),
                bottom: tile.bottom - pixels(2),
            };
            preview_shape(dc, tile, tile_radius, fill, fill);
        }
        if let Some((_, icon)) = entries
            .get(index)
            .and_then(|entry| preview.icons.iter().find(|(id, _)| id == &entry.id))
            .or_else(|| preview.icons.first())
        {
            let _ = DrawIconEx(
                dc,
                tile.left + offset.0,
                tile.top + offset.1,
                icon.handle(),
                tile_look.icon,
                tile_look.icon,
                0,
                None,
                DI_NORMAL,
            );
        }
        if look.show_labels {
            let label = entries
                .get(index)
                .map_or("Application", |entry| entry.name.as_str());
            let text_area = if tile_look.right_labels {
                RECT {
                    left: tile.left + offset.0 + tile_look.icon + pixels(10),
                    right: tile.right - pixels(6),
                    ..tile
                }
            } else {
                RECT {
                    left: tile.left + pixels(6),
                    top: tile.top + offset.1 + tile_look.icon + pixels(6),
                    right: tile.right - pixels(6),
                    bottom: tile.bottom - pixels(4),
                }
            };
            preview_text(
                dc,
                label,
                text_area,
                &font,
                foreground,
                if tile_look.right_labels {
                    DT_LEFT | DT_VCENTER
                } else {
                    DT_CENTER | DT_TOP
                },
            );
        }
    }
    // Page dots like the popup's pager, as many as fit beside the visible rows.
    let dots = pages.min((grid_height - 4) / 12);
    if dots > 1 {
        let center = frame.right - pixels(padding_x + strip / 2);
        let mut dot_top = grid_top + (pixels(grid_height) - pixels(dots * 12 + 4)) / 2;
        for page in 0..dots {
            let height = if page == 0 { 16 } else { 6 };
            let dot = RECT {
                left: center - pixels(3),
                top: dot_top,
                right: center + pixels(3),
                bottom: dot_top + pixels(height),
            };
            let color = blend(background, foreground, if page == 0 { 75 } else { 28 });
            preview_shape(dc, dot, pixels(3), color, color);
            dot_top += pixels(height + 6);
        }
    }
    let band_top = |bottom: bool| {
        top + pixels(if bottom {
            padding_y + if top_band { 40 } else { 0 } + grid_height
        } else {
            padding_y
        })
    };
    let mut title_right = frame.right - pixels(padding_x);
    if let Some(bottom) = button_band {
        let widths: Vec<_> = action_widths.iter().map(|width| pixels(*width)).collect();
        let positions = super::app::button_positions(
            left + pixels(padding_x + if shared { title_width } else { 0 }),
            frame.right - pixels(padding_x),
            &widths,
            pixels(6),
            look.button_alignment,
        );
        if shared {
            if let Some(first) = positions.first() {
                title_right = *first - pixels(6);
            }
        }
        for (((_, label), button_width), position) in
            [(look.show_edit_button, "Edit"), (show_all, "Launch all")]
                .into_iter()
                .filter(|(show, _)| *show)
                .zip(widths)
                .zip(positions)
        {
            let bounds = RECT {
                left: position,
                top: band_top(bottom) + pixels(5),
                right: position + button_width,
                bottom: band_top(bottom) + pixels(35),
            };
            preview_shape(
                dc,
                bounds,
                pixels(4),
                background,
                blend(background, foreground, 24),
            );
            preview_text(
                dc,
                label,
                bounds,
                &button_font,
                foreground,
                DT_CENTER | DT_VCENTER,
            );
        }
    }
    if let Some(bottom) = header_band {
        preview_text(
            dc,
            &preview.group.name,
            RECT {
                left: left + pixels(padding_x + 3),
                top: band_top(bottom),
                right: title_right,
                bottom: band_top(bottom) + pixels(40),
            },
            &title_font,
            foreground,
            DT_LEFT | DT_VCENTER,
        );
    }
    if saved != 0 {
        let _ = RestoreDC(dc, saved);
    }
}

unsafe fn paint_preview_buffered(window: HWND, item: &DRAWITEMSTRUCT, preview: &GroupPreview) {
    let bounds = item.rcItem;
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    let dc = CreateCompatibleDC(Some(item.hDC));
    if dc.is_invalid() {
        paint_group_preview(window, item, preview);
        return;
    }
    let bitmap = CreateCompatibleBitmap(item.hDC, width, height);
    if bitmap.is_invalid() {
        let _ = DeleteDC(dc);
        paint_group_preview(window, item, preview);
        return;
    }
    let previous = SelectObject(dc, bitmap.into());
    let mut buffered = *item;
    buffered.hDC = dc;
    paint_group_preview(window, &buffered, preview);
    let _ = BitBlt(
        item.hDC,
        bounds.left,
        bounds.top,
        width,
        height,
        Some(dc),
        0,
        0,
        SRCCOPY,
    );
    SelectObject(dc, previous);
    let _ = DeleteObject(bitmap.into());
    let _ = DeleteDC(dc);
}

fn form_values(fields: &[Field], controls: &[Option<HWND>]) -> Vec<String> {
    fields
        .iter()
        .zip(controls)
        .filter_map(|(field, control)| {
            let control = (*control)?;
            Some(match field {
                Field::Text { .. } | Field::Color { .. } => text(control),
                Field::Check { .. } => {
                    (send(control, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1).to_string()
                }
                Field::Choice { .. } => send(control, CB_GETCURSEL, WPARAM(0), LPARAM(0))
                    .0
                    .to_string(),
                Field::Preset { measurements, .. } => measurements
                    .get(send(control, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0.max(0) as usize)
                    .copied()
                    .unwrap_or_default()
                    .to_string(),
                Field::Page { .. } => return None,
            })
        })
        .collect()
}

#[derive(Default)]
struct FormState {
    done: Cell<Option<bool>>,
    tab: Cell<HWND>,
    pages: RefCell<Vec<Vec<HWND>>>,
    preview: RefCell<Option<GroupPreview>>,
    preview_window: Cell<HWND>,
    preview_dirty: Cell<bool>,
}

impl FormState {
    fn refresh_preview(&self, fields: &[Field], controls: &[Option<HWND>]) {
        if !self.preview_dirty.replace(false) {
            return;
        }
        let updated = self
            .preview
            .borrow_mut()
            .as_mut()
            .map(|preview| (preview.update)(&mut preview.group, &form_values(fields, controls)))
            .is_some();
        if !updated {
            return;
        }
        unsafe {
            let window = self.preview_window.get();
            let revision = GetPropW(window, w!("AppStack.PreviewRevision")).0 as usize + 1;
            let _ = InvalidateRect(Some(window), None, false);
            let _ = SetPropW(
                window,
                w!("AppStack.PreviewRevision"),
                Some(HANDLE(std::ptr::without_provenance_mut(revision))),
            );
            NotifyWinEvent(EVENT_OBJECT_STATECHANGE, window, 0, 0);
        }
    }

    fn select_page(&self, window: HWND, selected: usize) {
        let visible = unsafe { IsWindowVisible(window).as_bool() };
        if visible {
            send(window, WM_SETREDRAW, WPARAM(0), LPARAM(0));
        }
        send(self.tab.get(), TCM_SETCURSEL, WPARAM(selected), LPARAM(0));
        for (page, children) in self.pages.borrow().iter().enumerate() {
            for child in children {
                unsafe {
                    let _ = ShowWindow(*child, if page == selected { SW_SHOWNA } else { SW_HIDE });
                }
            }
        }
        if visible {
            send(window, WM_SETREDRAW, WPARAM(1), LPARAM(0));
            unsafe {
                let _ = RedrawWindow(
                    Some(window),
                    None,
                    None,
                    RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW,
                );
            }
        }
    }
}

unsafe extern "system" fn form_procedure(
    window: HWND,
    code: u32,
    first: WPARAM,
    second: LPARAM,
) -> LRESULT {
    if code == WM_NCCREATE {
        let create = &*(second.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let state = GetWindowLongPtrW(window, GWLP_USERDATA) as *const FormState;
    if !state.is_null() {
        if code == WM_DRAWITEM && second.0 != 0 {
            let item = &*(second.0 as *const DRAWITEMSTRUCT);
            if item.CtlID == 50 {
                if let Some(preview) = (*state).preview.borrow().as_ref() {
                    paint_preview_buffered(window, item, preview);
                    return LRESULT(1);
                }
            }
            if item.CtlID >= u32::from(SWATCH) {
                paint_swatch(window, item);
                return LRESULT(1);
            }
        }
        if code == WM_COMMAND {
            let id = (first.0 & 0xFFFF) as i32;
            let notification = (first.0 >> 16) as u32;
            if id >= i32::from(SWATCH) && notification == BN_CLICKED {
                if let Ok(edit) = GetDlgItem(Some(window), id - i32::from(SWATCH)) {
                    pick_color(window, edit);
                }
            } else if notification == EN_CHANGE {
                if let Ok(swatch) = GetDlgItem(Some(window), id + i32::from(SWATCH)) {
                    let _ = InvalidateRect(Some(swatch), None, false);
                }
            }
        }
        if code == WM_COMMAND && first.0 & 0xFFFF >= 100 {
            (*state).preview_dirty.set(true);
        }
        if code == WM_NOTIFY && second.0 != 0 {
            let header = &*(second.0 as *const NMHDR);
            if header.hwndFrom == (*state).tab.get() && header.code == TCN_SELCHANGE {
                let selected = send(header.hwndFrom, TCM_GETCURSEL, WPARAM(0), LPARAM(0))
                    .0
                    .max(0) as usize;
                (*state).select_page(window, selected);
                return LRESULT(0);
            }
        }
        if code == WM_CLOSE || (code == WM_COMMAND && first.0 & 0xFFFF == IDCANCEL.0 as usize) {
            (*state).done.set(Some(false));
            return LRESULT(0);
        }
        if code == WM_COMMAND && first.0 & 0xFFFF == IDOK.0 as usize {
            (*state).done.set(Some(true));
            return LRESULT(0);
        }
    }
    if let Some(result) = paint_colors(window, code, first) {
        return result;
    }
    DefWindowProcW(window, code, first, second)
}

/// Added to a color field's control id for its swatch button.
const SWATCH: u16 = 900;

/// A color field's swatch: the chosen color, or "Auto" while the field is empty.
unsafe fn paint_swatch(window: HWND, item: &DRAWITEMSTRUCT) {
    let (background, foreground) = COLORS
        .with_borrow(|colors| colors.get(&(window.0 as usize)).copied())
        .unwrap_or((GetSysColor(COLOR_WINDOW), GetSysColor(COLOR_WINDOWTEXT)));
    let dc = item.hDC;
    let bounds = item.rcItem;
    let color = GetDlgItem(Some(window), item.CtlID as i32 - i32::from(SWATCH))
        .ok()
        .and_then(|edit| appstack::model::parse_color(&text(edit)))
        .map(colorref);
    let active = item.itemState.0 & (ODS_SELECTED.0 | ODS_FOCUS.0) != 0;
    preview_shape(dc, bounds, 0, background, background);
    preview_shape(
        dc,
        bounds,
        scale(6, GetDpiForWindow(window).max(96)),
        color.unwrap_or(background),
        blend(background, foreground, if active { 60 } else { 30 }),
    );
    if color.is_none() {
        let font = SelectObject(
            dc,
            HGDIOBJ(send(item.hwndItem, WM_GETFONT, WPARAM(0), LPARAM(0)).0 as *mut _),
        );
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(foreground));
        let mut label = wide("Auto");
        label.pop();
        let mut area = bounds;
        DrawTextW(
            dc,
            &mut label,
            &mut area,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        SelectObject(dc, font);
    }
}

/// Opens the Windows color picker on a color field and writes the choice back as #RRGGBB.
unsafe fn pick_color(owner: HWND, edit: HWND) {
    let mut custom = CUSTOM_COLORS.with(Cell::get);
    let current = appstack::model::parse_color(&text(edit)).map(colorref);
    let mut choice = CHOOSECOLORW {
        lStructSize: size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: owner,
        rgbResult: COLORREF(current.unwrap_or_else(|| ACCENT.with(Cell::get))),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_RGBINIT | CC_FULLOPEN | CC_ANYCOLOR,
        ..Default::default()
    };
    if ChooseColorW(&mut choice).as_bool() {
        CUSTOM_COLORS.with(|colors| colors.set(custom));
        let color = choice.rgbResult.0;
        set_text(
            edit,
            &format!(
                "#{:02X}{:02X}{:02X}",
                color & 0xFF,
                color >> 8 & 0xFF,
                color >> 16 & 0xFF
            ),
        );
    }
}

pub fn register_form() -> Result<(), String> {
    register("AppStack.Form", Some(form_procedure))
}

pub fn checklist(
    owner: HWND,
    title: &str,
    items: &[(String, String)],
    theme: Theme,
) -> Result<Option<Vec<usize>>, String> {
    unsafe {
        let dpi = GetDpiForWindow(owner).max(96);
        let font = Font::new(dpi, 14);
        let area = work_area(owner);
        let width = scale(640, dpi).min(area.right - area.left - scale(24, dpi));
        let height = scale(480, dpi).min(area.bottom - area.top - scale(24, dpi));
        let state = Box::new(FormState {
            done: Cell::new(None),
            ..FormState::default()
        });
        let window = CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            w!("AppStack.Form"),
            PCWSTR(wide(title).as_ptr()),
            WS_CAPTION | WS_SYSMENU | WS_POPUP,
            area.left + (area.right - area.left - width) / 2,
            area.top + (area.bottom - area.top - height) / 2,
            width,
            height,
            Some(owner),
            None,
            Some(GetModuleHandleW(None).map_err(failure)?.into()),
            Some((&*state as *const FormState).cast()),
        )
        .map_err(failure)?;
        let mut bounds = RECT::default();
        let _ = GetClientRect(window, &mut bounds);
        let margin = scale(14, dpi);
        let list = list(
            window,
            20,
            &[("Group", width * 2 / 3), ("Applications", width / 4)],
            &font,
        )?;
        place(
            list,
            margin,
            margin,
            bounds.right - margin * 2,
            bounds.bottom - scale(70, dpi),
        );
        send(
            list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            WPARAM(0),
            LPARAM((LVS_EX_CHECKBOXES | LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize),
        );
        for (index, (name, count)) in items.iter().enumerate() {
            row(list, index, &[name.clone(), count.clone()], -1);
            let checked = LVITEMW {
                state: LIST_VIEW_ITEM_STATE_FLAGS(2 << 12),
                stateMask: LIST_VIEW_ITEM_STATE_FLAGS(0xF000),
                ..Default::default()
            };
            send(
                list,
                LVM_SETITEMSTATE,
                WPARAM(index),
                LPARAM(&checked as *const _ as isize),
            );
        }
        let accept = child(
            window,
            w!("BUTTON"),
            "Import",
            IDOK.0 as u16,
            WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
            &font,
        )?;
        let cancel = child(
            window,
            w!("BUTTON"),
            "Cancel",
            IDCANCEL.0 as u16,
            WS_TABSTOP,
            &font,
        )?;
        apply_theme(window, theme, &[list], None);
        place(
            accept,
            bounds.right - scale(194, dpi),
            bounds.bottom - scale(44, dpi),
            scale(84, dpi),
            scale(28, dpi),
        );
        place(
            cancel,
            bounds.right - scale(100, dpi),
            bounds.bottom - scale(44, dpi),
            scale(84, dpi),
            scale(28, dpi),
        );
        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetFocus(Some(list));
        let mut message = MSG::default();
        let mut quit = None;
        while state.done.get().is_none() {
            if GetMessageW(&mut message, None, 0, 0).0 <= 0 {
                quit = Some(message.wParam.0 as i32);
                break;
            }
            if !(WM_KEYFIRST..=WM_KEYLAST).contains(&message.message)
                || !IsDialogMessageW(window, &message).as_bool()
            {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let selected = if state.done.get() == Some(true) {
            Some(
                (0..items.len())
                    .filter(|index| {
                        (send(list, LVM_GETITEMSTATE, WPARAM(*index), LPARAM(0xF000)).0 >> 12) == 2
                    })
                    .collect(),
            )
        } else {
            None
        };
        let _ = EnableWindow(owner, true);
        let _ = DestroyWindow(window);
        let _ = SetForegroundWindow(owner);
        if let Some(code) = quit {
            PostQuitMessage(code);
        }
        Ok(selected)
    }
}

unsafe extern "system" fn form_tab_painter(
    window: HWND,
    code: u32,
    first: WPARAM,
    second: LPARAM,
    identifier: usize,
    _: usize,
) -> LRESULT {
    if code == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(window, Some(form_tab_painter), identifier);
    }
    if code == WM_PAINT {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(window, &mut paint);
        let owner = GetParent(window).unwrap_or_default();
        let (background, foreground) = COLORS
            .with_borrow(|colors| colors.get(&(owner.0 as usize)).copied())
            .unwrap_or((GetSysColor(COLOR_WINDOW), GetSysColor(COLOR_WINDOWTEXT)));
        let mut bounds = RECT::default();
        let _ = GetClientRect(window, &mut bounds);
        preview_shape(dc, bounds, 0, background, background);
        let selected = send(window, TCM_GETCURSEL, WPARAM(0), LPARAM(0)).0;
        let count = send(window, TCM_GETITEMCOUNT, WPARAM(0), LPARAM(0))
            .0
            .max(0);
        let font = SelectObject(
            dc,
            HGDIOBJ(send(window, WM_GETFONT, WPARAM(0), LPARAM(0)).0 as *mut _),
        );
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(foreground));
        for index in 0..count {
            let mut item_bounds = RECT::default();
            send(
                window,
                TCM_GETITEMRECT,
                WPARAM(index as usize),
                LPARAM(&mut item_bounds as *mut _ as isize),
            );
            let mut text = [0u16; 128];
            let mut item = TCITEMW {
                mask: TCIF_TEXT,
                pszText: PWSTR(text.as_mut_ptr()),
                cchTextMax: text.len() as i32,
                ..Default::default()
            };
            send(
                window,
                TCM_GETITEMW,
                WPARAM(index as usize),
                LPARAM(&mut item as *mut _ as isize),
            );
            if index == selected {
                let accent = if CONTRAST.with(Cell::get) {
                    foreground
                } else {
                    ACCENT.with(Cell::get)
                };
                preview_shape(
                    dc,
                    RECT {
                        top: item_bounds.bottom - scale(2, GetDpiForWindow(window)),
                        ..item_bounds
                    },
                    0,
                    accent,
                    accent,
                );
            }
            let length = text
                .iter()
                .position(|character| *character == 0)
                .unwrap_or(text.len());
            DrawTextW(
                dc,
                &mut text[..length],
                &mut item_bounds,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
            if index == selected
                && GetFocus() == window
                && send(window, WM_QUERYUISTATE, WPARAM(0), LPARAM(0)).0 & 1 == 0
            {
                let _ = DrawFocusRect(dc, &item_bounds);
            }
        }
        SelectObject(dc, font);
        let _ = EndPaint(window, &paint);
        return LRESULT(0);
    }
    DefSubclassProc(window, code, first, second)
}

pub fn form(
    owner: HWND,
    title: &str,
    fields: &[Field],
    theme: Theme,
) -> Result<Option<Vec<String>>, String> {
    form_with_preview(owner, title, fields, theme, None, None)
}

pub fn form_with_preview(
    owner: HWND,
    title: &str,
    fields: &[Field],
    theme: Theme,
    preview: Option<GroupPreview>,
    mut live_preview: Option<&mut dyn FnMut(&mut GroupPreview) -> bool>,
) -> Result<Option<Vec<String>>, String> {
    unsafe {
        let dpi = GetDpiForWindow(owner).max(96);
        let font = Font::new(dpi, 14);
        let area = work_area(owner);
        let page_labels: Vec<_> = fields
            .iter()
            .filter_map(|field| match field {
                Field::Page { label } => Some(label.as_str()),
                _ => None,
            })
            .collect();
        let paged = !page_labels.is_empty();
        let row_height = if paged { 54 } else { 60 };
        let mut page_counts = vec![0usize; page_labels.len().max(1)];
        let (mut page, mut seen_page) = (0, false);
        for field in fields {
            if matches!(field, Field::Page { .. }) {
                if seen_page {
                    page += 1;
                }
                seen_page = true;
            } else {
                page_counts[page] += 1;
            }
        }
        let page_columns: Vec<_> = page_counts
            .iter()
            .map(|count| {
                if paged && *count > 20 {
                    3
                } else if paged || *count > 8 {
                    2
                } else {
                    1
                }
            })
            .collect();
        let columns = *page_columns.iter().max().unwrap_or(&1);
        let rows = page_counts
            .iter()
            .zip(&page_columns)
            .map(|(count, columns)| count.div_ceil(*columns))
            .max()
            .unwrap_or(1)
            .max(1);
        let has_preview = preview.is_some();
        let width = scale(
            if has_preview {
                1240
            } else {
                match columns {
                    3 => 940,
                    2 => 740,
                    _ => 440,
                }
            },
            dpi,
        )
        .min(area.right - area.left - scale(24, dpi));
        let height = scale(
            rows as i32 * row_height + if paged { 144 } else { 100 },
            dpi,
        )
        .min(area.bottom - area.top - scale(24, dpi));
        let state = Box::new(FormState {
            done: Cell::new(None),
            preview: RefCell::new(preview),
            preview_dirty: Cell::new(has_preview),
            ..FormState::default()
        });
        let window = CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            w!("AppStack.Form"),
            PCWSTR(wide(title).as_ptr()),
            WS_CAPTION | WS_SYSMENU | WS_POPUP,
            area.left + (area.right - area.left - width) / 2,
            area.top + (area.bottom - area.top - height) / 2,
            width,
            height,
            Some(owner),
            None,
            Some(GetModuleHandleW(None).map_err(failure)?.into()),
            Some((&*state as *const FormState).cast()),
        )
        .map_err(failure)?;
        let mut controls = Vec::new();
        let mut rectangle = RECT::default();
        let _ = GetClientRect(window, &mut rectangle);
        let fields_width = if has_preview {
            rectangle.right * 3 / 5
        } else {
            rectangle.right
        };
        if paged {
            let tab = child(window, WC_TABCONTROLW, "", 40, WS_TABSTOP, &font)?;
            place(
                tab,
                scale(14, dpi),
                scale(12, dpi),
                fields_width - scale(28, dpi),
                scale(32, dpi),
            );
            for (index, label) in page_labels.iter().enumerate() {
                let mut label = wide(label);
                let item = TCITEMW {
                    mask: TCIF_TEXT,
                    pszText: PWSTR(label.as_mut_ptr()),
                    ..Default::default()
                };
                send(
                    tab,
                    TCM_INSERTITEMW,
                    WPARAM(index),
                    LPARAM(&item as *const _ as isize),
                );
            }
            state.tab.set(tab);
            let _ = SetWindowSubclass(tab, Some(form_tab_painter), 1, 0);
        }
        let mut pages = vec![Vec::new(); page_counts.len()];
        let mut offsets = vec![0usize; page_counts.len()];
        let (mut page, mut seen_page) = (0, false);
        for (index, field) in fields.iter().enumerate() {
            if matches!(field, Field::Page { .. }) {
                if seen_page {
                    page += 1;
                }
                seen_page = true;
                controls.push(None);
                continue;
            }
            let columns = page_columns[page];
            let rows = page_counts[page].div_ceil(columns).max(1);
            let column_width = (fields_width - scale(36, dpi)) / columns as i32;
            let offset = offsets[page];
            offsets[page] += 1;
            let left = scale(14, dpi) + (offset / rows) as i32 * column_width;
            let top = scale(
                if paged { 58 } else { 12 } + (offset % rows) as i32 * row_height,
                dpi,
            );
            let id = 100 + index as u16;
            let control = match field {
                Field::Text { label, value } | Field::Color { label, value } => {
                    let label = child(window, w!("STATIC"), label, 0, WINDOW_STYLE(0), &font)?;
                    pages[page].push(label);
                    place(
                        label,
                        left,
                        top,
                        column_width - scale(14, dpi),
                        scale(20, dpi),
                    );
                    let control = child(
                        window,
                        w!("EDIT"),
                        value,
                        id,
                        WS_TABSTOP | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                        &font,
                    )?;
                    let swatch = if matches!(field, Field::Color { .. }) {
                        scale(52, dpi)
                    } else {
                        0
                    };
                    place(
                        control,
                        left,
                        top + scale(22, dpi),
                        column_width - scale(14, dpi) - swatch,
                        scale(26, dpi),
                    );
                    send(control, EM_SETLIMITTEXT, WPARAM(8192), LPARAM(0));
                    if swatch > 0 {
                        let button = child(
                            window,
                            w!("BUTTON"),
                            "Pick a color",
                            id + SWATCH,
                            WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
                            &font,
                        )?;
                        place(
                            button,
                            left + column_width - scale(14, dpi) - swatch + scale(6, dpi),
                            top + scale(22, dpi),
                            swatch - scale(6, dpi),
                            scale(26, dpi),
                        );
                        pages[page].push(button);
                    }
                    control
                }
                Field::Check { label, value } => {
                    let control = child(
                        window,
                        w!("BUTTON"),
                        label,
                        id,
                        WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                        &font,
                    )?;
                    place(
                        control,
                        left,
                        top + scale(20, dpi),
                        column_width - scale(14, dpi),
                        scale(28, dpi),
                    );
                    send(control, BM_SETCHECK, WPARAM(usize::from(*value)), LPARAM(0));
                    control
                }
                Field::Choice {
                    label,
                    values,
                    selected,
                }
                | Field::Preset {
                    label,
                    values,
                    selected,
                    ..
                } => {
                    let label = child(window, w!("STATIC"), label, 0, WINDOW_STYLE(0), &font)?;
                    pages[page].push(label);
                    place(
                        label,
                        left,
                        top,
                        column_width - scale(14, dpi),
                        scale(20, dpi),
                    );
                    let control = child(
                        window,
                        w!("COMBOBOX"),
                        "",
                        id,
                        WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
                        &font,
                    )?;
                    place(
                        control,
                        left,
                        top + scale(22, dpi),
                        column_width - scale(14, dpi),
                        scale(240, dpi),
                    );
                    for value in values {
                        send(
                            control,
                            CB_ADDSTRING,
                            WPARAM(0),
                            LPARAM(wide(value).as_ptr() as isize),
                        );
                    }
                    send(control, CB_SETCURSEL, WPARAM(*selected), LPARAM(0));
                    control
                }
                Field::Page { .. } => unreachable!(),
            };
            pages[page].push(control);
            controls.push(Some(control));
        }
        if has_preview {
            let preview = child(
                window,
                w!("STATIC"),
                "Live preview",
                50,
                WINDOW_STYLE(0x000D),
                &font,
            )?;
            place(
                preview,
                fields_width + scale(6, dpi),
                scale(12, dpi),
                rectangle.right - fields_width - scale(20, dpi),
                rectangle.bottom - scale(76, dpi),
            );
            state.preview_window.set(preview);
            state.refresh_preview(fields, &controls);
        }
        let accept = child(
            window,
            w!("BUTTON"),
            "Save",
            IDOK.0 as u16,
            WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
            &font,
        )?;
        let cancel = child(
            window,
            w!("BUTTON"),
            "Cancel",
            IDCANCEL.0 as u16,
            WS_TABSTOP,
            &font,
        )?;
        place(
            accept,
            rectangle.right - scale(190, dpi),
            rectangle.bottom - scale(44, dpi),
            scale(80, dpi),
            scale(28, dpi),
        );
        place(
            cancel,
            rectangle.right - scale(100, dpi),
            rectangle.bottom - scale(44, dpi),
            scale(80, dpi),
            scale(28, dpi),
        );
        apply_theme(window, theme, &[], None);
        if paged {
            state.pages.replace(pages);
            state.select_page(window, 0);
        }
        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(window, SW_SHOW);
        if let Some(first) = controls
            .iter()
            .flatten()
            .find(|control| IsWindowVisible(**control).as_bool())
        {
            let _ = SetFocus(Some(*first));
        }
        let mut message = MSG::default();
        let mut quit = None;
        while state.done.get().is_none() {
            let status = GetMessageW(&mut message, None, 0, 0).0;
            if status <= 0 {
                quit = Some(message.wParam.0 as i32);
                break;
            }
            if !(WM_KEYFIRST..=WM_KEYLAST).contains(&message.message)
                || !IsDialogMessageW(window, &message).as_bool()
            {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            if let Some(update) = live_preview.as_mut() {
                let preview = state.preview.borrow_mut().take();
                if let Some(mut preview) = preview {
                    let changed = update(&mut preview);
                    state.preview.replace(Some(preview));
                    if changed {
                        state.preview_dirty.set(true);
                    }
                }
            }
            state.refresh_preview(fields, &controls);
        }
        let result = if state.done.get() == Some(true) {
            Some(form_values(fields, &controls))
        } else {
            None
        };
        let _ = EnableWindow(owner, true);
        let _ = DestroyWindow(window);
        let _ = SetForegroundWindow(owner);
        if let Some(code) = quit {
            PostQuitMessage(code);
        }
        Ok(result)
    }
}
