// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Escape hatch: `ledge --restore-taskbar` puts the Windows taskbar back and exits.
    if std::env::args().any(|a| a == "--restore-taskbar") {
        ledge_lib::restore_taskbar_and_exit();
    }
    ledge_lib::run()
}
