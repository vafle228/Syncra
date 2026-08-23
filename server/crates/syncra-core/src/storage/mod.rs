//! SQLite-хранилище: соединение, миграции, служебная таблица `meta`.
//!
//! Путь к файлу приходит снаружи — ядро не знает ни про Tauri, ни про папки
//! приложения. Благодаря этому все тесты идут на временной БД, а не на настоящей.

pub mod metadata;
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
            restrict(parent, 0o700);
        }
        let storage = Self::from_connection(Connection::open(path)?)?;
        // После открытия, а не до: файла может ещё не быть. WAL и `-shm` создаёт
        // сам SQLite и наследует права от основного файла.
        restrict(path, 0o600);
        Ok(storage)
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
             PRAGMA foreign_keys = ON;
             -- Освобождённые страницы затираются, а не остаются лежать в файле
             -- со старым содержимым. С S7.1 всё, что попадает во freelist,
             -- зашифровано — но затирание никуда не делось: на нём держится
             -- перекладка метаданных под шифр (`schema::seal_legacy_metadata`),
             -- которая как раз и освобождает страницы с открытым текстом.
             PRAGMA secure_delete = ON;",
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

/// Ужать права на файл или папку хранилища.
///
/// На Windows это ничего не делает: папка приложения там и так пользовательская,
/// а прав в понимании POSIX у файла нет. Для сборки под Linux и для MVP 2 (iOS)
/// — делает: `create_dir_all` и `Connection::open` оставляют права на усмотрение
/// umask, а хранилище менеджера паролей читаемым для всех быть не должно.
///
/// Неудача проглатывается намеренно: не сумели ужать права — это не повод не
/// открыть хранилище человеку, который его и так открывает своим.
#[cfg(unix)]
pub(crate) fn restrict(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
pub(crate) fn restrict(_path: &Path, _mode: u32) {}
