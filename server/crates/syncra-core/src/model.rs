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
pub type DeviceId = String;
/// ISO-8601 UTC, напр. `2026-08-05T21:14:03.000Z`.
pub type IsoDateTime = String;

/// `MASTER_PASSWORD_MIN_LENGTH` (`contract.ts:239`).
pub const MASTER_PASSWORD_MIN_LENGTH: usize = 8;
/// `PIN_LENGTH` (`contract.ts:176`). Ступень одна: выбор «4 или 6» — вопрос
/// политики ядра, а не настройка на экране.
pub const PIN_LENGTH: usize = 4;
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
// Доверенные устройства (§2.1, §2.3)
// ---------------------------------------------------------------------------

/// Тип устройства (`DeviceKind`, `contract.ts:605`). Только для иконки и
/// подписи: в доверии не участвует — там решает публичный ключ (§2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Desktop,
    Mobile,
}

impl DeviceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Mobile => "mobile",
        }
    }

    /// Разбор того, что лежит в БД. Незнакомое значение — испорченный файл, а не
    /// пользовательский ввод: показать устройство «неизвестного типа» честнее,
    /// чем уронить весь список из-за иконки.
    pub fn parse_or_desktop(raw: &str) -> Self {
        match raw {
            "mobile" => Self::Mobile,
            _ => Self::Desktop,
        }
    }
}

/// Доверенное устройство (`Device`, `contract.ts:615`).
///
/// Полей ровно восемь, и это проверяется с двух сторон: здесь — типом, во
/// фронте — `core/__tests__/devices.spec.ts`, который сверяет набор ключей
/// списком. Ни публичного ключа, ни MAC-адреса тут нет: корень доверия остаётся
/// в ядре, а сетевые адреса пользователю показывать незачем (§2.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Device {
    pub device_id: DeviceId,
    pub name: String,
    pub kind: DeviceKind,
    /// Устройство, на котором открыт этот UI. Себя отозвать нельзя (F9).
    pub is_this_device: bool,
    pub paired_at: IsoDateTime,
    /// Слова, сверенные глазами при сопряжении (§2.2). Метаданные, а не ключ.
    pub fingerprint_words: Vec<String>,
    /// Когда устройство последний раз выходило на связь. `null` — ни разу.
    pub last_seen_at: Option<IsoDateTime>,
    /// Отзыв доступа (§2.3). `null` у действующих.
    pub revoked_at: Option<IsoDateTime>,
}

/// Что оболочка знает про машину, а ядро — нет.
///
/// Приходит готовым аргументом, как и путь к файлу хранилища: ядро не умеет
/// спрашивать у ОС имя хоста и не должно уметь (§8.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostDevice {
    pub name: String,
    pub kind: DeviceKind,
}

/// Чем подписывается устройство, у которого ОС не назвала имени.
pub const UNNAMED_DEVICE: &str = "Это устройство";
/// Потолок длины имени. Имя приходит из ОС и бывает любым, а помещаться ему в
/// карточку списка устройств.
pub const DEVICE_NAME_MAX_LENGTH: usize = 64;

impl HostDevice {
    /// Имя из ОС как есть не берётся: пустое подменяется, длинное подрезается.
    /// Это не валидация чужого ввода — отказывать тут некому, — а приведение
    /// платформенного факта к тому, что можно нарисовать.
    pub fn new(name: &str, kind: DeviceKind) -> Self {
        let trimmed = name.trim();
        let name = if trimmed.is_empty() {
            UNNAMED_DEVICE.to_owned()
        } else {
            trimmed.chars().take(DEVICE_NAME_MAX_LENGTH).collect()
        };
        Self { name, kind }
    }

    /// MVP 1 — десктоп (§9); мобильная ветка появится вместе с iOS-клиентом.
    pub fn desktop(name: &str) -> Self {
        Self::new(name, DeviceKind::Desktop)
    }
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

/// Итог смены мастер-пароля (`contract.ts:285`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangeMasterPasswordResponse {
    pub changed_at: IsoDateTime,
    /// Сколько доверенных устройств спросят новый пароль при следующей встрече в
    /// сети. Пока сопряжения нет, устройство ровно одно — это, и ноль здесь
    /// правда, а не заглушка.
    pub devices_to_update: i64,
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
