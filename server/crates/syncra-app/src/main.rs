// Прод-сборка не поднимает консольное окно рядом с приложением.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    syncra_app_lib::run()
}
