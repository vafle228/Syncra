//! Оболочка Syncra на Tauri 2.
//!
//! Держит окно и мост IPC — и больше ничего. Крипта, хранилище и правила живут
//! в `syncra-core`, который про Tauri не знает (§8.2).

mod commands;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use commands::{CoreState, NodeState};
use syncra_core::{Core, HostDevice, Node, NodeSettings};
use tauri::Manager;

/// Файл хранилища. Лежит в папке приложения — путь выбирает оболочка, потому что
/// только она знает про платформу; ядру он приходит готовым.
const DB_FILE_NAME: &str = "syncra.db";

/// Как часто сторож спрашивает ядро, не пора ли запереть хранилище.
///
/// Секунда — не точность автоблокировки, а её зернистость: сроки в настройках
/// начинаются с минуты, и опоздать на секунду не страшно. Чаще спрашивать значит
/// будить процесс без нужды, реже — заметно врать про выбранный срок.
///
/// Тот же сторож поднимает и опускает сеть (S3): отпертому хранилищу она нужна,
/// запертому — нет. Вызвать это прямо из `unlock` нельзя, там уже взят мьютекс
/// ядра, а сети он нужен, чтобы прочитать ключи.
const AUTOLOCK_TICK: Duration = Duration::from_secs(1);

/// Как это устройство подпишется в списке доверенных (F9).
///
/// Имя хоста — платформенный факт, и берёт его оболочка: ядро про ОС не знает и
/// знать не должно (§8.2). Тип фиксирован десктопом — MVP 1 других платформ не
/// собирает (§9), а мобильная ветка приедет вместе с iOS-клиентом.
fn host_device() -> HostDevice {
    HostDevice::desktop(&gethostname::gethostname().to_string_lossy())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let path = app.path().app_data_dir()?.join(DB_FILE_NAME);
            let core = Arc::new(Mutex::new(Core::open(&path, host_device())?));
            app.manage(CoreState(Arc::clone(&core)));

            // Сеть живёт в своих потоках и трогает ядро короткими операциями:
            // сетевое ожидание под мьютексом ядра заморозило бы весь UI, включая
            // кнопку «Запереть» (§5.6).
            let (node, events) = Node::new(core, NodeSettings::default());
            let node = Arc::new(node);
            app.manage(NodeState(Arc::clone(&node)));

            // Автоблокировку исполняет ядро (§8.2), но кто-то должен его будить:
            // отдельный поток, а не таймер во фронте, потому что окно может быть
            // свёрнуто, а вкладка — выгружена, хранилище же должно закрыться всё равно.
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(AUTOLOCK_TICK);
                commands::autolock_tick(&handle);
                node.tick();
            });

            // События ядра — сюда. Ядро складывает их в канал и про Tauri не
            // знает; имя события на проводе выбирает оболочка.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                for event in events {
                    commands::forward(&handle, event);
                }
            });

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
            // Генератор паролей (F6)
            commands::get_generator_profile,
            commands::save_generator_profile,
            commands::generate_passwords,
            // Сопряжение (F8)
            commands::get_pairing_payload,
            commands::submit_paired_key,
            commands::confirm_pairing,
            commands::cancel_pairing,
            // Доверенные устройства (F9)
            commands::list_devices,
            commands::revoke_device,
            // Безопасность и вход (F13)
            commands::unlock_with_pin,
            commands::change_master_password,
            commands::get_security_settings,
            commands::save_security_settings,
            // Синхронизация (F10): обнаружение есть, обмена записями ещё нет
            commands::get_sync_status,
            commands::sync_now,
            commands::list_conflicts,
            // Следующие шаги — зарегистрированы ради внятного отказа
            commands::get_totp_code,
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
