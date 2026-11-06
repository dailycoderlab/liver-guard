// Keep the console window away in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    liverguard_desktop_lib::run()
}
