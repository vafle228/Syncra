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
pub type PairingSessionId = String;
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

// ---------------------------------------------------------------------------
// Потолки длины полей записи
// ---------------------------------------------------------------------------
//
// Это не про красоту формы, а про то, доедет ли запись. Кадр обмена ограничен
// мегабайтом (`net::wire::MAX_FRAME`), длину кадра называет отправитель, и
// опустить этот потолок нельзя. Запись, которая в кадр не влезает, не уезжает
// НИКОГДА: `net::sync::split` кладёт её в отдельную порцию, `write_frame`
// порцию отвергает, круг рвётся — и рвётся каждую минуту до конца времён.
// Человеку при этом никто не говорит, из-за чего; §5.4 называет тихо не
// доехавшую запись худшей из поломок.
//
// Поэтому потолок стоит ЗДЕСЬ, при создании, где о нём можно сказать словами.
// Суммарно запись на всех потолках сразу — около четырёхсот килобайт: втрое
// меньше кадра, и с запасом на кодирование, шифрование и метаданные секции.

/// Потолок секретного поля: пароль, заметка, секрет TOTP.
///
/// Сто двадцать восемь килобайт текста — это примерно полсотни страниц; заметка
/// в записи менеджера паролей столько не бывает, а кадр от трёх таких полей
/// ещё не лопается.
pub const SECRET_FIELD_MAX_BYTES: usize = 128 * 1024;

/// Потолок метаданного поля: имя сервиса, логин, подпись аккаунта, один адрес.
pub const META_FIELD_MAX_BYTES: usize = 4 * 1024;

/// Сколько адресов может быть у одной записи.
pub const RECORD_URLS_MAX: usize = 16;

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

    /// Разобрать имя поля, пришедшее из IPC.
    ///
    /// Именем, а не `Deserialize`, потому что от неизвестного значения ядро
    /// обязано ответить `VALIDATION` с человеческим текстом
    /// (`mock/index.ts:1407`). Провал serde на границе Tauri дал бы вместо
    /// этого строку, из которой `toCoreError` слепит безликое «непредвиденная
    /// ошибка ядра».
    pub fn parse(raw: &str) -> CoreResult<Self> {
        match raw {
            "password" => Ok(Self::Password),
            "notes" => Ok(Self::Notes),
            "totp_secret" => Ok(Self::TotpSecret),
            _ => Err(CoreError::validation(
                "У записи нет такого секретного поля.",
            )),
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
// Сопряжение (F8, §2.2)
// ---------------------------------------------------------------------------

/// Готовая матрица QR-кода (`QrMatrix`, `contract.ts:643`).
///
/// Кодирует ЯДРО, а не UI: в пейлоаде лежит одноразовый ключ сеанса, и чем
/// меньше он существует в виде JS-строки, тем лучше. Наружу уходит картинка
/// кода, а не то, что в нём написано.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QrMatrix {
    /// Сторона матрицы в модулях. Полей вокруг нет — их рисует UI.
    pub size: usize,
    /// `size` строк по `size` символов: `'1'` — тёмный модуль, `'0'` — светлый.
    pub rows: Vec<String>,
}

/// Код, который устройство показывает второму (`PairingOffer`, `contract.ts:656`).
///
/// Строки пейлоада здесь нет и не будет: она осталась внутри `qr`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairingOffer {
    pub session_id: PairingSessionId,
    pub qr: QrMatrix,
    /// Тот же сеанс шестью символами — для случая, когда камеры нет.
    /// Канонический вид, без разделителей: их для читаемости расставляет UI.
    pub manual_code: String,
    pub expires_at: IsoDateTime,
}

/// Что второе устройство узнало, прочитав код (`PairingHandshake`,
/// `contract.ts:676`). Сопряжение на этом ещё НЕ состоялось.
///
/// `fingerprint_words` — та самая защита от MITM (§2.2): слова считаются из
/// ключей обеих сторон и должны совпасть на двух экранах. Поэтому ядро
/// возвращает их ДО того, как запишет устройство в доверенные, а не после.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairingHandshake {
    pub session_id: PairingSessionId,
    pub peer_name: String,
    pub peer_kind: DeviceKind,
    pub fingerprint_words: Vec<String>,
    /// Докуда действителен сеанс: сверять отпечаток вечно нельзя.
    pub expires_at: IsoDateTime,
}

/// Чем закончилось сопряжение (`PairingResult`, `contract.ts:686`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairingResult {
    pub device: Device,
    /// Сколько записей уехало на новое устройство.
    ///
    /// Считается по настоящему переносу: сразу после подтверждения ядро
    /// доставляет соседу «я тебя записал» и тем же заходом гоняет круг обмена.
    /// Записи локальных секций в это число не входят: выключенная синхронизация
    /// означает «никуда», в том числе на только что сопряжённое устройство
    /// (§4.2).
    ///
    /// Ноль бывает и правдой: сосед не отозвался с первого раза (доставку и
    /// перенос тогда доделывает фон), либо это событие `device_paired` на
    /// стороне, которая ПОКАЗЫВАЛА код, — ей звонить соседу ещё некуда.
    pub records_transferred: i64,
    /// Сколько занял перенос. Считает ядро — оно его и делало.
    pub duration_ms: i64,
}

// ---------------------------------------------------------------------------
// Синхронизация (F10, §5.1 · §5.3)
// ---------------------------------------------------------------------------

/// Машинное состояние синхронизации (`SyncPhase`, `contract.ts:779`).
///
/// Четыре значения, а не шесть видов индикатора: «ждут отправки», «рядом
/// никого» и «всё сошлось» UI выводит сам из остальных полей
/// (`components/sync/syncFormat.ts`). Ядру про них знать незачем.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncPhase {
    /// Ничего не происходит. Это НЕ «всё плохо»: с найденными соседями и без
    /// ждущих записей `idle` и означает «всё сошлось».
    Idle,
    /// Ищем устройства в локальной сети (§5.1).
    Searching,
    /// Идёт обмен манифестами и диффом с конкретным устройством (§5.3).
    Syncing,
    /// Последняя попытка не удалась. Данные при этом целы: не доехал дифф.
    Error,
}

/// Что происходит с синхронизацией прямо сейчас (`SyncStatus`, `contract.ts:798`).
///
/// Счётчика конфликтов здесь намеренно НЕТ: он приходит из `list_conflicts`, и
/// две правды об одном числе рано или поздно разъедутся.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncStatus {
    pub phase: SyncPhase,
    /// Сколько ДОВЕРЕННЫХ устройств на связи. Отозванное устройство
    /// обнаруживается, но сюда не попадает — оно не пир (§2.3).
    pub peers_online: u32,
    /// С кем идёт обмен. `null` вне фазы `syncing`.
    pub peer_name: Option<String>,
    /// Записи, ждущие отправки — список, а не счётчик: по нему UI помечает
    /// строки.
    ///
    /// «Ждёт отправки» считается ПО ПИРАМ: запись попадает сюда, только если
    /// есть действующее сопряжённое устройство, которому она ещё не уехала. Нет
    /// таких устройств — список пуст, потому что «ждёт отправки» без адресата
    /// обещало бы отправку, которой не будет. Это то же рассуждение, которым
    /// отсюда исключены локальные секции: им некуда ехать (§4.2).
    pub pending_records: Vec<RecordId>,
    pub last_sync_at: Option<IsoDateTime>,
    /// Человеческое объяснение — непусто только в фазе `error`. Ни адресов, ни
    /// ключей, ни путей, как и любое сообщение ядра.
    pub message: Option<String>,
}

impl SyncStatus {
    /// Состояние узла, который ещё ничего не сделал.
    ///
    /// Это же значение отдаётся на запертом хранилище: сети на нём нет, и `idle`
    /// с нулём соседей — правда о состоянии, а не заглушка.
    pub fn idle() -> Self {
        Self {
            phase: SyncPhase::Idle,
            peers_online: 0,
            peer_name: None,
            pending_records: Vec::new(),
            last_sync_at: None,
            message: None,
        }
    }
}

/// Доверенное устройство появилось в сети (`PeerFoundEvent`, `contract.ts:1306`).
///
/// Отдельно от `sync_status` намеренно: статус — «что происходит сейчас», а это
/// — «вот это устройство на связи». По нему двигается `Device.last_seen_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PeerFound {
    pub device_id: DeviceId,
    pub name: String,
    pub kind: DeviceKind,
    pub found_at: IsoDateTime,
}

// ---------------------------------------------------------------------------
// Конфликты версий (F11, §5.5)
// ---------------------------------------------------------------------------

/// Чья версия (`ConflictSide`, `contract.ts:836`).
///
/// Человек выбирает СТОРОНУ, а не номер: конфликт растёт из общего предка, и обе
/// стороны сплошь и рядом несут одно и то же число — 4 стало 5 и здесь, и там.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictSide {
    /// Эта машина.
    Local,
    /// То, что приехало от пира.
    Remote,
}

impl ConflictSide {
    /// Разобрать сторону, пришедшую из IPC, — по той же причине, что и
    /// [`SecretField::parse`].
    pub fn parse(raw: &str) -> CoreResult<Self> {
        match raw {
            "local" => Ok(Self::Local),
            "remote" => Ok(Self::Remote),
            _ => Err(CoreError::validation("У конфликта нет такой стороны.")),
        }
    }
}

/// Поле, по которому версии разошлись (`ConflictField`, `contract.ts:842`).
///
/// Секретные поля здесь ЕСТЬ — но как имена, а не как значения: «пароли
/// различаются» это факт о записи, а не сам пароль. Закон №1 цел.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictField {
    VaultId,
    ServiceName,
    Urls,
    Login,
    AccountLabel,
    Password,
    Notes,
    TotpSecret,
}

/// Одна из двух версий записи (`ConflictVersion`, `contract.ts:854`).
///
/// Секретных значений нет — те же флаги `has_notes` / `has_totp`, что и в
/// [`RecordMeta`]. «Полный diff» §5.5 собирается так: метаданные приходят сразу,
/// а секреты открываются разово через `get_conflict_secret`. Иначе один
/// приехавший конфликт выкладывал бы на экран два пароля, которых никто не
/// просил.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConflictVersion {
    pub side: ConflictSide,
    /// Устройство, на котором сделана эта правка, — «какая версия чья».
    pub device_name: String,
    pub version: i64,
    /// «Какая версия раньше, какая позже» (§5.5) — показывается человеку.
    pub updated_at: IsoDateTime,
    /// Секцию тоже могли поменять — а вместе с ней и то, уезжает ли запись (§4.2).
    pub vault_id: VaultId,
    pub service_name: String,
    pub urls: Vec<String>,
    pub login: String,
    pub account_label: Option<String>,
    pub has_notes: bool,
    pub has_totp: bool,
}

/// Одна и та же запись, независимо изменённая на двух устройствах
/// (`RecordConflict`, `contract.ts:878`).
///
/// Разрешение — на уровне ЦЕЛОЙ записи: человек выбирает одну версию целиком, а
/// не собирает её по полям. Поэтому здесь ровно две стороны и ни одного «слить
/// автоматически».
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordConflict {
    pub record_id: RecordId,
    /// Когда ядро обнаружило расхождение — при обмене диффом (§5.3).
    pub raised_at: IsoDateTime,
    pub local: ConflictVersion,
    pub remote: ConflictVersion,
    /// По каким полям версии расходятся. Значений секретов здесь НЕТ.
    pub differing_fields: Vec<ConflictField>,
}

/// Одно секретное поле обеих версий сразу (`GetConflictSecretResponse`,
/// `contract.ts:921`).
///
/// Вторая из трёх команд Закона №1. Обе стороны в одном ответе намеренно:
/// сравнить одно значение с другим — весь смысл действия, а по одному значению
/// человеку пришлось бы нажимать дважды и держать первое на экране, пока едет
/// второе.
///
/// `password` здесь тоже `Option`, хотя в [`RecordSecrets`] он обязателен: у
/// приехавшего надгробия секретов нет вовсе.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConflictSecrets {
    pub local: Option<String>,
    pub remote: Option<String>,
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
    within(trimmed, META_FIELD_MAX_BYTES, field)?;
    Ok(trimmed.to_owned())
}

/// Влезает ли поле в свой потолок. Считаем в БАЙТАХ, а не в символах: в кадр
/// едут байты, и кириллическая заметка весит вдвое против латинской.
pub fn within(value: &str, limit: usize, field: &str) -> CoreResult<()> {
    if value.len() > limit {
        return Err(CoreError::validation(format!(
            "Поле «{field}» длиннее {} КБ — такая запись не уедет на другое \
             устройство. Сократите его.",
            limit / 1024
        )));
    }
    Ok(())
}

/// Необязательное метаданное поле: потолок тот же, пустота законна.
pub fn optional_meta(value: Option<&str>, field: &str) -> CoreResult<Option<String>> {
    match value {
        Some(text) => {
            within(text, META_FIELD_MAX_BYTES, field)?;
            Ok(Some(text.to_owned()))
        }
        None => Ok(None),
    }
}

/// Адреса записи: и число, и длина каждого.
pub fn valid_urls(urls: &[String]) -> CoreResult<Vec<String>> {
    if urls.len() > RECORD_URLS_MAX {
        return Err(CoreError::validation(format!(
            "У записи не может быть больше {RECORD_URLS_MAX} адресов."
        )));
    }
    for url in urls {
        within(url, META_FIELD_MAX_BYTES, "Адрес")?;
    }
    Ok(urls.to_vec())
}

/// Секретное поле: проверяем, но НЕ трогаем. Пробел по краям — легитимный символ
/// пароля, и обрезать его нельзя (см. `mock/index.ts:289`).
pub fn require_present(value: &str, field: &str) -> CoreResult<String> {
    if value.trim().is_empty() {
        return Err(CoreError::validation(format!(
            "Поле «{field}» обязательно."
        )));
    }
    within(value, SECRET_FIELD_MAX_BYTES, field)?;
    Ok(value.to_owned())
}

/// Пустая-после-trim заметка — это «заметки нет», а не «заметка из пробелов»
/// (`mock/seed.ts:22`). Такое значение до шифрования не доходит вовсе.
pub fn normalize_optional_secret(value: Option<&str>, field: &str) -> CoreResult<Option<String>> {
    match value {
        Some(text) if !text.trim().is_empty() => {
            within(text, SECRET_FIELD_MAX_BYTES, field)?;
            Ok(Some(text.to_owned()))
        }
        _ => Ok(None),
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
