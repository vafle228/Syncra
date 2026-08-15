//! Модель данных ядра — зеркало `client/src/core/contract.ts` (§4 спека).
//!
//! Имена полей и их необязательность повторяют контракт дословно: этот модуль
//! сериализуется прямо в IPC-ответы, и любое расхождение здесь — расхождение с
//! фронтом.
//!
//! ЗАКОН №1 в типах: [`RecordMeta`] структурно не может нести секрет. Он есть
//! только в [`RecordSecrets`], который возвращает единственная команда —
//! `get_secret`.

use serde::{Deserialize, Deserializer, Serialize};

use crate::error::{CoreError, CoreResult};

pub type RecordId = String;
pub type VaultId = String;
/// ISO-8601 UTC, напр. `2026-08-05T21:14:03.000Z`.
pub type IsoDateTime = String;

/// `MASTER_PASSWORD_MIN_LENGTH` (`contract.ts:239`).
pub const MASTER_PASSWORD_MIN_LENGTH: usize = 8;
/// `VAULT_NAME_MAX_LENGTH` (`contract.ts:395`).
pub const VAULT_NAME_MAX_LENGTH: usize = 40;
/// `VAULT_COLORS` (`contract.ts:361`) — порядок как в палитре макета.
pub const VAULT_COLORS: [&str; 5] = ["indigo", "amber", "magenta", "mint", "coral"];

/// Текущее время в том же виде, в каком его отдаёт `new Date().toISOString()`.
pub fn now_iso() -> IsoDateTime {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

// ---------------------------------------------------------------------------
// Записи
// ---------------------------------------------------------------------------

/// Метаданные записи. Секретных полей нет намеренно — их отсутствие в типе и
/// есть машинно-проверяемая часть Закона №1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordMeta {
    pub record_id: RecordId,
    pub vault_id: VaultId,
    pub service_name: String,
    pub urls: Vec<String>,
    pub login: String,
    pub account_label: Option<String>,
    /// Заполнено ли `notes`. Считается по наличию шифротекста — не расшифровывая.
    pub has_notes: bool,
    /// Подключён ли `totp_secret`. Так же по наличию шифротекста.
    pub has_totp: bool,
    pub version: i64,
    pub created_at: IsoDateTime,
    pub updated_at: IsoDateTime,
    pub password_updated_at: IsoDateTime,
    /// Надгробие (§5.4). `null` у живых записей.
    pub deleted_at: Option<IsoDateTime>,
}

/// Расшифрованные секреты. Уходят наружу только из `get_secret`, разово.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordSecrets {
    pub password: String,
    pub notes: Option<String>,
    pub totp_secret: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecordDraft {
    /// Не указан — запись ложится в секцию по умолчанию.
    #[serde(default)]
    pub vault_id: Option<VaultId>,
    pub service_name: String,
    pub urls: Vec<String>,
    pub login: String,
    #[serde(default)]
    pub account_label: Option<String>,
    pub password: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub totp_secret: Option<String>,
}

/// Патч записи. Отсутствующее поле = «не трогать», явный `null` = «очистить».
///
/// Отсюда двойной `Option` у тех полей, которые контракт объявляет нулевыми:
/// без него «не трогать заметку» и «стереть заметку» приезжали бы одинаково.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RecordPatch {
    #[serde(default)]
    pub vault_id: Option<VaultId>,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub urls: Option<Vec<String>>,
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub account_label: Option<Option<String>>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub notes: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable")]
    pub totp_secret: Option<Option<String>>,
}

/// Отличает «поля нет в JSON» от «поле пришло как `null`».
fn nullable<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

/// Имя секретного поля — для AAD и для точечных reveal в будущих командах.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretField {
    Password,
    Notes,
    TotpSecret,
}

impl SecretField {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::Notes => "notes",
            Self::TotpSecret => "totp_secret",
        }
    }
}

// ---------------------------------------------------------------------------
// Секции
// ---------------------------------------------------------------------------

/// Секция (§4.2). Это папка, а не отдельное хранилище: ключ у всех секций один.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Vault {
    pub vault_id: VaultId,
    pub name: String,
    /// Имя ступени палитры, а не CSS-цвет: оформление в синкаемых данных не живёт.
    pub color: String,
    /// Уезжает ли содержимое секции на другие устройства (§4.2).
    pub sync: bool,
    /// Куда ложится запись, созданная без явной секции. Помечена ровно одна.
    pub is_default: bool,
    pub created_at: IsoDateTime,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct VaultPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

// ---------------------------------------------------------------------------
// Состояние хранилища
// ---------------------------------------------------------------------------

/// Быстрый вход по PIN. Свойство УСТРОЙСТВА, а не хранилища.
///
/// В этом шаге PIN не реализован, но поле обязано быть в ответе: экран блокировки
/// решает, что рисовать, по нему — до первой отрисовки, а не после второго запроса.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PinStatus {
    pub enrolled: bool,
    pub attempts_left: Option<i64>,
}

impl PinStatus {
    pub fn not_enrolled() -> Self {
        Self {
            enrolled: false,
            attempts_left: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VaultStatus {
    pub initialized: bool,
    pub unlocked: bool,
    pub unlocked_at: Option<IsoDateTime>,
    pub pin: PinStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InitVaultResponse {
    pub initialized_at: IsoDateTime,
    pub unlocked_at: IsoDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnlockResponse {
    pub unlocked_at: IsoDateTime,
}

// ---------------------------------------------------------------------------
// Валидация — общая для create и update, чтобы правила не разъехались
// ---------------------------------------------------------------------------

/// Метаданное поле: обрезаем края и требуем непустоту.
pub fn require_non_empty(value: &str, field: &str) -> CoreResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(CoreError::validation(format!(
            "Поле «{field}» обязательно."
        )));
    }
    Ok(trimmed.to_owned())
}

/// Секретное поле: проверяем, но НЕ трогаем. Пробел по краям — легитимный символ
/// пароля, и обрезать его нельзя (см. `mock/index.ts:289`).
pub fn require_present(value: &str, field: &str) -> CoreResult<String> {
    if value.trim().is_empty() {
        return Err(CoreError::validation(format!(
            "Поле «{field}» обязательно."
        )));
    }
    Ok(value.to_owned())
}

/// Пустая-после-trim заметка — это «заметки нет», а не «заметка из пробелов»
/// (`mock/seed.ts:22`). Такое значение до шифрования не доходит вовсе.
pub fn normalize_optional_secret(value: Option<&str>) -> Option<String> {
    match value {
        Some(text) if !text.trim().is_empty() => Some(text.to_owned()),
        _ => None,
    }
}

pub fn valid_vault_name(name: &str) -> CoreResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CoreError::validation("У секции должно быть имя."));
    }
    if trimmed.chars().count() > VAULT_NAME_MAX_LENGTH {
        return Err(CoreError::validation(format!(
            "Имя секции длиннее {VAULT_NAME_MAX_LENGTH} символов."
        )));
    }
    Ok(trimmed.to_owned())
}

pub fn valid_vault_color(color: &str) -> CoreResult<String> {
    if VAULT_COLORS.contains(&color) {
        Ok(color.to_owned())
    } else {
        Err(CoreError::validation("Неизвестный цвет метки секции."))
    }
}
