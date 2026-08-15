//! SQLite-хранилище: соединение, миграции, служебная таблица `meta`.
//!
//! Путь к файлу приходит снаружи — ядро не знает ни про Tauri, ни про папки
//! приложения. Благодаря этому все тесты идут на временной БД, а не на настоящей.

pub mod records;
pub mod schema;
pub mod vaults;

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

use crate::error::CoreResult;

pub struct Storage {
    conn: Connection,
}

impl Storage {
    pub fn open(path: &Path) -> CoreResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| crate::error::CoreError::internal("Не удалось создать хранилище."))?;
        }
        Self::from_connection(Connection::open(path)?)
    }

    /// Для тестов и для проверок, которым не нужен файл на диске.
    pub fn open_in_memory() -> CoreResult<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> CoreResult<Self> {
        // WAL — ради живучести при внезапном выключении; FULL — потому что терять
        // только что сохранённый пароль нельзя, а запись здесь редкая.
        //
        // Через `execute_batch`, а не `pragma_update`: `journal_mode` возвращает
        // строку с новым режимом, а `pragma_update` не ждёт от прагмы ответа.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;",
        )?;
        schema::migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Хранилище создано, если в нём есть проверочное значение: именно оно, а не
    /// наличие файла, отличает «здесь есть чей-то vault» от «файл только что завели».
    pub fn is_initialized(&self) -> CoreResult<bool> {
        Ok(self.meta_get(schema::META_VERIFIER)?.is_some())
    }

    pub fn meta_get(&self, key: &str) -> CoreResult<Option<Vec<u8>>> {
        let value = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get::<_, Vec<u8>>(0)
            })
            .optional()?;
        Ok(value)
    }

    pub fn meta_set(&self, key: &str, value: &[u8]) -> CoreResult<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        )?;
        Ok(())
    }
}
