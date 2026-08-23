//! Чистка надгробий по сроку (S7.2, §5.4).
//!
//! Проверяется не SQL — его держат тесты внутри `sync::tombstones`, — а то, что
//! чистка **включена**: сама собой, без команды, в те два момента, когда про
//! надгробие есть что нового узнать.
//!
//! Хранилище здесь файловое, а не в памяти: тесту нужно состарить надгробие, а
//! ждать тридцать дней нечем. Часы подкручиваются вторым соединением к тому же
//! файлу — так же, как это делает `at_rest.rs`, когда лезет в БД посторонним
//! инструментом.

mod common;

use std::path::Path;

use common::{draft, MASTER_PASSWORD};
use syncra_core::{Core, RecordMeta};

/// Заведомо старше тридцати дней от «сегодня» любого запуска: ядро сравнивает
/// `deleted_at` с моментом «месяц назад», и тест обязан пережить календарь.
const LONG_AGO: &str = "2000-01-01T10:00:00.000Z";

struct Vault {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    core: Core,
}

impl Vault {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("временная папка");
        let path = dir.path().join("syncra.db");
        let mut core = Core::open(&path, common::host()).expect("хранилище");
        core.init_vault(MASTER_PASSWORD).expect("init_vault");
        Self {
            _dir: dir,
            path,
            core,
        }
    }

    fn records(&self) -> Vec<RecordMeta> {
        self.core.list_records(None, true).expect("список записей")
    }

    fn has(&self, record_id: &str) -> bool {
        self.records()
            .iter()
            .any(|record| record.record_id == record_id)
    }

    /// Запереть и отпереть заново — то есть пройти тем же путём, каким утром
    /// проходит человек.
    fn relock(&mut self) {
        self.core.lock();
        self.core.unlock(MASTER_PASSWORD).expect("unlock");
    }

    fn aside<T>(&self, action: impl FnOnce(&rusqlite::Connection) -> T) -> T {
        aside(&self.path, action)
    }
}

/// Посторонний взгляд в тот же файл: подкрутить часы, подсадить соседа,
/// отметить доставку. Ядру таких ручек нет и не будет.
fn aside<T>(path: &Path, action: impl FnOnce(&rusqlite::Connection) -> T) -> T {
    let conn = rusqlite::Connection::open(path).expect("второе соединение");
    action(&conn)
}

fn age(conn: &rusqlite::Connection, record_id: &str) {
    let changed = conn
        .execute(
            "UPDATE records SET deleted_at = ?2 WHERE record_id = ?1 AND deleted_at IS NOT NULL",
            rusqlite::params![record_id, LONG_AGO],
        )
        .expect("состаривание");
    assert_eq!(changed, 1, "состарить нечего: это не надгробие");
}

fn add_peer(conn: &rusqlite::Connection, device_id: &str) {
    conn.execute(
        "INSERT INTO devices (device_id, name, kind, public_key, fingerprint_words, paired_at)
         VALUES (?1, 'Ноутбук', 'desktop', x'00', '[]', ?2)",
        rusqlite::params![device_id, LONG_AGO],
    )
    .expect("сосед");
}

fn mark_delivered(conn: &rusqlite::Connection, device_id: &str, record: &RecordMeta) {
    conn.execute(
        "INSERT INTO sync_state VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![device_id, record.record_id, record.version, LONG_AGO],
    )
    .expect("отметка обмена");
}

#[test]
fn an_expired_tombstone_is_gone_after_the_next_unlock() {
    let mut vault = Vault::new();
    let kept = vault
        .core
        .create_record(&draft("GitHub", "octocat", "пароль"))
        .unwrap();
    let doomed = vault
        .core
        .create_record(&draft("Twitter", "octocat", "пароль"))
        .unwrap();
    vault.core.delete_record(&doomed.record_id).unwrap();

    // До срока надгробие никуда не девается — иначе удаление не успело бы
    // доехать ни до одного соседа.
    vault.relock();
    assert!(vault.has(&doomed.record_id), "свежее надгробие уже стёрли");

    vault.aside(|conn| age(conn, &doomed.record_id));
    vault.relock();

    assert!(!vault.has(&doomed.record_id), "отжившее надгробие осталось");
    assert!(vault.has(&kept.record_id), "живую запись задело чисткой");
}

#[test]
fn a_tombstone_the_laptop_has_not_seen_survives_every_unlock() {
    let mut vault = Vault::new();
    let doomed = vault
        .core
        .create_record(&draft("Twitter", "octocat", "пароль"))
        .unwrap();
    let tombstone = vault.core.delete_record(&doomed.record_id).unwrap();

    vault.aside(|conn| {
        age(conn, &doomed.record_id);
        // Ноутбук сопряжён, но в шкафу: удаление до него не доехало.
        add_peer(conn, "laptop");
    });

    vault.relock();
    assert!(
        vault.has(&doomed.record_id),
        "удаление стёрли раньше, чем оно доехало до ноутбука"
    );

    // Ноутбук вернулся и отчитался — держать надгробие больше не для кого.
    vault.aside(|conn| mark_delivered(conn, "laptop", &tombstone));
    vault.relock();
    assert!(
        !vault.has(&doomed.record_id),
        "довезённое удаление так и не убрали"
    );
}

#[test]
fn a_finished_sync_round_sweeps_what_it_just_delivered() {
    // Второй момент чистки: круг обмена только что закрыл отметки, и про
    // надгробие впервые стало известно, что оно разошлось. Ждать до утреннего
    // отпирания незачем.
    let vault = Vault::new();
    let doomed = vault
        .core
        .create_record(&draft("Twitter", "octocat", "пароль"))
        .unwrap();
    let tombstone = vault.core.delete_record(&doomed.record_id).unwrap();

    vault.aside(|conn| {
        age(conn, &doomed.record_id);
        add_peer(conn, "laptop");
        mark_delivered(conn, "laptop", &tombstone);
    });

    assert!(
        vault.has(&doomed.record_id),
        "чистка сработала раньше круга"
    );
    vault.core.sync_finish().expect("круг закончился");
    assert!(
        !vault.has(&doomed.record_id),
        "круг закончился, а надгробие осталось"
    );
}

#[test]
fn a_locked_vault_is_never_swept() {
    // Чистка ходит в ту же БД, что и всё остальное, но заперта вместе с ней:
    // за замком ядро не делает ничего, включая уборку.
    let mut vault = Vault::new();
    let doomed = vault
        .core
        .create_record(&draft("Twitter", "octocat", "пароль"))
        .unwrap();
    vault.core.delete_record(&doomed.record_id).unwrap();
    vault.aside(|conn| age(conn, &doomed.record_id));

    vault.core.lock();
    assert!(
        vault.core.sync_finish().is_err(),
        "запертое хранилище закончило круг обмена"
    );
    let left: i64 = vault.aside(|conn| {
        conn.query_row("SELECT COUNT(*) FROM records", [], |row| row.get(0))
            .unwrap()
    });
    assert_eq!(left, 1, "запертое хранилище всё-таки подмели");
}
