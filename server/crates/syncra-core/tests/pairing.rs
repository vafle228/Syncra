//! Сопряжение устройств (F8, §2.2).
//!
//! Сети ещё нет (S3), поэтому «второе устройство» здесь — второе ядро в том же
//! процессе, на своём хранилище и со своим мастер-паролем. Обмен кодами — то,
//! что в жизни делает камера: строка пейлоада из одного ядра подаётся в другое.
//!
//! Разбор формата проверяется unit-тестами в `src/pairing.rs`; здесь — то, что
//! видно снаружи: сеанс, замок, отпечаток и запись в `devices`.

mod common;

use std::path::Path;

use common::{assert_code, host, unlocked, MASTER_PASSWORD};
use syncra_core::pairing::{MANUAL_CODE_LENGTH, PAIRING_CODE_TTL_MS};
use syncra_core::{Core, CoreErrorCode, Device, DeviceKind, HostDevice, PairingOffer};

const CODE_ALPHABET: &str = "23456789ABCDEFGHJKLMNPQRSTUVWXYZ";

/// Второе устройство: своё хранилище, свой мастер-пароль, своё имя хоста.
fn peer_core(name: &str) -> Core {
    let mut core = Core::in_memory(HostDevice::desktop(name)).expect("хранилище в памяти");
    core.init_vault("мастер-пароль-2").expect("init_vault");
    core
}

/// То, что в жизни переносит камера: код с экрана второго устройства.
///
/// Ядро отдаёт наружу картинку, а не строку, — тест берёт строку там же, где
/// её берёт телефон, только без декодера QR (`Core::shown_pairing_payload`).
fn payload_of(core: &mut Core) -> (PairingOffer, String) {
    let offer = core.get_pairing_payload().expect("код сопряжения");
    let payload = core
        .shown_pairing_payload()
        .expect("показанный код")
        .to_owned();
    (offer, payload)
}

fn this_device(core: &Core) -> Device {
    core.list_devices()
        .unwrap()
        .into_iter()
        .find(|device| device.is_this_device)
        .expect("своё устройство в списке")
}

fn peers(core: &Core) -> Vec<Device> {
    core.list_devices()
        .unwrap()
        .into_iter()
        .filter(|device| !device.is_this_device)
        .collect()
}

/// Публичный ключ устройства лежит открыто в `devices` — читаем мимо ядра.
fn stored_public_key(path: &Path, device_id: &str) -> Vec<u8> {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.query_row(
        "SELECT public_key FROM devices WHERE device_id = ?1",
        [device_id],
        |row| row.get(0),
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// Код, который показывают
// ---------------------------------------------------------------------------

#[test]
fn an_offer_looks_like_the_contract() {
    let offer = unlocked().get_pairing_payload().unwrap();

    assert_eq!(offer.manual_code.chars().count(), MANUAL_CODE_LENGTH);
    assert!(offer
        .manual_code
        .chars()
        .all(|char| CODE_ALPHABET.contains(char)));

    assert_eq!(offer.qr.rows.len(), offer.qr.size);
    assert!(offer.qr.rows.iter().all(|row| row.len() == offer.qr.size));
    assert!(offer
        .qr
        .rows
        .iter()
        .all(|row| row.bytes().all(|byte| byte == b'0' || byte == b'1')));
    assert!(!offer.session_id.is_empty());
}

#[test]
fn the_code_lives_exactly_three_minutes() {
    // Хранилище создаётся до отсчёта: `init_vault` выводит ключ через Argon2 и
    // занимает секунды — на срок жизни кода это не влияет, а на замер влияло бы.
    let mut core = unlocked();

    let before = chrono::Utc::now();
    let offer = core.get_pairing_payload().unwrap();
    let after = chrono::Utc::now();

    // Срок отсчитывается от момента выдачи, а он где-то между двумя замерами.
    // Три минуты — то, что показывает обратный отсчёт на экране (§3.6 макета);
    // код, живущий дольше обещанного, — приглашение сопрячься для прохожего.
    let deadline = chrono::DateTime::parse_from_rfc3339(&offer.expires_at)
        .unwrap()
        .with_timezone(&chrono::Utc);
    let ttl = chrono::Duration::milliseconds(PAIRING_CODE_TTL_MS);

    assert!(deadline >= before + ttl, "{deadline} против {before}");
    assert!(deadline <= after + ttl, "{deadline} против {after}");
}

#[test]
fn every_request_gives_a_brand_new_code() {
    let mut core = unlocked();
    let first = core.get_pairing_payload().unwrap();
    let second = core.get_pairing_payload().unwrap();

    // Одноразовый ключ на то и одноразовый: прежний не доживает свой срок.
    assert_ne!(first.manual_code, second.manual_code);
    assert_ne!(first.session_id, second.session_id);
    assert_ne!(first.qr.rows, second.qr.rows);
}

#[test]
fn the_offer_never_carries_the_payload_itself() {
    let offer = unlocked().get_pairing_payload().unwrap();
    let json = serde_json::to_string(&offer).unwrap();

    // ЗАКОН №1: в пейлоаде ключи, и JS-строкой ему существовать незачем.
    assert!(!json.to_lowercase().contains("syncra-pair:"), "{json}");
}

// ---------------------------------------------------------------------------
// Замок
// ---------------------------------------------------------------------------

#[test]
fn pairing_lives_behind_the_lock() {
    let mut core = unlocked();
    core.lock();

    assert_code(core.get_pairing_payload(), CoreErrorCode::Locked);
    assert_code(core.submit_paired_key("4TQ9MB"), CoreErrorCode::Locked);
    assert_code(core.confirm_pairing("нет такого"), CoreErrorCode::Locked);
    assert_code(core.cancel_pairing("нет такого"), CoreErrorCode::Locked);
}

#[test]
fn the_lock_forgets_the_code_and_the_started_session() {
    let (mut ours, mut theirs) = (unlocked(), peer_core("Второе"));
    let (_, their_payload) = payload_of(&mut theirs);

    let (offer, _) = payload_of(&mut ours);
    let handshake = ours.submit_paired_key(&their_payload).unwrap();

    ours.lock();
    ours.unlock(MASTER_PASSWORD).unwrap();

    // Начатое сопряжение замок стирает: подтверждать после блокировки нечего.
    assert_code(
        ours.confirm_pairing(&handshake.session_id),
        CoreErrorCode::NotFound,
    );
    // И свой прежний код перестаёт быть «своим»: ядро о нём больше не знает.
    assert_code(
        ours.submit_paired_key(&offer.manual_code),
        CoreErrorCode::NotFound,
    );
}

// ---------------------------------------------------------------------------
// Что приходит на вход
// ---------------------------------------------------------------------------

#[test]
fn what_is_not_a_syncra_code_is_refused() {
    let mut core = unlocked();

    for raw in [
        "",
        "привет",
        "0O1I0O",
        "4TQ9M",
        "syncra-pair:коротко",
        "syncra-pair:4TQ9MB.не-base32",
    ] {
        assert_code(core.submit_paired_key(raw), CoreErrorCode::Validation);
    }
}

#[test]
fn a_hand_typed_code_needs_the_network_that_does_not_exist_yet() {
    // ГРАНИЦА ШАГА. Шесть символов не несут публичного ключа: это имя сеанса, по
    // которому устройство ищут в локальной сети. Поиска нет до S3, и ответ
    // «рядом такого нет» — правда, а не заглушка. Когда поиск появится, этот
    // тест упадёт и потребует переписать себя вместе с обещанием.
    let mut core = unlocked();

    assert_code(core.submit_paired_key("4TQ9MB"), CoreErrorCode::NotFound);
    assert_code(
        core.submit_paired_key(" 4tq · 9mb "),
        CoreErrorCode::NotFound,
    );
}

#[test]
fn our_own_code_is_not_someone_elses() {
    let mut core = unlocked();
    let (offer, payload) = payload_of(&mut core);

    // Самая частая ошибка после устаревшего кода — прочитать свой собственный.
    assert_code(core.submit_paired_key(&payload), CoreErrorCode::Validation);
    assert_code(
        core.submit_paired_key(&offer.manual_code),
        CoreErrorCode::Validation,
    );
}

#[test]
fn a_code_we_have_already_replaced_reports_expired() {
    let mut core = unlocked();
    let (_, retired) = payload_of(&mut core);
    core.get_pairing_payload().unwrap();

    // Не «незнакомый код», а «истёк»: экран по этому коду предложит показать
    // новый, а не проверять набранное (`usePairing.ts:135`).
    assert_code(
        core.submit_paired_key(&retired),
        CoreErrorCode::PairingExpired,
    );
}

#[test]
fn a_tampered_payload_does_not_pass() {
    let (mut ours, mut theirs) = (unlocked(), peer_core("Второе"));
    let (_, payload) = payload_of(&mut theirs);

    let at = payload.len() - 4;
    let swapped = if payload.as_bytes()[at] == b'A' {
        'B'
    } else {
        'A'
    };
    let tampered = format!("{}{swapped}{}", &payload[..at], &payload[at + 1..]);

    // Подпись покрывает всё тело: переклеить чужой ключ в чужой код нельзя.
    assert_code(ours.submit_paired_key(&tampered), CoreErrorCode::Validation);
    assert!(peers(&ours).is_empty());
}

// ---------------------------------------------------------------------------
// Отпечаток (SAS)
// ---------------------------------------------------------------------------

#[test]
fn both_screens_show_the_same_four_words() {
    let (mut ours, mut theirs) = (unlocked(), peer_core("Второе"));
    let (_, our_payload) = payload_of(&mut ours);
    let (_, their_payload) = payload_of(&mut theirs);

    let here = ours.submit_paired_key(&their_payload).unwrap();
    let there = theirs.submit_paired_key(&our_payload).unwrap();

    // В этом весь смысл сверки: слова считаются из ключей ОБЕИХ сторон, и у
    // человека посередине они выйдут другими.
    assert_eq!(here.fingerprint_words, there.fingerprint_words);
    assert_eq!(here.fingerprint_words.len(), 4);
    assert!(here
        .fingerprint_words
        .iter()
        .all(|word| word.chars().count() > 2));

    // Имя и тип второго устройства приезжают в пейлоаде, а не выдумываются.
    assert_eq!(here.peer_name, "Второе");
    assert_eq!(here.peer_kind, DeviceKind::Desktop);
    assert_eq!(there.peer_name, common::HOST_NAME);
}

#[test]
fn a_third_device_gives_different_words() {
    let (mut ours, mut theirs, mut third) = (unlocked(), peer_core("Второе"), peer_core("Третье"));
    let (_, their_payload) = payload_of(&mut theirs);
    let (_, third_payload) = payload_of(&mut third);

    let with_second = ours.submit_paired_key(&their_payload).unwrap();
    let with_third = ours.submit_paired_key(&third_payload).unwrap();

    assert_ne!(with_second.fingerprint_words, with_third.fingerprint_words);
}

#[test]
fn the_words_come_before_the_device_is_trusted() {
    let (mut ours, mut theirs) = (unlocked(), peer_core("Второе"));
    let (_, their_payload) = payload_of(&mut theirs);

    ours.submit_paired_key(&their_payload).unwrap();

    // Иначе человек сверял бы отпечаток уже сопряжённого устройства, и сверка
    // не значила бы ничего.
    assert!(peers(&ours).is_empty());
}

// ---------------------------------------------------------------------------
// Подтверждение и отмена
// ---------------------------------------------------------------------------

#[test]
fn confirming_writes_the_peer_into_trusted_devices() {
    let (mut ours, mut theirs) = (unlocked(), peer_core("Второе"));
    let (_, their_payload) = payload_of(&mut theirs);

    let handshake = ours.submit_paired_key(&their_payload).unwrap();
    let result = ours.confirm_pairing(&handshake.session_id).unwrap();

    assert!(!result.device.is_this_device);
    assert_eq!(result.device.name, "Второе");
    assert_eq!(result.device.kind, DeviceKind::Desktop);
    assert_eq!(result.device.fingerprint_words, handshake.fingerprint_words);
    assert_eq!(result.device.revoked_at, None);
    assert_eq!(result.device.device_id, this_device(&theirs).device_id);

    assert_eq!(peers(&ours), vec![result.device]);
}

#[test]
fn nothing_travels_yet() {
    let (mut ours, mut theirs) = (unlocked(), peer_core("Второе"));
    let (_, their_payload) = payload_of(&mut theirs);

    let handshake = ours.submit_paired_key(&their_payload).unwrap();
    let result = ours.confirm_pairing(&handshake.session_id).unwrap();

    // ГРАНИЦА ШАГА: переносить записи некуда, пока нет сети (S3, S4). Ноль —
    // правда о состоянии, а не заглушка.
    assert_eq!(result.records_transferred, 0);
    assert!(result.duration_ms >= 0);
}

#[test]
fn a_session_is_confirmed_only_once() {
    let (mut ours, mut theirs) = (unlocked(), peer_core("Второе"));
    let (_, their_payload) = payload_of(&mut theirs);

    let handshake = ours.submit_paired_key(&their_payload).unwrap();
    ours.confirm_pairing(&handshake.session_id).unwrap();

    assert_code(
        ours.confirm_pairing(&handshake.session_id),
        CoreErrorCode::NotFound,
    );
}

#[test]
fn a_cancelled_session_leaves_no_trace() {
    let (mut ours, mut theirs) = (unlocked(), peer_core("Второе"));
    let (_, their_payload) = payload_of(&mut theirs);

    let handshake = ours.submit_paired_key(&their_payload).unwrap();
    ours.cancel_pairing(&handshake.session_id).unwrap();

    assert!(peers(&ours).is_empty());
    assert_code(
        ours.confirm_pairing(&handshake.session_id),
        CoreErrorCode::NotFound,
    );
    // Отменять уже отменённое — норма для кнопки, по которой жмут дважды.
    assert!(ours.cancel_pairing(&handshake.session_id).is_ok());
}

#[test]
fn pairing_again_brings_a_revoked_device_back() {
    let (mut ours, mut theirs) = (unlocked(), peer_core("Второе"));

    let (_, first) = payload_of(&mut theirs);
    let handshake = ours.submit_paired_key(&first).unwrap();
    let paired = ours.confirm_pairing(&handshake.session_id).unwrap().device;

    let revoked = ours.revoke_device(&paired.device_id).unwrap();
    assert!(revoked.revoked_at.is_some());

    let (_, second) = payload_of(&mut theirs);
    let handshake = ours.submit_paired_key(&second).unwrap();
    let again = ours.confirm_pairing(&handshake.session_id).unwrap().device;

    // «Сопрячь заново» из списка устройств: вернуть отозванный ноутбук можно
    // только новым сопряжением — и раз оно состоялось, оно его и возвращает.
    assert_eq!(again.device_id, paired.device_id);
    assert_eq!(again.revoked_at, None);
    assert_eq!(peers(&ours).len(), 1);
}

// ---------------------------------------------------------------------------
// Что попадает в файл
// ---------------------------------------------------------------------------

#[test]
fn the_trusted_row_holds_the_real_key_of_the_other_device() {
    let ours_dir = tempfile::tempdir().unwrap();
    let theirs_dir = tempfile::tempdir().unwrap();
    let (ours_path, theirs_path) = (
        ours_dir.path().join("syncra.db"),
        theirs_dir.path().join("syncra.db"),
    );

    let mut ours = Core::open(&ours_path, host()).unwrap();
    ours.init_vault(MASTER_PASSWORD).unwrap();
    let mut theirs = Core::open(&theirs_path, HostDevice::desktop("Второе")).unwrap();
    theirs.init_vault("мастер-пароль-2").unwrap();

    let (_, their_payload) = payload_of(&mut theirs);
    let handshake = ours.submit_paired_key(&their_payload).unwrap();
    let peer = ours.confirm_pairing(&handshake.session_id).unwrap().device;

    // Корень доверия доехал целиком и без изменений: в нашей строке про них
    // лежит ровно тот ключ, который они считают своим (§2.1).
    assert_eq!(
        stored_public_key(&ours_path, &peer.device_id),
        stored_public_key(&theirs_path, &this_device(&theirs).device_id)
    );
    assert_eq!(stored_public_key(&ours_path, &peer.device_id).len(), 64);
}

// ---------------------------------------------------------------------------
// Ручная сверка
// ---------------------------------------------------------------------------

/// Печатает настоящий код, чтобы его можно было прочитать телефоном.
///
/// Автоматически это не проверить: декодера QR в зависимостях нет и заводить
/// его ради одного теста незачем. А проверять надо — в связке «пейлоад → кодер
/// → матрица» ошибка даёт правдоподобную, но нечитаемую картинку, и никакой
/// другой тест этого не заметит. Отсюда `#[ignore]`: тест не мешает `cargo
/// test`, но остаётся под рукой.
///
/// ```sh
/// cargo test -p syncra-core --test pairing -- --ignored --nocapture scan_this
/// ```
///
/// Сканер должен показать строку, начинающуюся с `SYNCRA-PAIR:`.
#[test]
#[ignore = "ручная сверка: код читают телефоном, а не тестом"]
fn scan_this_code_with_a_phone() {
    let offer = unlocked().get_pairing_payload().unwrap();

    println!(
        "
код для ручного ввода: {}
",
        offer.manual_code
    );
    for row in &offer.qr.rows {
        let line: String = row
            .chars()
            .map(|module| if module == '1' { "  " } else { "██" })
            .collect();
        // Светлый модуль печатается блоком, а тёмный пробелом: терминал по
        // умолчанию тёмный, и код в нём выходит инвертированным — сканеры такой
        // не читают.
        println!("████{line}████");
    }
    println!();
}
