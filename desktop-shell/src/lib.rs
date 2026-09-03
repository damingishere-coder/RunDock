use url::Url;

pub const DASHBOARD_URL: &str = "http://127.0.0.1:2999/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Foreground,
    Background,
    QuitExisting,
}

impl LaunchMode {
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Self {
        let mut background = false;
        for argument in args {
            match argument.as_str() {
                "--quit" => return Self::QuitExisting,
                "--background" => background = true,
                _ => {}
            }
        }
        if background {
            Self::Background
        } else {
            Self::Foreground
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationDecision {
    AllowInternal,
    OpenExternal,
    ShellAction,
    Deny,
}

pub fn classify_navigation(url: &Url) -> NavigationDecision {
    if url.scheme() == "rundock-shell" {
        return NavigationDecision::ShellAction;
    }
    if (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (matches!(url.scheme(), "http" | "https") && url.host_str() == Some("tauri.localhost"))
    {
        return NavigationDecision::AllowInternal;
    }
    if url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port_or_known_default() == Some(2999)
        && url.username().is_empty()
        && url.password().is_none()
    {
        return NavigationDecision::AllowInternal;
    }
    if matches!(url.scheme(), "http" | "https") {
        return NavigationDecision::OpenExternal;
    }
    if matches!(
        url.scheme(),
        "file" | "javascript" | "data" | "about" | "blob" | "devtools"
    ) {
        return NavigationDecision::Deny;
    }
    if url.scheme().len() >= 2
        && url.scheme().chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
    {
        return NavigationDecision::OpenExternal;
    }
    NavigationDecision::Deny
}

#[cfg(windows)]
mod windows_app;

#[cfg(windows)]
pub use windows_app::run;

#[cfg(windows)]
pub fn show_fatal_error(detail: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let message = format!(
        "RunDock 桌面窗口无法启动。请修复或重新安装 RunDock；如果系统缺少组件，请安装 Microsoft Edge WebView2 Runtime。\n\n详细信息：{detail}"
    );
    unsafe {
        let _ = MessageBoxW(
            None,
            &HSTRING::from(message),
            &HSTRING::from("RunDock"),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    Err("RunDock desktop shell is supported on Windows only".into())
}

#[cfg(not(windows))]
pub fn show_fatal_error(detail: &str) {
    eprintln!("RunDock: {detail}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_mode_is_explicit_and_quit_wins() {
        assert_eq!(LaunchMode::from_args(Vec::new()), LaunchMode::Foreground);
        assert_eq!(
            LaunchMode::from_args(vec!["--background".to_string()]),
            LaunchMode::Background
        );
        assert_eq!(
            LaunchMode::from_args(vec!["--background".to_string(), "--quit".to_string()]),
            LaunchMode::QuitExisting
        );
    }

    #[test]
    fn only_the_canonical_dashboard_is_embedded() {
        assert_eq!(
            classify_navigation(&Url::parse("http://tauri.localhost/index.html").unwrap()),
            NavigationDecision::AllowInternal
        );
        assert_eq!(
            classify_navigation(&Url::parse("http://127.0.0.1:2999/processes").unwrap()),
            NavigationDecision::AllowInternal
        );
        assert_eq!(
            classify_navigation(&Url::parse("http://localhost:2999/").unwrap()),
            NavigationDecision::OpenExternal
        );
        assert_eq!(
            classify_navigation(&Url::parse("http://127.0.0.1:5173/").unwrap()),
            NavigationDecision::OpenExternal
        );
        assert_eq!(
            classify_navigation(
                &Url::parse("https://github.com/damingishere-coder/RunDock").unwrap()
            ),
            NavigationDecision::OpenExternal
        );
        assert_eq!(
            classify_navigation(&Url::parse("wanmotai://open").unwrap()),
            NavigationDecision::OpenExternal
        );
        assert_eq!(
            classify_navigation(&Url::parse("file:///C:/Windows/System32/cmd.exe").unwrap()),
            NavigationDecision::Deny
        );
        assert_eq!(
            classify_navigation(&Url::parse("rundock-shell://retry").unwrap()),
            NavigationDecision::ShellAction
        );
    }
}
