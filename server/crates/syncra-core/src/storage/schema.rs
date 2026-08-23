//! Схема хранилища и её миграции (§8.3 «Storage»).

use rusqlite::{Connection, OptionalExtension};

use crate::error::{CoreError, CoreResult};

/// Версия схемы. Растёт вместе с миграциями; лежит в `meta`.
pub const SCHEMA_VERSION: i64 = 4;

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

/// Без `meta` не прочитать версию схемы, а без версии не выбрать миграции —
/// поэтому одна эта таблица создаётся до всякой цепочки. В `MIGRATION_1` она
/// тоже есть, и повторение здесь безвредно: обе формы — `IF NOT EXISTS`.
const BOOTSTRAP: &str = "CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value BLOB NOT NULL
);";

/// Миграции по порядку: индекс `i` переводит схему с версии `i` на `i + 1`.
const MIGRATIONS: [&str; SCHEMA_VERSION as usize] =
    [MIGRATION_1, MIGRATION_2, MIGRATION_3, MIGRATION_4];

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
