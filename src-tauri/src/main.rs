// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(all(not(debug_assertions), feature = "gui"), windows_subsystem = "windows")]

fn main() {
    api_switch_lib::run()
}
