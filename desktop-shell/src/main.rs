#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    if let Err(error) = rundock_desktop::run() {
        rundock_desktop::show_fatal_error(&error.to_string());
        std::process::exit(1);
    }
}
