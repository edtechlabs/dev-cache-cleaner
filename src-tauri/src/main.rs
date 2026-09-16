#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    macos_dev_cache_cleaner_lib::run();
}
