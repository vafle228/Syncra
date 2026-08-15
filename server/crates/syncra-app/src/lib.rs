//! Оболочка Syncra на Tauri 2.
//!
//! Держит окно и мост IPC — и больше ничего. Крипта, хранилище и правила живут
//! в `syncra-core`, который про Tauri не знает (§8.2).

mod commands;

use std::sync::Mutex;

use commands::CoreState;
use syncra_core::Core;
use tauri::Manager;

/// Файл хранилища. Лежит в папке приложения — путь выбирает оболочка, потому что
/// только она знает про платформу; ядру он приходит готовым.
const DB_FILE_NAME: &str = "syncra.db";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let path = app.path().app_data_dir()?.join(DB_FILE_NAME);
            let core = Core::open(&path)?;
            app.manage(CoreState(Mutex::new(core)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Жизненный цикл хранилища (F3)
            commands::get_vault_status,
            commands::init_vault,
            commands::unlock,
            commands::lock,
            // Записи (F4, F5)
            commands::list_records,
            commands::get_secret,
            commands::create_record,
            commands::update_record,
            commands::delete_record,
            // Секции (F7)
            commands::list_vaults,
            commands::create_vault,
            commands::update_vault,
            commands::set_vault_sync,
            commands::set_default_vault,
            commands::delete_vault,
            // Синхронизация: её пока нет, и ядро говорит об этом прямо
            commands::get_sync_status,
            commands::sync_now,
            commands::list_conflicts,
            // Следующие шаги — зарегистрированы ради внятного отказа
            commands::unlock_with_pin,
            commands::change_master_password,
            commands::get_security_settings,
            commands::save_security_settings,
            commands::get_generator_profile,
            commands::save_generator_profile,
            commands::generate_passwords,
            commands::get_totp_code,
            commands::get_pairing_payload,
            commands::submit_paired_key,
            commands::confirm_pairing,
            commands::cancel_pairing,
            commands::list_devices,
            commands::revoke_device,
            commands::resolve_conflict,
            commands::get_conflict_secret,
            commands::export_csv,
            commands::export_backup,
            commands::delete_export,
            commands::restore_backup,
            commands::begin_import,
            commands::commit_import,
            commands::cancel_import,
        ])
        .run(tauri::generate_context!())
        .expect("не удалось запустить Syncra");
}
