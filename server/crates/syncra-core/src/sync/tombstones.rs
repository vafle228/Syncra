//! Чистка надгробий по сроку (S7.2, §5.4).
//!
//! Удалённая запись превращается в надгробие и **распространяется** наравне с
//! живой: не поехавшее удаление воскресает с другого устройства. Значит, стирать
//! надгробие насовсем можно только тогда, когда воскресать ему уже неоткуда.
//!
//! # Два условия, и оба обязательны
//!
//! **Прошёл срок.** §5.4 называет ~30 дней и обосновывает их так: в подавляющем
//! большинстве случаев устройства встречаются в одной сети не реже раза в сутки,
//! и тридцать суток покрывают риск с большим запасом. Срок жёсткий и
//! пользовательской настройки не имеет намеренно: это невидимый механизм, а
//! рычаг от невидимого механизма — способ молча испортить себе хранилище.
//!
//! **Надгробие заведомо разошлось.** Каждое действующее доверенное устройство
//! отчиталось, что довезло эту версию (`sync_state.synced_version >= version`).
//! Соседей нет вовсе — условие выполнено пустым множеством: воскрешать запись
//! некому. Отозванный сосед не считается — ему больше ничего не поедет ни в ту,
//! ни в другую сторону (§2.3).
//!
//! Условия соединены «и», а не «или», и порядок здесь не стилистический: срок
//! без доставки — это чистка надгробия, которого ноутбук в шкафу ещё не видел,
//! и первая же встреча с ним вернёт пароль, стёртый месяц назад. Обратная
//! сторона честная и названа вслух: **надгробие для устройства, которое не
//! появляется, не чистится никогда.** Это выбор в пользу человека — строка в БД
//! против воскресшего пароля, — и снимается он отзывом устройства, то есть тем
//! же действием, которым человек и так признаёт, что ноутбук не вернётся.
//!
//! # Чего здесь нет
//!
//! Секретов у надгробия нет уже месяц: их стирает `records::delete` в момент
//! удаления, а не эта чистка. Здесь исчезает последняя строка — `record_id`,
//! `deleted_at` и счётчик версий, — и вместе с ней, каскадом (`MIGRATION_5`),
//! отметки обмена и отложенный спор, если он был.

use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;

use crate::error::CoreResult;
use crate::model::IsoDateTime;

/// Сколько живёт надгробие (§5.4). Не настройка — см. шапку модуля.
pub const TOMBSTONE_TTL_DAYS: i64 = 30;

/// Стереть надгробия, которым больше [`TOMBSTONE_TTL_DAYS`] и которые уже
/// разошлись по всем действующим соседям. Возвращает, сколько строк исчезло.
///
/// Идемпотентна и обычно не находит ничего: за месяц человек удаляет запись-две.
pub fn collect(conn: &Connection, this_device_id: &str) -> CoreResult<usize> {
    collect_before(conn, this_device_id, &cutoff(Utc::now()))
}

/// Момент, старше которого надгробие считается отжившим.
fn cutoff(now: DateTime<Utc>) -> IsoDateTime {
    (now - Duration::days(TOMBSTONE_TTL_DAYS)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// То же самое, но с явной границей — так это проверяется тестами, которым
/// ждать тридцать дней нечем.
///
/// Сравнение дат **строковое**, и это не небрежность: `model::now_iso` пишет
/// один и тот же формат — UTC, миллисекунды, `Z`, — у которого лексикографический
/// порядок совпадает с хронологическим. Чужое `deleted_at` приезжает из того же
/// `now_iso` соседа, поэтому формат общий.
///
/// Разъехавшиеся часы соседа искажают срок, но не безопасность: ушедшие вперёд
/// задерживают чистку, отставшие делают надгробие отжившим раньше времени — и
/// там его всё равно держит второе условие, доставка.
fn collect_before(
    conn: &Connection,
    this_device_id: &str,
    cutoff: &IsoDateTime,
) -> CoreResult<usize> {
    let removed = conn.execute(
        "DELETE FROM records
          WHERE deleted_at IS NOT NULL
            AND deleted_at < ?1
            AND NOT EXISTS (
                  SELECT 1
                    FROM devices d
                    LEFT JOIN sync_state s
                           ON s.device_id = d.device_id
                          AND s.record_id = records.record_id
                   WHERE d.device_id <> ?2
                     AND d.revoked_at IS NULL
                     AND (s.synced_version IS NULL OR s.synced_version < records.version)
                )",
        rusqlite::params![cutoff, this_device_id],
    )?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::schema;

    const THIS: &str = "this-device";
    const LONG_AGO: &str = "2026-01-01T10:00:00.000Z";
    const YESTERDAY: &str = "2026-08-22T10:00:00.000Z";
    const NOW: &str = "2026-08-23T10:00:00.000Z";

    /// Хранилище со схемой, одной секцией и ничем больше.
    fn storage() -> Connection {
        let conn = Connection::open_in_memory().expect("хранилище в памяти");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        schema::migrate(&conn).expect("миграции");
        conn.execute_batch(
            "INSERT INTO vaults VALUES ('v', 'Личное', 'indigo', 1, 1, '2026-01-01T10:00:00.000Z');",
        )
        .unwrap();
        conn
    }

    /// Надгробие: запись с `deleted_at` и без единого шифротекста — ровно то,
    /// что оставляет после себя `records::delete`.
    fn tombstone(conn: &Connection, record_id: &str, version: i64, deleted_at: &str) {
        conn.execute(
            "INSERT INTO records (record_id, vault_id, service_name, urls, login, version,
                                  created_at, updated_at, password_updated_at, deleted_at)
             VALUES (?1, 'v', 'GitHub', '[]', 'octocat', ?2, ?3, ?3, ?3, ?3)",
            rusqlite::params![record_id, version, deleted_at],
        )
        .unwrap();
    }

    fn live(conn: &Connection, record_id: &str) {
        conn.execute(
            "INSERT INTO records (record_id, vault_id, service_name, urls, login, version,
                                  created_at, updated_at, password_updated_at)
             VALUES (?1, 'v', 'GitHub', '[]', 'octocat', 1, ?2, ?2, ?2)",
            rusqlite::params![record_id, LONG_AGO],
        )
        .unwrap();
    }

    fn peer(conn: &Connection, device_id: &str, revoked_at: Option<&str>) {
        conn.execute(
            "INSERT INTO devices (device_id, name, kind, public_key, fingerprint_words,
                                  paired_at, revoked_at)
             VALUES (?1, 'Ноутбук', 'desktop', x'00', '[]', ?2, ?3)",
            rusqlite::params![device_id, LONG_AGO, revoked_at],
        )
        .unwrap();
    }

    fn delivered(conn: &Connection, device_id: &str, record_id: &str, version: i64) {
        conn.execute(
            "INSERT INTO sync_state VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![device_id, record_id, version, NOW],
        )
        .unwrap();
    }

    fn left(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM records", [], |row| row.get(0))
            .unwrap()
    }

    /// Чистка «сегодня»: граница считается из [`NOW`] тем же правилом, что и в
    /// жизни, — иначе тест проверял бы не срок, а произвольное число.
    fn sweep(conn: &Connection) -> usize {
        let now: DateTime<Utc> = NOW.parse().expect("момент");
        collect_before(conn, THIS, &cutoff(now)).expect("чистка")
    }

    #[test]
    fn a_tombstone_nobody_can_resurrect_goes_after_its_term() {
        let conn = storage();
        tombstone(&conn, "r", 2, LONG_AGO);

        assert_eq!(sweep(&conn), 1);
        assert_eq!(left(&conn), 0);
    }

    #[test]
    fn a_fresh_tombstone_stays_even_with_nobody_around() {
        let conn = storage();
        tombstone(&conn, "r", 2, YESTERDAY);

        assert_eq!(sweep(&conn), 0, "надгробие вчерашнего дня уже стёрли");
        assert_eq!(left(&conn), 1);
    }

    #[test]
    fn a_live_record_is_never_touched() {
        let conn = storage();
        live(&conn, "alive");
        tombstone(&conn, "gone", 2, LONG_AGO);

        assert_eq!(sweep(&conn), 1);
        assert_eq!(left(&conn), 1);
    }

    #[test]
    fn a_tombstone_the_laptop_has_not_seen_waits_for_it() {
        let conn = storage();
        tombstone(&conn, "r", 2, LONG_AGO);
        peer(&conn, "laptop", None);

        // Ноутбук в шкафу: отметки нет вовсе.
        assert_eq!(sweep(&conn), 0, "удаление стёрли раньше, чем оно доехало");

        // Доехала версия постарше — этого мало: воскреснет ровно то удаление,
        // которого ноутбук ещё не видел.
        delivered(&conn, "laptop", "r", 1);
        assert_eq!(sweep(&conn), 0);

        conn.execute(
            "UPDATE sync_state SET synced_version = 2 WHERE device_id = 'laptop'",
            [],
        )
        .unwrap();
        assert_eq!(sweep(&conn), 1, "довезённое удаление так и не убрали");
    }

    #[test]
    fn one_silent_peer_holds_the_tombstone_for_everyone() {
        let conn = storage();
        tombstone(&conn, "r", 2, LONG_AGO);
        peer(&conn, "phone", None);
        peer(&conn, "laptop", None);
        delivered(&conn, "phone", "r", 2);

        assert_eq!(sweep(&conn), 0, "хватило одного отчитавшегося соседа");
    }

    #[test]
    fn a_revoked_peer_holds_nothing() {
        let conn = storage();
        tombstone(&conn, "r", 2, LONG_AGO);
        peer(&conn, "laptop", Some(NOW));

        // Отозванному больше ничего не поедет — ни туда, ни оттуда (§2.3),
        // и ждать его отчёта значило бы ждать вечно.
        assert_eq!(sweep(&conn), 1);
    }

    #[test]
    fn this_device_does_not_wait_for_itself() {
        let conn = storage();
        tombstone(&conn, "r", 2, LONG_AGO);
        peer(&conn, THIS, None);

        // Своей строки в `sync_state` не бывает и быть не должно: сам себе
        // ничего не возят. Считать её отсутствие «не доставлено» значило бы
        // не чистить надгробия никогда.
        assert_eq!(sweep(&conn), 1);
    }

    #[test]
    fn a_swept_tombstone_takes_its_marks_and_its_conflict_with_it() {
        let conn = storage();
        tombstone(&conn, "r", 2, LONG_AGO);
        peer(&conn, "laptop", None);
        delivered(&conn, "laptop", "r", 2);
        conn.execute(
            "INSERT INTO conflicts (record_id, device_id, raised_at, local_version, version,
                                    vault_id, service_name, urls, login, updated_at,
                                    password_updated_at)
             VALUES ('r', 'laptop', ?1, 2, 3, 'v', 'GitHub', '[]', 'octocat', ?1, ?1)",
            [NOW],
        )
        .unwrap();

        assert_eq!(sweep(&conn), 1);
        let orphans: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM sync_state) + (SELECT COUNT(*) FROM conflicts)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(orphans, 0, "ссылки на стёртое надгробие остались сиротами");
    }

    #[test]
    fn the_cutoff_is_exactly_thirty_days_back() {
        let now: DateTime<Utc> = "2026-08-23T10:00:00Z".parse().unwrap();
        assert_eq!(cutoff(now), "2026-07-24T10:00:00.000Z");
    }
}
