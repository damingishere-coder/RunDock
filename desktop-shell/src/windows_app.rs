use crate::{classify_navigation, LaunchMode, NavigationDecision, DASHBOARD_URL};
use alter::daemon::lifecycle::ensure_daemon;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::{NewWindowResponse, WebviewWindowBuilder};
use tauri::{AppHandle, Manager, WebviewUrl, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

const HOST: &str = "127.0.0.1";
const PORT: u16 = 2999;
const AUTOSTART_MARKER: &str = "desktop-shell-autostart.json";
const SKIP_AUTOSTART_ENV: &str = "RUNDOCK_SKIP_AUTOSTART_INIT";

struct LaunchState {
    in_progress: AtomicBool,
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
            api.prevent_close();
            let _ = close_window.hide();
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
    let quit = MenuItem::with_id(app, "quit", "退出 RunDock", true, None::<&str>)?;
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
            "quit" => app.exit(0),
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
            app.exit(0);
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
        })
        .setup(move |app| {
            if mode == LaunchMode::QuitExisting {
                app.handle().exit(0);
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
