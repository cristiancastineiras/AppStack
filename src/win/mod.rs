mod app;
mod drop_target;
mod icons;
mod jobs;
pub mod shell;
mod ui;

use appstack::{command::Options, storage::Store};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{DataExchange::COPYDATASTRUCT, Threading::*},
        UI::{Accessibility::NotifyWinEvent, Controls::*, HiDpi::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, w},
};

struct Instance {
    mutex: HANDLE,
    ready: HANDLE,
    owner: bool,
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            if self.owner {
                let _ = ReleaseMutex(self.mutex);
            }
            let _ = CloseHandle(self.mutex);
            let _ = CloseHandle(self.ready);
        }
    }
}

fn finish_startup_feedback() -> Result<(), String> {
    const FEEDBACK: u32 = WM_APP + 1;
    unsafe {
        let mut message = MSG::default();
        let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
        PostThreadMessageW(GetCurrentThreadId(), FEEDBACK, WPARAM(0), LPARAM(0))
            .map_err(shell::failure)?;
        if GetMessageW(&mut message, None, FEEDBACK, FEEDBACK).0 <= 0 {
            return Err("Could not finish Windows startup feedback".into());
        }
    }
    Ok(())
}

fn startup_anchor(startup: &STARTUPINFOW, fallback: POINT) -> (POINT, bool) {
    unsafe {
        let mut monitor = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !startup.dwFlags.contains(STARTF_USESTDHANDLES)
            && !startup.hStdOutput.is_invalid()
            && GetMonitorInfoW(HMONITOR(startup.hStdOutput.0), &mut monitor).as_bool()
        {
            return (
                POINT {
                    x: (monitor.rcMonitor.left + monitor.rcMonitor.right) / 2,
                    y: (monitor.rcMonitor.top + monitor.rcMonitor.bottom) / 2,
                },
                true,
            );
        }
    }
    (fallback, false)
}

fn invocation_point() -> (POINT, Option<POINT>) {
    unsafe {
        let mut current = POINT::default();
        let _ = GetPhysicalCursorPos(&mut current);
        let mut startup = STARTUPINFOW::default();
        GetStartupInfoW(&mut startup);
        let (origin, taskbar) = startup_anchor(&startup, current);
        (current, taskbar.then_some(origin))
    }
}

pub fn run() -> Result<(), String> {
    let startup_clock = std::time::Instant::now();
    let startup_trace = std::env::var_os("APPSTACK_STARTUP_TRACE");
    let mut startup_stages = Vec::new();
    macro_rules! startup_stage {
        ($name:literal) => {
            if startup_trace.is_some() {
                startup_stages.push(($name, startup_clock.elapsed().as_micros()));
            }
        };
    }
    let (point, taskbar) = invocation_point();
    finish_startup_feedback()?;
    startup_stage!("feedback");
    let mut options = Options::parse(std::env::args().skip(1))?;
    options.request.taskbar = if options.position.is_none() {
        taskbar.map(|origin| (origin.x, origin.y))
    } else {
        None
    };
    let (left, top) = options.position.unwrap_or((point.x, point.y));
    options.request.x = left;
    options.request.y = top;
    if options.help {
        println!(
            "AppStack 1.0.0\n\nappstack [--group ID|NAME] [--edit ID|NAME] [--launch-all ID|NAME]\n         [--silent] [--quit] [--portable] [--data-dir DIRECTORY]\n         [--check] [--list]"
        );
        return Ok(());
    }
    let executable = std::env::current_exe().map_err(shell::failure)?;
    let portable_marker = executable
        .parent()
        .is_some_and(|parent| parent.join("portable.txt").exists());
    let data_dir = options.data_dir.or_else(|| {
        if options.portable || portable_marker {
            None
        } else {
            std::env::var_os("LOCALAPPDATA")
                .map(|root| std::path::PathBuf::from(root).join(crate::APP_NAME))
        }
    });
    let store = Store::discover(options.portable, data_dir)?;
    startup_stage!("paths");
    if options.check || options.list {
        let config = store.load()?;
        if options.list {
            for group in config.ordered_groups() {
                println!("{}\t{}\t{}", group.id, group.name, group.entries.len());
            }
        } else {
            println!(
                "OK: {} groups; {}",
                config.groups.len(),
                store.root.display()
            );
        }
        return Ok(());
    }
    let mut hash = DefaultHasher::new();
    store.root.to_string_lossy().to_lowercase().hash(&mut hash);
    std::env::var("USERNAME")
        .unwrap_or_default()
        .hash(&mut hash);
    let identity = format!("AppStack.{:016x}", hash.finish());
    let class = format!("{identity}.Host");
    let instance = unsafe {
        let mutex = CreateMutexW(
            None,
            false,
            PCWSTR(shell::wide(format!("Local\\{identity}")).as_ptr()),
        )
        .map_err(shell::failure)?;
        let ready = CreateEventW(
            None,
            true,
            false,
            PCWSTR(shell::wide(format!("Local\\{identity}.Ready")).as_ptr()),
        )
        .map_err(shell::failure)?;
        let status = WaitForSingleObject(mutex, 0);
        Instance {
            mutex,
            ready,
            owner: status == WAIT_OBJECT_0 || status == WAIT_ABANDONED,
        }
    };
    if !instance.owner {
        unsafe {
            if WaitForSingleObject(instance.ready, 5000) != WAIT_OBJECT_0 {
                return Err("The running AppStack instance did not become ready".into());
            }
            let owner =
                FindWindowW(PCWSTR(shell::wide(&class).as_ptr()), None).map_err(shell::failure)?;
            if ui::text(owner) != format!("{} dispatcher", crate::APP_NAME) {
                return Err("This profile is open in the other edition. Close it first, or select a different --data-dir.".into());
            }
            let mut process = 0;
            GetWindowThreadProcessId(owner, Some(&mut process));
            let _ = AllowSetForegroundWindow(process);
            let bytes = serde_json::to_vec(&options.request).map_err(shell::failure)?;
            let data = COPYDATASTRUCT {
                dwData: 0x41505354,
                cbData: bytes.len() as u32,
                lpData: bytes.as_ptr().cast_mut().cast(),
            };
            let mut accepted = 0;
            let result = SendMessageTimeoutW(
                owner,
                WM_COPYDATA,
                WPARAM(0),
                LPARAM(&data as *const _ as isize),
                SMTO_ABORTIFHUNG,
                3000,
                Some(&mut accepted),
            );
            if result.0 == 0 || accepted != 1 {
                return Err("The running AppStack instance is not responding".into());
            }
        }
        return Ok(());
    }
    startup_stage!("single_instance");
    unsafe {
        let _ = ResetEvent(instance.ready);
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let _apartment = shell::Apartment::new()?;
    startup_stage!("com");
    unsafe {
        let _ = InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES | ICC_STANDARD_CLASSES | ICC_TAB_CLASSES | ICC_UPDOWN_CLASS,
        });
    }
    ui::load_fonts();
    ui::register(&class, Some(ui::procedure))?;
    ui::register("AppStack.Manager", Some(ui::procedure))?;
    ui::register("AppStack.Popup", Some(ui::procedure))?;
    ui::register("AppStack.Viewport", Some(ui::procedure))?;
    ui::register("AppStack.Edge", Some(ui::edge_procedure))?;
    ui::register_form()?;
    startup_stage!("window_classes");
    let host = ui::window(
        &class,
        &format!("{} dispatcher", crate::APP_NAME),
        WS_POPUP,
        WS_EX_TOOLWINDOW,
        None,
        1,
        1,
    )?;
    startup_stage!("host");
    let config = store.load()?;
    startup_stage!("configuration");
    let mut application = app::Application::new(host, store, config)?;
    startup_stage!("application");
    unsafe {
        let _ = SetEvent(instance.ready);
    }
    let prewarm = matches!(&options.request.action, appstack::command::Action::Silent);
    application.request(options.request)?;
    startup_stage!("requested_window");
    if !application.quitting {
        application.initialize_shell();
        if prewarm {
            let _ = application.prewarm_popups(point);
        }
    }
    unsafe {
        SetPropW(
            host,
            w!("AppStack.DispatcherReady"),
            Some(HANDLE(std::ptr::without_provenance_mut(1))),
        )
        .map_err(shell::failure)?;
        NotifyWinEvent(EVENT_OBJECT_STATECHANGE, host, 0, 0);
    }
    startup_stage!("shell_integration");
    if let Some(path) = startup_trace {
        let report = serde_json::json!({ "stages_us": startup_stages });
        let _ = std::fs::write(path, report.to_string());
    }
    loop {
        while let Some(event) = ui::pop() {
            if let Err(error) = application.event(event) {
                #[cfg(debug_assertions)]
                eprintln!("event error: {error}");
                application.report_error(&error);
            }
            if application.quitting {
                break;
            }
        }
        if application.quitting {
            break;
        }
        let mut message = MSG::default();
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if result == 0 {
            break;
        }
        if result < 0 {
            return Err("The Windows message loop failed".into());
        }
        if application.scroll(&message) {
            continue;
        }
        match application.keyboard(&message) {
            Ok(true) => continue,
            Err(error) => {
                shell::message(Some(application.host), "AppStack", &error, true);
                continue;
            }
            _ => {}
        }
        if (WM_KEYFIRST..=WM_KEYLAST).contains(&message.message) {
            if let Some(dialog) = application.dialog_window(message.hwnd) {
                if unsafe { IsDialogMessageW(dialog, &message) }.as_bool() {
                    continue;
                }
            }
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taskbar_launch_uses_its_monitor_instead_of_the_live_cursor() {
        let monitor = unsafe { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTONEAREST) };
        let startup = STARTUPINFOW {
            hStdOutput: HANDLE(monitor.0),
            ..Default::default()
        };
        let (first, taskbar) = startup_anchor(&startup, POINT { x: -2000, y: -2000 });
        let (second, _) = startup_anchor(&startup, POINT { x: 2000, y: 2000 });
        assert!(taskbar);
        assert_eq!((first.x, first.y), (second.x, second.y));
    }

    #[test]
    fn ordinary_launches_preserve_the_captured_cursor() {
        let (point, taskbar) = startup_anchor(&STARTUPINFOW::default(), POINT { x: -1500, y: 800 });
        assert!(!taskbar);
        assert_eq!((point.x, point.y), (-1500, 800));
    }

    #[test]
    fn startup_feedback_preserves_other_thread_messages() {
        std::thread::spawn(|| unsafe {
            let mut message = MSG::default();
            let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
            PostThreadMessageW(GetCurrentThreadId(), WM_APP, WPARAM(42), LPARAM(0)).unwrap();
            finish_startup_feedback().unwrap();
            assert!(PeekMessageW(&mut message, None, WM_APP, WM_APP, PM_REMOVE).as_bool());
            assert_eq!(message.wParam.0, 42);
        })
        .join()
        .unwrap();
    }
}
