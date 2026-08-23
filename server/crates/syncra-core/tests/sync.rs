//! Обмен записями между двумя устройствами (S4, §5.2 · §5.3 · §5.4).
//!
//! Стенд тот же, что у `net.rs`: два узла в одном процессе, на своих хранилищах
//! и со **своими мастер-паролями**, разговаривают через настоящий TCP на
//! loopback. Разные пароли здесь не декорация — это главная проверка всего
//! шага: ключи хранилищ у сторон разные, шифротексты несовместимы, и приехавший
//! пароль обязан читаться на той стороне только потому, что его перешифровали.
//!
//! mDNS выключен: адреса узлам подаются готовыми, как подавал бы их поиск.

mod common;

use std::time::{Duration, Instant};

use common::{draft, settings, trust_each_other, Device2, MASTER_PASSWORD};
use syncra_core::{Core, RecordMeta, SyncPhase};

/// Пароль второго устройства. Не тот же самый — в этом весь смысл (см. шапку).
const OTHER_PASSWORD: &str = "мастер-пароль-2";

/// Дождаться, пока условие станет правдой. Дольше нескольких кругов здесь
/// ничего не происходит; если происходит — тест обязан упасть, а не висеть.
#[track_caller]
fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("так и не дождались: {what}");
}

fn records(device: &Device2) -> Vec<RecordMeta> {
    device.with_core(|core| core.list_records(None, true).unwrap())
}

fn find(device: &Device2, record_id: &str) -> Option<RecordMeta> {
    records(device)
        .into_iter()
        .find(|record| record.record_id == record_id)
}

/// Сопрячь два ядра, **не** давая им адресов друг друга.
///
/// Так выглядит сопряжённое, но недоступное устройство: телефон унесли, ноутбук
/// закрыли. Записи при этом обязаны числиться ждущими отправки — им есть куда
/// ехать, просто некому доставить.
fn trust_offline(a: &Device2, b: &Device2) {
    for (left, right) in [(a, b), (b, a)] {
        let payload = right.with_core(|core| {
            core.get_pairing_payload().unwrap();
            core.shown_pairing_payload().unwrap().to_owned()
        });
        let handshake = left
            .with_core(|core| core.submit_paired_key(&payload))
            .unwrap();
        left.with_core(|core| core.confirm_pairing(&handshake.session_id))
            .unwrap();
    }
}

fn introduce(a: &Device2, b: &Device2) {
    a.node.seed_peer(b.addr());
    b.node.seed_peer(a.addr());
}

// ---------------------------------------------------------------------------
// Сходимость
// ---------------------------------------------------------------------------

#[test]
fn two_cores_converge_and_the_secret_survives_the_crossing() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", OTHER_PASSWORD);
    trust_each_other(&a, &b);

    let mut item = draft("GitHub", "octocat", "  пробелы по краям  ");
    item.notes = Some("вторая почта".to_owned());
    let created = a.with_core(|core| core.create_record(&item).unwrap());

    wait_until(
        "запись доехала до второго устройства",
        || find(&b, &created.record_id).is_some(),
    );

    let mirrored = find(&b, &created.record_id).unwrap();
    assert_eq!(mirrored.service_name, "GitHub");
    assert_eq!(mirrored.login, "octocat");
    assert_eq!(mirrored.version, created.version);
    assert!(mirrored.has_notes);

    // Вот ради этого всё и затевалось: пароль запечатан ЧУЖИМ ключом хранилища,
    // выведенным из другого мастер-пароля, и читается — значит, его правда
    // перешифровали, а не скопировали шифротекст (§3.2).
    let secrets = b.with_core(|core| core.get_secret(&created.record_id).unwrap());
    assert_eq!(secrets.password, "  пробелы по краям  ");
    assert_eq!(secrets.notes.as_deref(), Some("вторая почта"));

    // Секция приехала вместе с записью — под тем же идентификатором, иначе одна
    // и та же папка разъехалась бы на две.
    let vaults = b.with_core(|core| core.list_vaults().unwrap());
    assert!(vaults
        .iter()
        .any(|vault| vault.vault_id == created.vault_id && vault.sync));
}

#[test]
fn a_change_travels_back_the_other_way() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", OTHER_PASSWORD);
    trust_each_other(&a, &b);

    let created = a.with_core(|core| {
        core.create_record(&draft("Notion", "me", "старый"))
            .unwrap()
    });
    wait_until("запись доехала", || {
        find(&b, &created.record_id).is_some()
    });

    // Правим на ВТОРОМ устройстве — обмен симметричен (§5.3).
    let patch = syncra_core::RecordPatch {
        password: Some("новый".to_owned()),
        ..Default::default()
    };
    let updated = b.with_core(|core| core.update_record(&created.record_id, &patch).unwrap());
    assert!(updated.version > created.version);

    wait_until("правка вернулась", || {
        find(&a, &created.record_id).is_some_and(|record| record.version == updated.version)
    });
    let secrets = a.with_core(|core| core.get_secret(&created.record_id).unwrap());
    assert_eq!(secrets.password, "новый");
}

// ---------------------------------------------------------------------------
// Локальные секции (§4.2)
// ---------------------------------------------------------------------------

#[test]
fn a_local_section_does_not_travel_in_either_direction() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", OTHER_PASSWORD);
    trust_each_other(&a, &b);

    let local = a.with_core(|core| {
        let vault = core.create_vault("Только здесь", "amber").unwrap();
        core.set_vault_sync(&vault.vault_id, false).unwrap()
    });

    let mut item = draft("Банк", "ivan", "секрет");
    item.vault_id = Some(local.vault_id.clone());
    let hidden = a.with_core(|core| core.create_record(&item).unwrap());
    let shared = a.with_core(|core| {
        core.create_record(&draft("Slack", "ivan", "пароль"))
            .unwrap()
    });

    // Ждём соседнюю запись: если доехала она, круг прошёл целиком.
    wait_until("синкаемая запись доехала", || {
        find(&b, &shared.record_id).is_some()
    });

    assert!(
        find(&b, &hidden.record_id).is_none(),
        "запись локальной секции уехала на второе устройство"
    );
    let vaults = b.with_core(|core| core.list_vaults().unwrap());
    assert!(
        !vaults.iter().any(|vault| vault.vault_id == local.vault_id),
        "локальная секция уехала на второе устройство"
    );

    // ...и обратно её тоже никто не просит: собеседник знает, что эта секция
    // здесь локальная, и в `pending` она не висит.
    assert!(!a.node.status().pending_records.contains(&hidden.record_id));

    // Ещё несколько кругов — и ничего не изменилось.
    std::thread::sleep(Duration::from_millis(700));
    assert!(find(&b, &hidden.record_id).is_none());
    assert!(find(&a, &hidden.record_id).is_some());
}

#[test]
fn turning_a_shared_section_local_stops_the_flow_and_leaves_nothing_hanging() {
    // Ради этого случая в манифесте и есть строки локальных секций, помеченные
    // «не поедет»: без них сосед видел бы только «у него этой записи нет» и слал
    // бы её заново каждую минуту навсегда.
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", OTHER_PASSWORD);
    trust_each_other(&a, &b);

    let created = a.with_core(|core| {
        core.create_record(&draft("Linear", "me", "пароль"))
            .unwrap()
    });
    wait_until("запись доехала", || {
        find(&b, &created.record_id).is_some()
    });
    wait_until("список ожидающих опустел", || {
        a.node.status().pending_records.is_empty()
    });

    // Человек выключил синхронизацию секции уже после того, как она разъехалась.
    a.with_core(|core| core.set_vault_sync(&created.vault_id, false).unwrap());

    // Правка теперь никуда не едет — и в «ждут отправки» не висит: ждать её
    // некому, секция локальная (§4.2).
    let patch = syncra_core::RecordPatch {
        login: Some("никому-не-видно".to_owned()),
        ..Default::default()
    };
    a.with_core(|core| core.update_record(&created.record_id, &patch).unwrap());
    assert!(a.node.status().pending_records.is_empty());

    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(find(&b, &created.record_id).unwrap().login, "me");

    // И у второго устройства ничего не зависло: оно знает, что запись там есть,
    // просто никуда не поедет, — и своей копии обратно не навязывает.
    assert!(b.node.status().pending_records.is_empty());
    assert_eq!(
        find(&a, &created.record_id).unwrap().login,
        "никому-не-видно",
        "местную правку перезаписала приехавшая копия"
    );
}

// ---------------------------------------------------------------------------
// Надгробия (§5.4)
// ---------------------------------------------------------------------------

#[test]
fn a_tombstone_survives_the_meeting_and_the_record_does_not_come_back() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", OTHER_PASSWORD);
    trust_each_other(&a, &b);

    let created = a.with_core(|core| {
        core.create_record(&draft("Dropbox", "me", "пароль"))
            .unwrap()
    });
    wait_until("запись доехала", || {
        find(&b, &created.record_id).is_some()
    });

    let deleted = a.with_core(|core| core.delete_record(&created.record_id).unwrap());
    assert!(deleted.deleted_at.is_some());

    wait_until("надгробие доехало", || {
        find(&b, &created.record_id).is_some_and(|record| record.deleted_at.is_some())
    });

    // Живой запись на втором устройстве больше не считается...
    let live = b.with_core(|core| core.list_records(None, false).unwrap());
    assert!(live
        .iter()
        .all(|record| record.record_id != created.record_id));
    // ...и секреты у надгробия стёрты, а не просто спрятаны.
    let mirrored = find(&b, &created.record_id).unwrap();
    assert!(!mirrored.has_notes && !mirrored.has_totp);

    // Несколько кругов спустя запись не воскресла ни на одной стороне: это и
    // есть та поломка, ради которой надгробия вообще существуют.
    std::thread::sleep(Duration::from_millis(700));
    assert!(find(&a, &created.record_id).unwrap().deleted_at.is_some());
    assert!(find(&b, &created.record_id).unwrap().deleted_at.is_some());
}

// ---------------------------------------------------------------------------
// Дифференциальность
// ---------------------------------------------------------------------------

#[test]
fn a_second_round_over_the_same_state_moves_nothing() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", OTHER_PASSWORD);
    trust_each_other(&a, &b);

    let created = a.with_core(|core| core.create_record(&draft("Figma", "me", "пароль")).unwrap());
    wait_until("запись доехала", || {
        find(&b, &created.record_id).is_some()
    });
    wait_until("список ожидающих опустел", || {
        a.node.status().pending_records.is_empty()
    });

    // Планы считаются по манифестам, как в настоящем круге, — только без
    // сокета. Пусто с обеих сторон значит, что следующий круг не повёз бы
    // ни одной записи (§5.3: «уже актуальные данные повторно не передаются»).
    let manifest_of = |device: &Device2| device.with_core(|core| core.sync_manifest()).unwrap();
    let plan_a = a.with_core(|core| core.sync_plan(&manifest_of(&b)).unwrap());
    let plan_b = b.with_core(|core| core.sync_plan(&manifest_of(&a)).unwrap());

    assert!(
        plan_a.send.is_empty() && plan_a.want.is_empty(),
        "{plan_a:?}"
    );
    assert!(
        plan_b.send.is_empty() && plan_b.want.is_empty(),
        "{plan_b:?}"
    );
    // Обе стороны при этом знают, на какой версии сошлись, — это общий предок
    // для разрешения конфликтов (§5.5).
    assert_eq!(plan_a.settled.len(), 1);
    assert_eq!(
        plan_a.settled[0],
        (created.record_id.clone(), created.version)
    );
}

// ---------------------------------------------------------------------------
// «Ждут отправки»
// ---------------------------------------------------------------------------

#[test]
fn pending_records_wait_for_somebody_to_carry_them() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", OTHER_PASSWORD);

    let created = a.with_core(|core| {
        core.create_record(&draft("Trello", "me", "пароль"))
            .unwrap()
    });
    // Сопряжённых устройств нет — ехать записи некуда, и «ждёт отправки» было бы
    // обещанием отправки, которой не будет.
    assert!(a.node.status().pending_records.is_empty());

    trust_offline(&a, &b);
    assert_eq!(
        a.node.status().pending_records,
        vec![created.record_id.clone()]
    );

    introduce(&a, &b);
    wait_until(
        "запись доехала и перестала ждать",
        || a.node.status().pending_records.is_empty(),
    );
    assert!(find(&b, &created.record_id).is_some());

    // Новая правка снова становится ждущей — до следующего круга.
    let status = a.node.status();
    assert_eq!(status.phase, SyncPhase::Idle);
    assert!(status.last_sync_at.is_some());
}

#[test]
fn a_revoked_device_is_nobody_to_wait_for() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", OTHER_PASSWORD);
    trust_each_other(&a, &b);

    let peer = a.peers().into_iter().next().expect("сосед в списке");
    a.with_core(|core| core.revoke_device(&peer.device_id).unwrap());

    let created = a.with_core(|core| core.create_record(&draft("Jira", "me", "пароль")).unwrap());

    // Отозванному устройству записи не возят (§2.3), а других устройств нет —
    // значит и ждать некому.
    assert!(a.node.status().pending_records.is_empty());
    std::thread::sleep(Duration::from_millis(700));
    assert!(
        find(&b, &created.record_id).is_none(),
        "запись уехала на отозванное устройство"
    );
}

// ---------------------------------------------------------------------------
// Первичный перенос при сопряжении (хвост S2)
// ---------------------------------------------------------------------------

#[test]
fn confirming_the_pairing_carries_the_vault_over() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", OTHER_PASSWORD);

    let expected = 3;
    for index in 0..expected {
        a.with_core(|core| {
            core.create_record(&draft(&format!("Сервис{index}"), "me", "пароль"))
                .unwrap()
        });
    }
    // Запись локальной секции в счёт не идёт: выключенная синхронизация означает
    // «никуда», в том числе на только что сопряжённое устройство (§4.2).
    a.with_core(|core| {
        let vault = core.create_vault("Только здесь", "coral").unwrap();
        core.set_vault_sync(&vault.vault_id, false).unwrap();
        let mut item = draft("Банк", "ivan", "секрет");
        item.vault_id = Some(vault.vault_id);
        core.create_record(&item).unwrap()
    });

    // Человек прочитал код с экрана A на устройстве B и подтвердил слова.
    let payload = a.with_core(|core| {
        core.get_pairing_payload().unwrap();
        core.shown_pairing_payload().unwrap().to_owned()
    });
    let handshake = b
        .with_core(|core| core.submit_paired_key(&payload))
        .unwrap();
    b.node.seed_peer(a.addr());

    let result = b.node.confirm_pairing(&handshake.session_id).unwrap();

    assert_eq!(result.device.name, "Ноутбук");
    assert_eq!(
        result.records_transferred, expected,
        "первичный перенос не состоялся"
    );
    assert_eq!(
        b.with_core(|core| core.list_records(None, false).unwrap())
            .len(),
        expected as usize
    );
}

#[test]
fn a_settings_change_does_not_break_the_stand() {
    // Часовой: круг обмена трогает и `vaults`, и `records`, и `meta`, и если
    // соседние подсистемы от этого поедут, заметить это должно здесь, а не в S5.
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", OTHER_PASSWORD);
    trust_each_other(&a, &b);

    let created = a.with_core(|core| core.create_record(&draft("Miro", "me", "пароль")).unwrap());
    wait_until("запись доехала", || {
        find(&b, &created.record_id).is_some()
    });

    // Смена мастер-пароля перешифровывает всё хранилище — включая только что
    // приехавшую запись.
    b.with_core(|core| {
        core.change_master_password(OTHER_PASSWORD, "мастер-пароль-3")
            .unwrap()
    });
    let secrets = b.with_core(|core| core.get_secret(&created.record_id).unwrap());
    assert_eq!(secrets.password, "пароль");

    // ...и обмен после этого продолжает работать: идентичность устройства
    // перешифровалась вместе с записями.
    let patch = syncra_core::RecordPatch {
        login: Some("другой".to_owned()),
        ..Default::default()
    };
    let updated = b.with_core(|core| core.update_record(&created.record_id, &patch).unwrap());
    wait_until(
        "правка после смены пароля доехала",
        || find(&a, &created.record_id).is_some_and(|record| record.version == updated.version),
    );
    assert_eq!(find(&a, &created.record_id).unwrap().login, "другой");
}

// ---------------------------------------------------------------------------
// Схема
// ---------------------------------------------------------------------------

/// Хранилище схемы 2 создавалось без таблицы `sync_state`.
///
/// Открытие таким хранилищем не должно ни падать, ни терять записи, а обмен
/// после миграции обязан работать с нуля: отметок в файле не было, значит всё
/// содержимое ещё нигде не побывало.
#[test]
fn a_schema_2_vault_migrates_without_losing_anything() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    let record_id = {
        let mut core = Core::open(&path, common::host()).unwrap();
        core.init_vault(MASTER_PASSWORD).unwrap();
        core.create_record(&draft("GitHub", "octocat", "тайна-1"))
            .unwrap()
            .record_id
    };

    // Откатываем файл до схемы 2 мимо ядра — ровно то, что лежит на диске у
    // человека, поставившего прошлую версию.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "DROP TABLE sync_state;
             UPDATE meta SET value = CAST('2' AS BLOB) WHERE key = 'schema_version';",
        )
        .unwrap();
    }

    let mut core = Core::open(&path, common::host()).unwrap();
    core.unlock(MASTER_PASSWORD).unwrap();

    assert_eq!(core.list_records(None, false).unwrap().len(), 1);
    assert_eq!(core.get_secret(&record_id).unwrap().password, "тайна-1");

    // Таблица досоздалась и работает: пустой манифест и пустые отметки — это
    // «ещё ни с кем не сходились», а не «всё уже уехало».
    assert_eq!(core.sync_manifest().unwrap().entries.len(), 1);
    assert!(core.pending_records().unwrap().is_empty());
    assert!(core.last_sync_at().unwrap().is_none());
}

#[test]
fn settings_of_the_stand_are_the_tight_ones() {
    // Страховка от невнимательности: если стенд начнёт ходить с боевыми
    // сроками, тесты обмена станут ждать по минуте и их выключат.
    assert!(settings().sync_interval < Duration::from_secs(1));
}
