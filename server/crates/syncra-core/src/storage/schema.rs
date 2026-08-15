//! Схема хранилища и её миграции (§8.3 «Storage»).

use rusqlite::Connection;

use crate::error::CoreResult;

/// Версия схемы. Растёт вместе с миграциями; лежит в `meta`.
pub const SCHEMA_VERSION: i64 = 1;

/// Ключи таблицы `meta`.
pub const META_SCHEMA_VERSION: &str = "schema_version";
pub const META_KDF_SALT: &str = "kdf_salt";
pub const META_KDF_PARAMS: &str = "kdf_params";
pub const META_VERIFIER: &str = "verifier";
pub const META_DEVICE_ID: &str = "device_id";
pub const META_CREATED_AT: &str = "created_at";

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

pub fn migrate(conn: &Connection) -> CoreResult<()> {
    conn.execute_batch(MIGRATION_1)?;
    Ok(())
}
