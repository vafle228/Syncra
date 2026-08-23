//! Схема хранилища и её миграции (§8.3 «Storage»).

use rusqlite::{Connection, OptionalExtension};

use crate::crypto::VaultKey;
use crate::error::{CoreError, CoreResult};
use crate::model::RecordId;
use crate::storage::metadata;

/// Версия схемы. Растёт вместе с миграциями; лежит в `meta`.
pub const SCHEMA_VERSION: i64 = 6;

/// Ключи таблицы `meta`.
pub const META_SCHEMA_VERSION: &str = "schema_version";
pub const META_KDF_SALT: &str = "kdf_salt";
pub const META_KDF_PARAMS: &str = "kdf_params";
pub const META_VERIFIER: &str = "verifier";
pub const META_DEVICE_ID: &str = "device_id";
/// Приватная половина пары ключей устройства (§2.1) — запечатанная ключом
/// хранилища. Публичная лежит открыто, в своей строке таблицы `devices`.
pub const META_DEVICE_SECRET: &str = "device_secret";
pub const META_CREATED_AT: &str = "created_at";
/// Профиль генератора и настройки безопасности — JSON, открытым текстом.
///
/// Это настройки, а не секреты: в профиле нет ни одного пароля, а таймауты и так
/// видны по поведению окна. Отдельной таблицы они не заводят — по строке в `meta`
/// хватает, и старое хранилище без этих строк открывается умолчаниями, а не
/// требует миграции.
pub const META_GENERATOR_PROFILE: &str = "generator_profile";
pub const META_SECURITY_SETTINGS: &str = "security_settings";
/// Когда последний обмен с соседом успешно закончился (§5.3). Одна строка на
/// хранилище, а не на устройство: индикатор спрашивает «когда синхронизировались
/// в последний раз», а не «когда — с ноутбуком».
pub const META_LAST_SYNC_AT: &str = "last_sync_at";

const MIGRATION_1: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS vaults (
  vault_id   TEXT PRIMARY KEY,
  name       TEXT    NOT NULL,
  color      TEXT    NOT NULL,
  sync       INTEGER NOT NULL,
  is_default INTEGER NOT NULL,
  created_at TEXT    NOT NULL
);

-- «Ровно одна секция по умолчанию» — инвариант уровня БД, а не соглашение в
-- коде: `create_record` без `vault_id` иначе было бы некуда положить.
CREATE UNIQUE INDEX IF NOT EXISTS vaults_one_default
  ON vaults(is_default) WHERE is_default = 1;

CREATE TABLE IF NOT EXISTS records (
  record_id           TEXT PRIMARY KEY,
  vault_id            TEXT    NOT NULL REFERENCES vaults(vault_id),
  service_name        TEXT    NOT NULL,
  -- JSON-массив: список доменов, а не одно поле (§4.1).
  urls                TEXT    NOT NULL,
  login               TEXT    NOT NULL,
  account_label       TEXT,
  -- Шифротексты полей. NULL у надгробия и у незаполненных секретов; отсюда же
  -- берутся `has_notes` / `has_totp` — не расшифровывая ничего.
  password_ct         BLOB,
  notes_ct            BLOB,
  totp_ct             BLOB,
  version             INTEGER NOT NULL,
  created_at          TEXT    NOT NULL,
  updated_at          TEXT    NOT NULL,
  password_updated_at TEXT    NOT NULL,
  -- Надгробие (§5.4). NULL у живых записей.
  deleted_at          TEXT
);

CREATE INDEX IF NOT EXISTS records_by_vault ON records(vault_id);
"#;

/// Доверенные устройства (§2.2, §2.3). Заводится миграцией 2: хранилищам,
/// созданным до неё, эта таблица досоздаётся при первом же открытии.
const MIGRATION_2: &str = r#"
CREATE TABLE IF NOT EXISTS devices (
  device_id         TEXT PRIMARY KEY,
  name              TEXT NOT NULL,
  kind              TEXT NOT NULL,      -- 'desktop' | 'mobile'
  -- Корень доверия (§2.1): `ed25519_pub(32) || x25519_pub(32)`. Ни MAC, ни IP
  -- здесь нет и не будет — они помогают найти адрес, но не дают права доверять.
  public_key        BLOB NOT NULL,
  -- JSON-массив слов, сверенных человеком при сопряжении. Это метаданные, а не
  -- ключ: из слов ничего не восстанавливается.
  fingerprint_words TEXT NOT NULL,
  paired_at         TEXT NOT NULL,
  last_seen_at      TEXT,
  -- Отзыв (§2.3). NULL у действующих. Строка при отзыве НЕ удаляется: человек
  -- должен видеть, что ноутбук отозван, а не что его не было.
  revoked_at        TEXT
);
"#;

/// Что и до какой версии уже уехало на каждое доверенное устройство (§5.3).
///
/// Таблица, а не колонка в `records`: одна и та же запись бывает довезена до
/// ноутбука и не довезена до телефона — это два разных факта, и одной точкой
/// отсчёта их не описать. Это же значение — общий предок для разрешения
/// конфликтов (§5.5), поэтому оно заводится один раз и здесь.
const MIGRATION_3: &str = r#"
CREATE TABLE IF NOT EXISTS sync_state (
  device_id      TEXT    NOT NULL,
  -- Внешний ключ на записи: надгробие остаётся строкой в `records`, поэтому
  -- отметка живёт ровно столько же, сколько сама запись.
  record_id      TEXT    NOT NULL REFERENCES records(record_id),
  -- Версия, на которой обе стороны сошлись в последний раз (§5.2).
  synced_version INTEGER NOT NULL,
  synced_at      TEXT    NOT NULL,
  PRIMARY KEY (device_id, record_id)
);

CREATE INDEX IF NOT EXISTS sync_state_by_record ON sync_state(record_id);
"#;

/// Расхождения версий, ждущие решения человека (§5.5).
///
/// Приехавшая версия лежит здесь **целиком**, вместе с секретами: местная
/// сторона конфликта всегда есть в `records`, а вот чужую хранить больше негде —
/// в `records` она легла бы поверх местной, то есть ровно тем «тихим затиранием»,
/// от которого конфликт и защищает.
///
/// Строка одна на запись: спорят всегда две стороны, не три. Приедет третье
/// устройство со своим мнением — оно заменит собой прежнюю строку, и человек
/// увидит то расхождение, которое актуально сейчас.
///
/// Секреты перешифрованы **местным** ключом и под **своим** AAD
/// (`crypto::conflict_field_aad`): с общим AAD шифротекст из этой таблицы стал бы
/// взаимозаменяем с настоящим паролем записи.
///
/// `deleted_at` здесь нет намеренно: надгробие в конфликте не участвует, потому
/// что `ConflictVersion` (`contract.ts:854`) не умеет сказать «эта сторона
/// удалена». Удаление решается прежним правилом версии (§5.4).
const MIGRATION_4: &str = r#"
CREATE TABLE IF NOT EXISTS conflicts (
  record_id           TEXT    PRIMARY KEY REFERENCES records(record_id),
  -- От кого приехала версия: по нему берётся имя устройства для экрана.
  device_id           TEXT    NOT NULL,
  raised_at           TEXT    NOT NULL,
  -- Местная версия на момент подъёма. Нужна, чтобы не поднимать одно и то же
  -- расхождение событием на каждом круге, пока ни одна сторона не менялась.
  local_version       INTEGER NOT NULL,
  -- Дальше — приехавшая версия как она есть.
  version             INTEGER NOT NULL,
  vault_id            TEXT    NOT NULL,
  service_name        TEXT    NOT NULL,
  urls                TEXT    NOT NULL,   -- JSON-массив, как в `records`
  login               TEXT    NOT NULL,
  account_label       TEXT,
  password_ct         BLOB,
  notes_ct            BLOB,
  totp_ct             BLOB,
  updated_at          TEXT    NOT NULL,
  password_updated_at TEXT    NOT NULL
);
"#;

/// `ON DELETE CASCADE` на обеих ссылках, которые смотрят в `records` (S7.2).
///
/// Сегодня записи физически не удаляются — от них остаётся надгробие (§5.4), —
/// и разницы нет никакой. Но чистка надгробий по сроку упрётся во внешние
/// ключи: `sync_state.record_id` и `conflicts.record_id` ссылаются на
/// `records(record_id)`, а `PRAGMA foreign_keys` включена, и `DELETE` отвалился
/// бы по ограничению целостности. Решить это миграцией сейчас дешевле, чем в
/// середине шага, где будет не до схемы.
///
/// Форма — стандартная перестройка таблицы: `ALTER TABLE` в SQLite менять
/// внешние ключи не умеет. Обе таблицы дочерние, на них самих не ссылается
/// никто, поэтому переименование ничего больше не задевает. Всё это едет одной
/// транзакцией — см. [`apply`].
const MIGRATION_5: &str = r#"
CREATE TABLE sync_state_next (
  device_id      TEXT    NOT NULL,
  record_id      TEXT    NOT NULL REFERENCES records(record_id) ON DELETE CASCADE,
  synced_version INTEGER NOT NULL,
  synced_at      TEXT    NOT NULL,
  PRIMARY KEY (device_id, record_id)
);
INSERT INTO sync_state_next SELECT device_id, record_id, synced_version, synced_at FROM sync_state;
DROP TABLE sync_state;
ALTER TABLE sync_state_next RENAME TO sync_state;
CREATE INDEX IF NOT EXISTS sync_state_by_record ON sync_state(record_id);

CREATE TABLE conflicts_next (
  record_id           TEXT    PRIMARY KEY REFERENCES records(record_id) ON DELETE CASCADE,
  device_id           TEXT    NOT NULL,
  raised_at           TEXT    NOT NULL,
  local_version       INTEGER NOT NULL,
  version             INTEGER NOT NULL,
  vault_id            TEXT    NOT NULL,
  service_name        TEXT    NOT NULL,
  urls                TEXT    NOT NULL,
  login               TEXT    NOT NULL,
  account_label       TEXT,
  password_ct         BLOB,
  notes_ct            BLOB,
  totp_ct             BLOB,
  updated_at          TEXT    NOT NULL,
  password_updated_at TEXT    NOT NULL
);
INSERT INTO conflicts_next SELECT
  record_id, device_id, raised_at, local_version, version, vault_id, service_name,
  urls, login, account_label, password_ct, notes_ct, totp_ct, updated_at,
  password_updated_at
FROM conflicts;
DROP TABLE conflicts;
ALTER TABLE conflicts_next RENAME TO conflicts;
"#;

/// Метаданные записи уезжают под шифр (S7.1, §3.1).
///
/// Здесь только половина дела — та, которую можно сделать **без ключа
/// хранилища**: заводится колонка под шифротекст. Переложить в неё то, что уже
/// лежит в файле открытым текстом, и убрать открытые колонки миграция не может:
/// ключ появляется только при отпирании, а миграции идут при открытии файла.
/// Вторую половину доделывает [`seal_legacy_metadata`] — по тому же образцу, по
/// которому S1 доснабжает хранилище схемы 1 парой ключей устройства.
///
/// `ALTER TABLE ADD COLUMN`, а не перестройка таблицы: `records` — таблица
/// **родительская**, на неё смотрят два внешних ключа, и переименование при
/// включённой `foreign_keys` переписало бы ссылки в детях на промежуточное имя.
/// Цена — колонка объявлена допускающей NULL, хотя пустой она не бывает; кто
/// именно её заполняет и почему пустота означает испорченный файл, написано в
/// `storage::metadata::open`.
const MIGRATION_6: &str = r#"
ALTER TABLE records   ADD COLUMN meta_ct BLOB;
ALTER TABLE conflicts ADD COLUMN meta_ct BLOB;
"#;

/// Открытые метаданные, оставшиеся от прошлых схем, — под шифр (S7.1).
///
/// Зовётся при каждом отпирании и почти всегда не делает ничего: колонок уже
/// нет, работать не с чем. Хранилище, заведённое до S7.1, проходит через неё
/// ровно один раз — в первое отпирание после обновления.
///
/// Открытые колонки после перекладки **удаляются** (`ALTER TABLE DROP COLUMN`,
/// SQLite ≥ 3.35), а не обнуляются: колонка, которую забыли перестать
/// заполнять, — это тихий возврат к открытому тексту через полгода. Страницы,
/// освободившиеся при удалении, затирает `PRAGMA secure_delete`, включённая при
/// открытии соединения.
///
/// Одной транзакцией с вызывающим: наполовину зашифрованное хранилище — это
/// строки, часть которых нечем прочитать.
pub fn seal_legacy_metadata(tx: &rusqlite::Transaction<'_>, key: &VaultKey) -> CoreResult<()> {
    for (table, place) in [
        ("records", metadata::Place::Record),
        ("conflicts", metadata::Place::Conflict),
    ] {
        if !has_column(tx, table, "service_name")? {
            continue;
        }

        let sql =
            format!("SELECT record_id, service_name, urls, login, account_label FROM {table}");
        let mut stmt = tx.prepare(&sql)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, RecordId>("record_id")?,
                    metadata::RecordFields {
                        service_name: row.get("service_name")?,
                        // Битый JSON здесь — испорченный файл, а не ввод
                        // человека: показать запись без адресов honest-нее,
                        // чем не открыть хранилище вовсе.
                        urls: serde_json::from_str(&row.get::<_, String>("urls")?)
                            .unwrap_or_default(),
                        login: row.get("login")?,
                        account_label: row.get("account_label")?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);

        let update = format!("UPDATE {table} SET meta_ct = ?2 WHERE record_id = ?1");
        for (record_id, fields) in rows {
            let blob = metadata::seal(key, place, &record_id, &fields)?;
            tx.execute(&update, rusqlite::params![record_id, blob])?;
        }

        for column in ["service_name", "urls", "login", "account_label"] {
            tx.execute_batch(&format!("ALTER TABLE {table} DROP COLUMN {column};"))?;
        }
    }
    Ok(())
}

/// Есть ли у таблицы такая колонка. Единственный способ отличить хранилище,
/// уже прошедшее перекладку, от только что мигрировавшего: номер схемы у них
/// один и тот же — перекладка не миграция и версии не двигает.
fn has_column(conn: &Connection, table: &str, column: &str) -> CoreResult<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let found = stmt
        .query_map([], |row| row.get::<_, String>("name"))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|name| name == column);
    Ok(found)
}

/// Без `meta` не прочитать версию схемы, а без версии не выбрать миграции —
/// поэтому одна эта таблица создаётся до всякой цепочки. В `MIGRATION_1` она
/// тоже есть, и повторение здесь безвредно: обе формы — `IF NOT EXISTS`.
const BOOTSTRAP: &str = "CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value BLOB NOT NULL
);";

/// Миграции по порядку: индекс `i` переводит схему с версии `i` на `i + 1`.
const MIGRATIONS: [&str; SCHEMA_VERSION as usize] = [
    MIGRATION_1,
    MIGRATION_2,
    MIGRATION_3,
    MIGRATION_4,
    MIGRATION_5,
    MIGRATION_6,
];

/// Применить миграции от записанной в `meta` версии до текущей.
///
/// Пустой файл (версии нет вовсе) проходит цепочку целиком. Версию пишет
/// **эта функция**, а не `init_vault`: миграции идут и по хранилищу, которое
/// ещё не создано, и хозяин у номера должен быть один. Созданным хранилище
/// по-прежнему делает проверочное значение, а не номер схемы.
pub fn migrate(conn: &Connection) -> CoreResult<()> {
    conn.execute_batch(BOOTSTRAP)?;

    let from = stored_version(conn)?.unwrap_or(0);
    if from > SCHEMA_VERSION {
        // Открыть хранилище схемы из будущего мы не умеем, а сделать вид, что
        // умеем, — значит испортить его молча.
        return Err(CoreError::internal(
            "Хранилище создано более новой версией Syncra.",
        ));
    }

    apply(conn, &MIGRATIONS[from as usize..], SCHEMA_VERSION)
}

/// Цепочка шагов и номер версии — ОДНОЙ транзакцией.
///
/// Сегодня все шаги это `CREATE TABLE IF NOT EXISTS`, и повторный проход
/// безвреден. Но первая же `ALTER TABLE` или переливка данных делает падение на
/// середине неисправимым: часть схемы новая, номер версии старый, и следующий
/// запуск погонит цепочку заново по уже изменённым данным. Найдётся это на
/// хранилище человека, а не на стенде, — поэтому транзакция появляется сейчас,
/// пока она стоит три строки.
///
/// `unchecked_transaction`, а не `Connection::transaction`: у миграций на руках
/// `&Connection` (владеет им `Storage`), и брать ради `BEGIN` изменяемую ссылку
/// значило бы протащить `&mut` через полдюжины вызовов. Вложенной транзакции
/// здесь не бывает — `migrate` зовётся ровно один раз, при открытии.
fn apply(conn: &Connection, steps: &[&str], version: i64) -> CoreResult<()> {
    let tx = conn.unchecked_transaction()?;
    for step in steps {
        tx.execute_batch(step)?;
    }
    tx.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![META_SCHEMA_VERSION, version.to_string().into_bytes()],
    )?;
    // Не докатившаяся транзакция откатывается в `Drop`: оборванная миграция не
    // оставляет ни половины схемы, ни номера, который ей не соответствует.
    tx.commit()?;
    Ok(())
}

/// Версия из `meta`. Нечитаемое значение — это испорченный файл, и молча
/// принимать его за нулевую версию нельзя: цепочка прошла бы по живым данным.
fn stored_version(conn: &Connection) -> CoreResult<Option<i64>> {
    // `CAST(... AS TEXT)` — потому что колонка объявлена как BLOB, но SQLite
    // хранит то, что в неё положили: ядро писало номер байтами, а посторонний
    // инструмент мог записать строкой или числом. Читателю миграций различать
    // эти три случая незачем.
    let raw: Option<String> = conn
        .query_row(
            "SELECT CAST(value AS TEXT) FROM meta WHERE key = ?1",
            [META_SCHEMA_VERSION],
            |row| row.get(0),
        )
        .optional()?;

    match raw {
        None => Ok(None),
        Some(text) => text
            .trim()
            .parse::<i64>()
            .map(Some)
            .map_err(|_| CoreError::internal("Хранилище повреждено.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened() -> Connection {
        let conn = Connection::open_in_memory().expect("хранилище в памяти");
        conn.execute_batch(BOOTSTRAP).expect("meta");
        conn
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .expect("sqlite_master")
            > 0
    }

    #[test]
    fn an_empty_file_goes_all_the_way_to_the_current_schema() {
        let conn = opened();
        migrate(&conn).expect("миграции");

        assert_eq!(stored_version(&conn).unwrap(), Some(SCHEMA_VERSION));
        assert!(table_exists(&conn, "records"));
        // Повторный проход по готовому хранилищу ничего не ломает.
        migrate(&conn).expect("миграции второй раз");
        assert_eq!(stored_version(&conn).unwrap(), Some(SCHEMA_VERSION));
    }

    #[test]
    fn a_deleted_record_takes_its_marks_and_its_conflict_with_it() {
        let conn = opened();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrate(&conn).expect("миграции");

        conn.execute_batch(
            "INSERT INTO vaults VALUES ('v', 'Личное', 'indigo', 1, 1, '2026-08-23T10:00:00.000Z');
             INSERT INTO records (record_id, vault_id, service_name, urls, login, version,
                                  created_at, updated_at, password_updated_at)
               VALUES ('r', 'v', 'GitHub', '[]', 'octocat', 1,
                       '2026-08-23T10:00:00.000Z', '2026-08-23T10:00:00.000Z',
                       '2026-08-23T10:00:00.000Z');
             INSERT INTO sync_state VALUES ('d', 'r', 1, '2026-08-23T10:00:00.000Z');
             INSERT INTO conflicts (record_id, device_id, raised_at, local_version, version,
                                    vault_id, service_name, urls, login, updated_at,
                                    password_updated_at)
               VALUES ('r', 'd', '2026-08-23T10:00:00.000Z', 1, 2, 'v', 'GitHub', '[]',
                       'octocat', '2026-08-23T10:00:00.000Z', '2026-08-23T10:00:00.000Z');",
        )
        .expect("наполнение");

        // Вот ради этого миграция и написана: без каскада чистка надгробий
        // (S7.2) отвалилась бы прямо здесь, по ограничению целостности.
        conn.execute("DELETE FROM records WHERE record_id = 'r'", [])
            .expect("надгробие удаляется");

        let left: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM sync_state) + (SELECT COUNT(*) FROM conflicts)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(left, 0, "ссылки на удалённую запись остались сиротами");
    }

    #[test]
    fn a_migration_that_falls_over_halfway_leaves_nothing_behind() {
        let conn = opened();

        // Первый шаг проходит, второй — нет. Ровно то, что случится с первой же
        // `ALTER TABLE` на чужом хранилище.
        let broken = apply(
            &conn,
            &[
                "CREATE TABLE half_done (id TEXT PRIMARY KEY);",
                "ALTER TABLE nothing_like_this ADD COLUMN oops TEXT;",
            ],
            7,
        );

        assert!(broken.is_err(), "оборванная миграция обязана быть ошибкой");
        assert!(
            !table_exists(&conn, "half_done"),
            "половина схемы пережила откат"
        );
        assert_eq!(
            stored_version(&conn).unwrap(),
            None,
            "номер версии уехал вперёд схемы"
        );
    }
}
