# AppStack

Native application groups for Windows 10/11, version 1.0.0. One executable,
`AppStackClassic.exe`, built with Rust and plain Win32: native editor, dialogs and
popups. No WinUI, Windows App SDK, .NET, Electron or WebView, and no runtime to
install.

## Install

`dist` contains a per-user installer (`AppStackClassic-1.0.0-setup.exe`, no
administrator rights, English and Spanish) and a portable ZIP. The installer puts
the program in `%LOCALAPPDATA%\Programs\AppStackClassic`, adds a Start menu
shortcut (desktop shortcut optional) and registers an uninstaller under Installed
apps. Uninstalling removes the program, its shortcuts and the start-with-Windows
entry, and asks before deleting your groups.

## Build And Run

Windows 10/11 x64, Rust 1.85+ with the MSVC toolchain, and Visual Studio Build
Tools with the Desktop development with C++ workload and a Windows SDK are
required.

```powershell
cd AppStack
.\scripts\build.ps1
.\target\release\AppStackClassic.exe
```

Use `-DebugBuild` for development and `-CheckOnly` for a type check. The script
links the MSVC CRT statically, so the executable needs no Visual C++
redistributable. `cargo classic` is a plain Cargo shortcut for a release build.
Installers need NSIS (`makensis`) on `PATH`; without it the packager only
produces the ZIP.

## Logo

Edit `appstack.svg`, then rebuild. `build.rs` watches it and embeds 16,
20, 24, 32, 40, 48, 64, 128 and 256 pixel ICO images, used for the executable,
windows, dialogs, the tray and fallback icons. A content revision invalidates the
persistent icon cache and refreshes generated group shortcuts in the background
on restart. Custom application/group icons remain custom. Restart a running copy
after rebuilding; Explorer may retain its own icon cache until it refreshes.

## Groups

- Create, edit, duplicate and delete groups; reorder groups and applications by
  dragging inside their lists or using their menu commands.
- Add files through the native picker, drag applications from Explorer or
  `shell:AppsFolder`, or enter a path, URL or Shell identifier manually.
- Popups work like folders: drag applications onto an open popup to add them to
  the folder it shows, or onto a subgroup tile (it lights up) to add them to that
  subgroup. As when moving between Explorer folders, shortcut files (`.lnk`,
  `.url`) leave where they were, such as the desktop, for the data folder's
  `items`; hold Ctrl to leave them in place. Programs, documents and Start menu
  apps are only referenced. Moves use Explorer's own file operation, so its
  prompts (administrator permission for the shared Public desktop) and Undo
  apply, and entries in any group that pointed at a moved shortcut follow it. The
  popup stays open while a mouse button is held outside it, so an icon can be
  picked up and dropped on it; a press outside that ends without a drop closes it.
- Right-click a popup for **Edit group** (the folder shown) and **New group**.
  The manager's group list and the tray menu also offer **New group**; in the
  group list, empty space offers only that.
- Support for Win32 applications, Windows apps, `.lnk`, `.url`, Steam shortcuts,
  PWAs, arguments, custom icons, tooltips and administrator execution.
- Add existing groups as subgroups. Cycles are rejected. Launch all follows
  the displayed order, including subgroups, and deduplicates identical launches.
- Group options include the group icon style (**Single**, **Grid**, or
  **Mosaic**, a grid that alternates large and small icons), columns, icon
  labels on/off, label position and size, headers, compact/card spacing, accent
  colors and per-group tray icons.
- The group editor separates **General** settings from **Interface** settings.
  Interface uses fixed size selects (`xs`, `sm`, `md`, `lg`, `xl`, `2xl`, `3xl`)
  rather than arbitrary pixel inputs. Spacing follows a four-pixel scale; icon,
  text and width presets have their own appropriate scales. Width also has
  `auto`, spacing has `none`, and rounding follows Tailwind-style tokens from
  `none` through `4xl` and `full`. Full rounding creates a real pill, like CSS
  `rounded-full`: the round ends span the popup's short side and only the long
  side gets extra room so the content clears them. Every popup has anti-aliased
  edges, a border (`0`, `1`, `2`, `4` or `8` pixels, inside the padding like
  Tailwind's border-box) and a shadow from Tailwind's `sm` through `2xl` scale.
  Accent and border colors are typed as `#RRGGBB` or picked from the Windows
  color picker beside each field; empty follows the theme.
  Sizes follow monitor DPI and the popup remains constrained to the work area.
- **Popup style** at the top of Interface fills in every Interface setting with a
  ready-made look (Dock, Bubble, List, Cards, Launchpad, Minimal, Indigo or
  Sunset), which can then be adjusted before **Save**.
- A live preview beside the settings shows the group's labels, icons, spacing,
  actions and rounding as controls change. Icons load asynchronously using the
  existing workers. Preview changes are not persisted until **Save**; **Cancel**
  leaves the configuration untouched. Existing custom numeric settings still
  load; editing presents the nearest standard preset and only Save applies it.
- Each group chooses whether its popup shows the **Edit** and **Launch all**
  buttons and whether they sit at the top or the bottom (sharing the title row
  when both are on the same side). Left, center and right alignment apply to
  the visible action group, including when only one button is enabled.
- **Scroll inside the group** limits the popup to a number of visible rows and
  pages like the Windows 11 Start menu: each wheel notch (or Page Up/Down) slides
  one whole page and dots on the right show and select the page. Without it, the
  popup grows to fit (still scrolling if the screen is too small).
- Subgroups open inside the same popup with a back button (Esc or Backspace also
  goes back). The popup is as wide as its widest folder and each folder gets the
  rows it needs. Edit and Launch all act on the folder shown.
- Long names are truncated on one line with an ellipsis, like CSS
  `text-overflow: ellipsis`; the full name appears as a tooltip.
- Preferences choose the folder title font and the icon label font. Space Grotesk
  (titles) and Inter (labels) are embedded and are the defaults; any installed font
  can be selected. **Hand cursor on clickable popup items** (on by default) shows
  the hand over applications, buttons and page dots, like CSS `cursor: pointer`.
  **Popup animations** fades each popup in as it rises a few pixels and pops its
  icons in one after another.
- Main and popup themes are independent, including dark buttons, scroll bars,
  lists, dialogs and context menus. High contrast takes precedence over custom
  colors. The manager's menu bar stays light because Win32 does not theme it.
- `F2` edits, `Delete` removes, `Enter` opens or launches, and `Ctrl+N` creates a
  group. In popups, arrows navigate, `Enter` launches, and `Esc` goes back one
  folder or closes.

## Shortcuts And Taskbar

Use **Group > Create desktop shortcut**, or **Open shortcut location**. Drag a
group outside the manager to begin a native Shell drag of its `.lnk` file.
Windows controls whether and how an item can be pinned; no unsupported taskbar
patching or automatic forced pinning is used.

Shortcuts use stable group identifiers and distinct AppUserModelIDs. Renaming a
group retains the generated shortcut path. Jump Lists expose Edit and Launch all.
Updating these resources runs in the background after edits, not when opening a
popup. Windows privacy or taskbar policy can restrict Jump Lists.

## Import And Backups

Use **File > Import backup / JSON** for AppStack `.agz` backups or
`appstack.json` files. The folder import accepts an AppStack data folder or
TaskbarGroups' `ObjectData.xml` configuration folders.

Import displays a scrollable selection preview. Required subgroups are included
with selected parents. Existing groups are never replaced: identifiers are
remapped, original files remain untouched, and available custom icons are copied.
Unavailable application paths are retained for repair, not silently deleted.

AppStack exports `.agz` ZIP archives containing its versioned JSON, custom
image assets and the shortcut files dropped into its `items` folder. Installed
applications are not embedded in backups.

## Storage

The default data directory is `%LOCALAPPDATA%\AppStackClassic`. For portable use,
put an empty `portable.txt` beside the executable, or pass `--portable`; data then
lives in `config` beside the executable. `--data-dir` selects an explicit
directory. Each directory has its own resident instance.

Shortcuts dragged into popups live in the data directory's `items` folder;
removing their application from a group leaves the file there.

`appstack.json` is written atomically, retaining `appstack.json.bak`. Invalid or
unsupported configurations produce an error and are not overwritten. To recover,
close AppStack and restore the backup. Exports remain separate backups.

## Commands

```powershell
.\AppStackClassic.exe --group GROUP_ID
.\AppStackClassic.exe --edit GROUP_ID
.\AppStackClassic.exe --launch-all GROUP_ID
.\AppStackClassic.exe --silent
.\AppStackClassic.exe --quit
.\AppStackClassic.exe --data-dir "D:\My groups"
.\AppStackClassic.exe --group GROUP_ID --position 800,800
```

Names are also accepted, but identifiers are unambiguous. `--check` validates
the configuration and `--list` lists groups; redirect standard output when using
these diagnostic commands from a GUI-subsystem executable.
`--position X,Y` supplies an explicit screen anchor, including negative monitor
coordinates. Without it, the invocation captures the cursor position once.

## Performance

The resident dispatcher sleeps in the Windows message loop. It has no global
mouse hook, polling loop, artificial delays, splash screen or loading overlay.
Popup anchoring captures the invocation position once. Normal mouse movement
does not reposition a popup. The one exception to idle sleeping is an open popup
without activation (a press outside it, or after a drop): it checks the mouse
buttons every 50 ms until it closes or is activated again.

For taskbar and Jump List launches, Windows supplies the source monitor. AppStack
anchors the popup to its group button's accessible rectangle on that monitor,
not the cursor position after process startup. The button reference is reused
and its bounds are checked on each invocation. `--position` still overrides this.

Each GUI invocation enters the Windows message queue immediately to end the
Shell's startup feedback cursor, including invocations forwarded to the resident
instance. This cannot remove time spent loading the executable before its code
runs. Tray setup, shortcut regeneration and cache maintenance start after the
requested popup's first paint, rather than competing with it.

Popups use one native ListView rather than a window per application, painted by
AppStack itself (custom draw), so hover only repaints the two cells that change.
Up to four previously opened popups are reused; the editor is created on demand.
Silent startup prepares up to four pinned groups with at most 32 entries without
showing their popups, reusing the same bounded window cache. This moves work to
resident preparation; it does not eliminate startup work. The small forwarding
process delay-loads COM, Shell, themes and controls until they are actually used.
Reopening checks that cache before copying or sorting application entries.
Showing a popup flushes pending paint without invalidating the entire window.
Folder changes freeze painting until the new layout is final, then repaint once.
Icon extraction and storage operations run on separate workers. In-memory icons
are capped at 256 entries; persistent cache maintenance targets 1,024 icons / 64
MiB. On a first cache miss, a stable fallback icon is used without delaying input.
Repeated icons share a native image-list entry within each popup, so a completed
icon load updates one image instead of copying it into every matching cell.

The performance harness now defaults to eight applications with distinct real
Windows executable targets. It invokes `.lnk` files through `ShellExecuteEx`,
without a synthetic `--position`, and records visibility, completion of the
first paint and the next DWM presentation separately. Resident instances stay
alive between samples; the first popup and preparation costs are retained
separately instead of being mixed into cached reopening statistics:

```powershell
.\tests\startup.ps1 -Applications 8
.\tests\startup.ps1 -Applications 8 -Resident
```

For the actual taskbar route, pass an existing pinned group shortcut. This
injects a physical click into its taskbar button, immediately moves the pointer,
and checks that the popup remains anchored to the button. It reads but does not
edit the profile, restores the pointer and closes only an instance it started:

```powershell
$shortcut = Join-Path $env:APPDATA 'Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\AppStackClassic - Work.lnk'
.\tests\startup.ps1 -Resident -Samples 7 -PinnedShortcut $shortcut
```

On the development desktop, the six-application Work group's latest seven physical
reopenings measured 162 ms median / 189 ms p95, with zero anchoring failures across
all eight clicks including the first. Resident preparation took 638 ms and the
first click took 209 ms. Earlier runs reached 1,401 ms on their first click.
First-launch and antivirus costs remain variable; these figures are measurements,
not native-speed guarantees. The older portable measured 165 ms median / 194 ms
p95 and eight anchoring failures, but it cannot verify the newer first-paint
marker (`-Legacy` diagnostic mode). Reopening speed has not improved consistently;
the reproducible improvement is stable taskbar anchoring during cursor movement.

`-MoveCursor:$false` disables pointer movement. The physical test moves it by
default; the unpinned `.lnk` harness does not. `-BaselineExecutable` compares
compatible binaries with separate profiles. Cold means a new process, not purged
OS caches. Screenshots, timing samples and definitions are retained under `target`.

## Verification

```powershell
cargo fmt --all -- --check
.\scripts\build.ps1
.\tests\smoke.ps1 -Executable .\target\release\AppStackClassic.exe
.\tests\smoke.ps1 -Executable .\target\release\AppStackClassic.exe -EditorOnly -Theme Light
.\tests\smoke.ps1 -Executable .\target\release\AppStackClassic.exe -EditorOnly -Theme Dark
.\scripts\package.ps1
```

The smoke test uses isolated generated data, 1,003 applications, native window
events and UI Automation. It verifies anchoring, scrolling, virtualization,
subfolders inside the popup, popup reuse, launching, editing and atomic
persistence. Screenshots and timing samples are written under `target/smoke-*`.
The focused `-EditorOnly` check verifies named selects, changing preview pixels,
Cancel isolation, saved presets and an actual pill-shaped popup region; its
screenshots are retained under `target/preview-*`.
No elevation is requested. `package.ps1` checks formatting, Clippy and Rust tests,
then creates the portable ZIP and the installer.

Before publishing, manually validate taskbar dragging/pinning on supported Windows
versions, several monitors with mixed DPI, screen readers and administrator/UAC
flows. These depend on the real desktop and system policy. No automatic update
service or release endpoint is configured.

## Code Layout

`model` owns ordered groups, validation and launch planning. `storage` owns atomic
JSON and imports/archives. `command` owns invocation parsing. `win` contains the
dispatcher, native editor and popups, Shell integration, COM drag/drop and worker
queues. Win32 callbacks enqueue copied events; application state is updated outside
the callbacks. Native icons, fonts, image lists and COM values have explicit owners.

See LICENSE and THIRD-PARTY-NOTICES.md.
