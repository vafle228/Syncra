//! Разбор файлов чужих менеджеров (F12, §6.2 — импорт как jump-start).
//!
//! Ядро знает только, КАК разобрать файл. Откуда его взять и что нажимать в
//! чужой программе — дело UI: это описание посторонних приложений, а не
//! свойство формата (`client/src/components/data/importSources.ts`).
//!
//! Колонки ищутся по ИМЕНАМ заголовка, а не по номерам: Chrome, Firefox,
//! 1Password и KeePass раскладывают одни и те же пять полей в разном порядке,
//! а Firefox не отдаёт имени сервиса вовсе — оно достаётся из адреса.

use std::collections::{HashMap, HashSet};

use zeroize::Zeroizing;

use crate::error::{CoreError, CoreResult};
use crate::model::{ImportRowStatus, ImportSource};

/// Одна строка разобранного файла — как она лежит в ядре между `begin_import`
/// и `commit_import`.
///
/// Здесь чужой пароль открытым текстом. Ровно поэтому строка живёт ЗДЕСЬ, а не
/// в UI, и умирает вместе с замком.
pub struct ImportEntry {
    pub site: String,
    pub login: String,
    /// Пустая строка — «в файле нет пароля»: такие строки записями не станут.
    pub password: Zeroizing<String>,
    pub notes: Option<String>,
    pub totp_secret: Option<String>,
}

/// `Debug` руками, как у `sync::SyncRecord`: производный вывалил бы чужой
/// пароль в первый же лог.
impl std::fmt::Debug for ImportEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportEntry")
            .field("site", &self.site)
            .field("login", &self.login)
            .field("password", &"<скрыт>")
            .field("has_notes", &self.notes.is_some())
            .field("has_totp", &self.totp_secret.is_some())
            .finish()
    }
}

/// Разобрать содержимое файла.
///
/// `text` уже прочитан и проверен на размер выше: этот модуль файлов не
/// открывает — так его можно проверять без диска.
pub fn parse(source: ImportSource, file_name: &str, text: &str) -> CoreResult<Vec<ImportEntry>> {
    if source == ImportSource::KeePass && looks_like_a_database(file_name, text) {
        // Граница шага, а не недоделка: `begin_import` в контракте не принимает
        // пароля (`contract.ts:1092`), а .kdbx без пароля не открывается ничем.
        // Расширять контракт — отдельная задача, согласуемая с фронтом.
        return Err(CoreError::validation(
            "KeePass отдаёт базу зашифрованной, а пароль от неё спросить негде. \
             Экспортируйте из KeePass CSV и выберите его.",
        ));
    }

    // Bitwarden отдаёт и JSON, и CSV; что именно принёс человек, видно по
    // первому непробельному символу, а не по названному источнику.
    if text.trim_start().starts_with('{') {
        return parse_bitwarden_json(text);
    }

    parse_csv(text)
}

/// `.xml` сюда же: XML-разбора в ядре нет, а лечение у этих двух случаев одно
/// и то же — «экспортируйте CSV».
fn looks_like_a_database(file_name: &str, text: &str) -> bool {
    let name = file_name.to_lowercase();
    name.ends_with(".kdbx") || name.ends_with(".xml") || text.trim_start().starts_with("<?xml")
}

// ---------------------------------------------------------------------------
// CSV
// ---------------------------------------------------------------------------

// Синонимы заголовков. Списки закрытые и лежат рядом друг с другом нарочно:
// один и тот же столбец у пяти программ называется пятью способами, и собирать
// это по коду заново потом никто не станет.

const NAME_COLUMNS: [&str; 5] = ["name", "title", "account", "display name", "item name"];
const URL_COLUMNS: [&str; 8] = [
    "url",
    "urls",
    "uri",
    "web site",
    "website",
    "web address",
    "login_uri",
    "login uri",
];
const LOGIN_COLUMNS: [&str; 6] = [
    "username",
    "user name",
    "login",
    "login name",
    "login_username",
    "login username",
];
const PASSWORD_COLUMNS: [&str; 3] = ["password", "login_password", "login password"];
const NOTES_COLUMNS: [&str; 4] = ["notes", "note", "comments", "comment"];
const TOTP_COLUMNS: [&str; 6] = [
    "totp",
    "otp",
    "otpauth",
    "login_totp",
    "login totp",
    "authenticator key",
];

struct Columns {
    name: Option<usize>,
    url: Option<usize>,
    login: Option<usize>,
    password: usize,
    notes: Option<usize>,
    totp: Option<usize>,
}

fn parse_csv(text: &str) -> CoreResult<Vec<ImportEntry>> {
    let rows = super::csv::parse(text);
    let Some(first) = rows.first() else {
        return Err(unreadable());
    };

    let header: Vec<String> = first
        .iter()
        .map(|cell| cell.trim().to_lowercase())
        .collect();
    let columns = Columns {
        name: find(&header, &NAME_COLUMNS),
        url: find(&header, &URL_COLUMNS),
        login: find(&header, &LOGIN_COLUMNS),
        // Без колонки паролей файл бесполезен, а угадывать её по содержимому
        // значило бы завести записи с чужими заметками в качестве паролей.
        password: find(&header, &PASSWORD_COLUMNS).ok_or_else(unreadable)?,
        notes: find(&header, &NOTES_COLUMNS),
        totp: find(&header, &TOTP_COLUMNS),
    };

    let entries = rows[1..]
        .iter()
        .filter(|row| row.iter().any(|cell| !cell.trim().is_empty()))
        .map(|row| ImportEntry {
            // Адрес важнее имени: по нему считается дубликат (§4.4). Имя идёт в
            // ход, только когда адреса в файле нет вовсе.
            site: cell(row, columns.url)
                .filter(|value| !value.is_empty())
                .or_else(|| cell(row, columns.name))
                .unwrap_or_default(),
            login: cell(row, columns.login).unwrap_or_default(),
            // Пароль НЕ обрезается по краям: пробел — законный символ пароля
            // (`model::require_present`).
            password: Zeroizing::new(row.get(columns.password).cloned().unwrap_or_default()),
            notes: filled(cell(row, columns.notes)),
            totp_secret: filled(cell(row, columns.totp)),
        })
        .collect();

    Ok(entries)
}

fn find(header: &[String], names: &[&str]) -> Option<usize> {
    header
        .iter()
        .position(|cell| names.contains(&cell.as_str()))
}

fn cell(row: &[String], column: Option<usize>) -> Option<String> {
    Some(row.get(column?)?.trim().to_owned())
}

fn filled(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.trim().is_empty())
}

fn unreadable() -> CoreError {
    CoreError::validation(
        "Не удалось понять, где в этом файле пароли. Проверьте, что это экспорт \
         менеджера паролей, а не другая таблица.",
    )
}

// ---------------------------------------------------------------------------
// Bitwarden JSON
// ---------------------------------------------------------------------------

/// Берутся только элементы типа «логин» (`type: 1`): карты, удостоверения и
/// защищённые заметки Syncra не хранит (§4.1), и заводить их как записи с
/// пустым паролем хуже, чем не заводить.
fn parse_bitwarden_json(text: &str) -> CoreResult<Vec<ImportEntry>> {
    let file: serde_json::Value = serde_json::from_str(text)
        .map_err(|_| CoreError::validation("Файл не читается как JSON."))?;

    if file.get("encrypted").and_then(serde_json::Value::as_bool) == Some(true) {
        // Та же граница, что у .kdbx, и по той же причине: пароля от файла
        // команда не принимает.
        return Err(CoreError::validation(
            "Этот экспорт Bitwarden зашифрован, а пароль от него спросить негде. \
             Экспортируйте незашифрованный файл.",
        ));
    }

    let items = file
        .get("items")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(unreadable)?;

    let entries = items
        .iter()
        .filter(|item| item.get("type").and_then(serde_json::Value::as_i64) == Some(1))
        .map(|item| {
            let login = item.get("login");
            let uri = login
                .and_then(|value| value.get("uris"))
                .and_then(serde_json::Value::as_array)
                .and_then(|uris| uris.first())
                .and_then(|first| first.get("uri"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim();

            ImportEntry {
                site: if uri.is_empty() {
                    text_of(item.get("name"))
                } else {
                    uri.to_owned()
                },
                login: text_of(login.and_then(|value| value.get("username"))),
                password: Zeroizing::new(text_of(login.and_then(|value| value.get("password")))),
                notes: filled(Some(text_of(item.get("notes")))),
                totp_secret: filled(Some(text_of(login.and_then(|value| value.get("totp"))))),
            }
        })
        .collect();

    Ok(entries)
}

fn text_of(value: Option<&serde_json::Value>) -> String {
    value
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

// ---------------------------------------------------------------------------
// Правила, общие для всех источников
// ---------------------------------------------------------------------------

/// Адрес в сравнимом виде. Повторяет `mock/transfer.ts::importHost` шаг в шаг:
/// расхождение здесь — это разные ответы на вопрос «дубликат ли это», то есть
/// разное содержимое хранилища у двух реализаций одного контракта.
pub fn import_host(site: &str) -> String {
    let trimmed = site.trim().to_lowercase();
    let without_scheme = match trimmed.find("://") {
        Some(at) => &trimmed[at + 3..],
        None => trimmed.as_str(),
    };
    let host = without_scheme.split(['/', '?']).next().unwrap_or_default();
    let host = host.strip_prefix("www.").unwrap_or(host);

    // Порт срезается, только если после двоеточия правда порт.
    match host.rsplit_once(':') {
        Some((left, port)) if !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()) => {
            left.to_owned()
        }
        _ => host.to_owned(),
    }
}

/// `figma.com` → `Figma`. Имя сервиса — подпись для человека, а не ключ (§4.1).
pub fn service_name_from_host(host: &str) -> String {
    let label = host.split('.').next().unwrap_or(host);
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => host.to_owned(),
    }
}

/// Ключ сравнения с тем, что уже есть в хранилище: адрес и логин (§4.4), а не
/// имя сервиса — имя не уникально.
pub fn pair_key(site: &str, login: &str) -> String {
    format!("{} {}", import_host(site), login.trim().to_lowercase())
}

/// Что будет со строкой: новая запись, такая уже есть, пароля нет.
pub fn row_status(entry: &ImportEntry, known: &HashSet<String>) -> ImportRowStatus {
    if entry.password.trim().is_empty() {
        return ImportRowStatus::NoPassword;
    }
    if known.contains(&pair_key(&entry.site, &entry.login)) {
        ImportRowStatus::Duplicate
    } else {
        ImportRowStatus::New
    }
}

/// Сколько ИМПОРТИРОВАННЫХ паролей повторяется на разных сайтах.
///
/// Считает это ядро у себя: чтобы получить то же число во фронте, пришлось бы
/// вынести туда все пароли разом — ровно то, чего не делает ни одна команда
/// контракта.
pub fn count_reused_passwords(entries: &[&ImportEntry]) -> i64 {
    let mut seen: HashMap<&str, i64> = HashMap::new();
    for entry in entries {
        if entry.password.is_empty() {
            continue;
        }
        *seen.entry(entry.password.as_str()).or_default() += 1;
    }

    entries
        .iter()
        .filter(|entry| seen.get(entry.password.as_str()).copied().unwrap_or(0) > 1)
        .count() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(site: &str, login: &str, password: &str) -> ImportEntry {
        ImportEntry {
            site: site.to_owned(),
            login: login.to_owned(),
            password: Zeroizing::new(password.to_owned()),
            notes: None,
            totp_secret: None,
        }
    }

    #[test]
    fn host_is_normalised_exactly_like_the_mock_does_it() {
        for (site, expected) in [
            ("https://www.Github.com/login?next=1", "github.com"),
            ("HTTP://ozon.ru", "ozon.ru"),
            ("nas.local:8080", "nas.local"),
            ("  figma.com  ", "figma.com"),
            ("старый-форум.рф", "старый-форум.рф"),
        ] {
            assert_eq!(import_host(site), expected, "адрес {site}");
        }
    }

    #[test]
    fn a_service_name_is_the_first_label_capitalised() {
        assert_eq!(service_name_from_host("figma.com"), "Figma");
        assert_eq!(service_name_from_host("старый-форум.рф"), "Старый-форум");
    }

    #[test]
    fn chrome_columns_are_found_by_name() {
        let text = "name,url,username,password,note\n\
                    GitHub,https://github.com/,octocat,тайна,заметка\n";
        let entries = parse(ImportSource::Chrome, "Chrome Passwords.csv", text).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].site, "https://github.com/");
        assert_eq!(entries[0].login, "octocat");
        assert_eq!(entries[0].password.as_str(), "тайна");
        assert_eq!(entries[0].notes.as_deref(), Some("заметка"));
    }

    #[test]
    fn a_column_order_nobody_expected_still_reads() {
        // KeePass: другие имена столбцов и другой их порядок.
        let text = "\"Account\",\"Login Name\",\"Password\",\"Web Site\",\"Comments\"\n\
                    NAS,admin,пароль,nas.local,локальный доступ\n";
        let entries = parse(ImportSource::KeePass, "keepass.csv", text).unwrap();

        assert_eq!(entries[0].site, "nas.local");
        assert_eq!(entries[0].login, "admin");
        assert_eq!(entries[0].password.as_str(), "пароль");
    }

    #[test]
    fn firefox_has_no_service_name_and_that_is_fine() {
        let text = "\"url\",\"username\",\"password\",\"guid\"\n\
                    \"https://dzen.ru\",\"demo-reader\",\"тайна\",\"{1}\"\n";
        let entries = parse(ImportSource::Firefox, "logins.csv", text).unwrap();

        assert_eq!(entries[0].site, "https://dzen.ru");
        assert_eq!(
            service_name_from_host(&import_host(&entries[0].site)),
            "Dzen"
        );
    }

    #[test]
    fn a_file_without_a_password_column_is_refused() {
        let refused = parse(ImportSource::Csv, "budget.csv", "месяц,сумма\nмай,100\n");
        assert!(refused.is_err(), "таблица без паролей принята за экспорт");
    }

    #[test]
    fn a_kdbx_says_what_to_do_instead() {
        let refused = parse(ImportSource::KeePass, "Passwords.kdbx", "мусор").unwrap_err();
        assert!(refused.message.contains("CSV"), "{}", refused.message);
    }

    #[test]
    fn bitwarden_json_takes_logins_and_leaves_the_rest() {
        let text = "{\"encrypted\": false, \"items\": [\
            {\"type\": 1, \"name\": \"Notion\", \"notes\": \"из папки Работа\",\
             \"login\": {\"username\": \"me\", \"password\": \"тайна\", \"totp\": \"ABC\",\
                         \"uris\": [{\"uri\": \"https://notion.so\"}]}},\
            {\"type\": 2, \"name\": \"Заметка\"}]}";
        let entries = parse(ImportSource::Bitwarden, "bitwarden_export.json", text).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].site, "https://notion.so");
        assert_eq!(entries[0].totp_secret.as_deref(), Some("ABC"));
        assert_eq!(entries[0].notes.as_deref(), Some("из папки Работа"));
    }

    #[test]
    fn an_encrypted_bitwarden_export_is_refused_like_a_kdbx() {
        let text = "{\"encrypted\": true, \"data\": \"…\"}";
        assert!(parse(ImportSource::Bitwarden, "b.json", text).is_err());
    }

    #[test]
    fn reused_passwords_count_the_rows_not_the_values() {
        let a = entry("a.com", "me", "общий");
        let b = entry("b.com", "me", "общий");
        let c = entry("c.com", "me", "свой");

        // Две строки делят один пароль — значит повторяются ДВЕ записи.
        assert_eq!(count_reused_passwords(&[&a, &b, &c]), 2);
    }

    #[test]
    fn a_row_without_a_password_is_not_a_record() {
        let known = HashSet::new();
        assert_eq!(
            row_status(&entry("a.com", "me", "   "), &known),
            ImportRowStatus::NoPassword
        );
    }

    #[test]
    fn a_duplicate_is_matched_by_host_and_login() {
        let known = HashSet::from([pair_key("https://github.com", "OctoCat")]);
        assert_eq!(
            row_status(&entry("www.github.com/login", "octocat", "x"), &known),
            ImportRowStatus::Duplicate
        );
    }
}
