use crate::{classify_navigation, LaunchMode, NavigationDecision, DASHBOARD_URL};
use alter::daemon::lifecycle::ensure_daemon;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::{NewWindowResponse, WebviewWindowBuilder};
use tauri::{AppHandle, Manager, WebviewUrl, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

const HOST: &str = "127.0.0.1";
const PORT: u16 = 2999;
const AUTOSTART_MARKER: &str = "desktop-shell-autostart.json";
const TRAY_NOTICE_MARKER: &str = "desktop-shell-tray-notice.json";
const SKIP_AUTOSTART_ENV: &str = "RUNDOCK_SKIP_AUTOSTART_INIT";
const TRAY_NOTICE_MESSAGE: &str = "RunDock 已缩小到系统托盘。左键托盘图标可重新打开；只有在托盘菜单选择“退出 RunDock”时才会关闭桌面端。后台项目会继续运行。";

struct LaunchState {
    in_progress: AtomicBool,
    quitting: AtomicBool,
    tray_notice_shown: AtomicBool,
}

fn should_intercept_close(quitting: bool) -> bool {
    !quitting
}

fn show_native_message(title: &'static str, message: String, error: bool) {
    std::thread::spawn(move || {
        use windows::core::HSTRING;
        use windows::Win32::UI::WindowsAndMessaging::{
            MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND,
        };

        let style = MB_OK
            | MB_SETFOREGROUND
            | if error {
                MB_ICONERROR
            } else {
                MB_ICONINFORMATION
            };
        unsafe {
            let _ = MessageBoxW(None, &HSTRING::from(message), &HSTRING::from(title), style);
        }
    });
}

fn claim_tray_notice(shown: &AtomicBool, marker: &Path) -> bool {
    if shown.swap(true, Ordering::SeqCst) || marker.exists() {
        return false;
    }
    if let Err(error) =
        alter::config::atomic_file::write_with_backup(marker, br#"{"shown":true}"#, None)
    {
        eprintln!(
            "RunDock could not persist the tray notice marker {}: {error}",
            marker.display()
        );
    }
    true
}

fn show_tray_notice_once(app: &AppHandle) {
    let state = app.state::<LaunchState>();
    let marker = alter::config::paths::data_dir().join(TRAY_NOTICE_MARKER);
    if claim_tray_notice(&state.tray_notice_shown, &marker) {
        show_native_message("RunDock", TRAY_NOTICE_MESSAGE.to_string(), false);
    }
}

fn request_exit(app: &AppHandle) {
    app.state::<LaunchState>()
        .quitting
        .store(true, Ordering::SeqCst);
    app.exit(0);
}

fn open_with_windows(target: &str) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let _ = std::process::Command::new("explorer.exe")
        .arg(target)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

fn sibling_daemon_exe() -> Result<PathBuf, String> {
    if let Some(override_path) = std::env::var_os("RUNDOCK_DAEMON_EXE") {
        let path = PathBuf::from(override_path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "RUNDOCK_DAEMON_EXE 指向的文件不存在：{}",
            path.display()
        ));
    }
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    let path = current
        .parent()
        .ok_or_else(|| "无法确定 RunDock 安装目录".to_string())?
        .join("alter.exe");
    if path.is_file() {
        Ok(path)
    } else {
        Err("未找到 RunDock 后台程序。请重新安装 RunDock。".to_string())
    }
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn render_launching(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.eval("window.__setLaunching && window.__setLaunching()");
    }
}

fn render_failure(app: &AppHandle, message: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let logs = alter::config::paths::daemon_log_file();
        let diagnostic = format!("{message}\n\n日志：{}", logs.display());
        if let Ok(serialized) = serde_json::to_string(&diagnostic) {
            let _ = window.eval(format!(
                "window.__setLaunchError && window.__setLaunchError({serialized})"
            ));
        }
    }
}

fn launch_dashboard(app: AppHandle) {
    let state = app.state::<LaunchState>();
    if state.in_progress.swap(true, Ordering::SeqCst) {
        return;
    }
    render_launching(&app);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let result = match sibling_daemon_exe() {
            Ok(daemon_exe) => ensure_daemon(&daemon_exe, HOST, PORT).await.map(|_| ()),
            Err(error) => Err(anyhow::anyhow!(error)),
        };
        app.state::<LaunchState>()
            .in_progress
            .store(false, Ordering::SeqCst);
        match result {
            Ok(()) => {
                if let (Some(window), Ok(url)) =
                    (app.get_webview_window("main"), DASHBOARD_URL.parse())
                {
                    let _ = window.navigate(url);
                }
            }
            Err(error) => render_failure(&app, &error.to_string()),
        }
    });
}

fn handle_shell_action(app: &AppHandle, action: Option<&str>) {
    match action {
        Some("retry") => launch_dashboard(app.clone()),
        Some("open-logs") => open_with_windows(&alter::config::paths::data_dir().to_string_lossy()),
        Some("open-browser") => open_with_windows(DASHBOARD_URL),
        _ => {}
    }
}

fn initialize_autostart(app: &AppHandle) {
    if std::env::var_os(SKIP_AUTOSTART_ENV).is_some() {
        return;
    }
    let marker = alter::config::paths::data_dir().join(AUTOSTART_MARKER);
    if marker.exists() {
        return;
    }
    if app.autolaunch().enable().is_ok() {
        let _ = alter::config::atomic_file::write_with_backup(
            &marker,
            br#"{"initialized":true}"#,
            None,
        );
    }
}

fn create_main_window(app: &tauri::App) -> tauri::Result<()> {
    let navigation_app = app.handle().clone();
    let new_window_app = app.handle().clone();
    let close_app = app.handle().clone();
    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("RunDock")
        .inner_size(1280.0, 820.0)
        .min_inner_size(960.0, 640.0)
        .visible(false)
        .on_navigation(move |url| match classify_navigation(url) {
            NavigationDecision::AllowInternal => true,
            NavigationDecision::OpenExternal => {
                open_with_windows(url.as_str());
                false
            }
            NavigationDecision::ShellAction => {
                handle_shell_action(&navigation_app, url.host_str());
                false
            }
            NavigationDecision::Deny => false,
        })
        .on_new_window(move |url, _| {
            match classify_navigation(&url) {
                NavigationDecision::OpenExternal => open_with_windows(url.as_str()),
                NavigationDecision::ShellAction => {
                    handle_shell_action(&new_window_app, url.host_str())
                }
                NavigationDecision::AllowInternal | NavigationDecision::Deny => {}
            }
            NewWindowResponse::Deny
        })
        .build()?;
    let close_window = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            if !should_intercept_close(
                close_app
                    .state::<LaunchState>()
                    .quitting
                    .load(Ordering::SeqCst),
            ) {
                return;
            }
            api.prevent_close();
            match close_window.hide() {
                Ok(()) => show_tray_notice_once(&close_app),
                Err(error) => {
                    let _ = close_window.show();
                    let _ = close_window.set_focus();
                    show_native_message(
                        "RunDock 无法缩小到托盘",
                        format!("窗口仍保持打开。请稍后重试。\n\n详细信息：{error}"),
                        true,
                    );
                }
            }
        }
    });
    Ok(())
}

fn create_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开 RunDock", true, None::<&str>)?;
    let browser = MenuItem::with_id(app, "browser", "在浏览器中打开", true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "登录 Windows 时自动启动",
        true,
        app.autolaunch().is_enabled().unwrap_or(false),
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        "退出 RunDock（项目继续运行）",
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&open, &browser, &autostart, &separator, &quit])?;
    let autostart_item = autostart.clone();
    TrayIconBuilder::with_id("rundock-tray")
        .icon(tauri::image::Image::from_bytes(include_bytes!(
            "../../assets/rundock-icon-1024.png"
        ))?)
        .tooltip("RunDock")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_window(tray.app_handle());
            }
        })
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show_window(app),
            "browser" => open_with_windows(DASHBOARD_URL),
            "autostart" => {
                let manager = app.autolaunch();
                let enabled = manager.is_enabled().unwrap_or(false);
                let changed = if enabled {
                    manager.disable().is_ok()
                } else {
                    manager.enable().is_ok()
                };
                if changed {
                    let _ = autostart_item.set_checked(!enabled);
                }
            }
            "quit" => request_exit(app),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mode = LaunchMode::from_args(std::env::args().skip(1));
    let mut builder = tauri::Builder::default();
    builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _| {
        let duplicate_mode = LaunchMode::from_args(args);
        if duplicate_mode == LaunchMode::QuitExisting {
            request_exit(app);
        } else {
            show_window(app);
        }
    }));
    builder = builder.plugin(tauri_plugin_autostart::init(
        MacosLauncher::LaunchAgent,
        Some(vec!["--background"]),
    ));
    builder
        .manage(LaunchState {
            in_progress: AtomicBool::new(false),
            quitting: AtomicBool::new(false),
            tray_notice_shown: AtomicBool::new(false),
        })
        .setup(move |app| {
            if mode == LaunchMode::QuitExisting {
                request_exit(app.handle());
                return Ok(());
            }
            initialize_autostart(app.handle());
            create_main_window(app)?;
            create_tray(app)?;
            if mode == LaunchMode::Foreground {
                show_window(app.handle());
            }
            launch_dashboard(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn close_is_intercepted_until_an_explicit_quit() {
        assert!(should_intercept_close(false));
        assert!(!should_intercept_close(true));
    }

    #[test]
    fn tray_notice_is_claimed_once_and_persists_across_processes() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "rundock-tray-notice-{}-{unique}",
            std::process::id()
        ));
        let marker = directory.join(TRAY_NOTICE_MARKER);
        let shown = AtomicBool::new(false);

        assert!(claim_tray_notice(&shown, &marker));
        assert!(marker.is_file());
        assert!(!claim_tray_notice(&shown, &marker));

        let next_process = AtomicBool::new(false);
        assert!(!claim_tray_notice(&next_process, &marker));

        std::fs::remove_dir_all(directory).expect("temporary marker directory should be removable");
    }

    #[test]
    fn tray_notice_stays_once_per_process_when_persistence_fails() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let parent_file = std::env::temp_dir().join(format!(
            "rundock-tray-notice-parent-{}-{unique}",
            std::process::id()
        ));
        std::fs::write(&parent_file, b"not a directory")
            .expect("temporary parent file should be writable");
        let marker = parent_file.join(TRAY_NOTICE_MARKER);
        let shown = AtomicBool::new(false);

        assert!(claim_tray_notice(&shown, &marker));
        assert!(!marker.exists());
        assert!(!claim_tray_notice(&shown, &marker));

        std::fs::remove_file(parent_file).expect("temporary parent file should be removable");
    }
}
