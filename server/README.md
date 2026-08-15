# Syncra — Rust-ядро

Ядро и оболочка Syncra: крипта, хранилище и мост IPC к Vue-фронту из `../client`.

Источник истины по продукту — [`../syncra-spec.md`](../syncra-spec.md).
Договор с фронтом — [`../client/src/core/contract.ts`](../client/src/core/contract.ts).

## Что готово

Шаг 1: **жизненный цикл хранилища и CRUD**.

| Область | Команды |
|---|---|
| Замок (F3) | `get_vault_status`, `init_vault`, `unlock`, `lock` |
| Записи (F4, F5) | `list_records`, `get_secret`, `create_record`, `update_record`, `delete_record` |
| Секции (F7) | `list_vaults`, `create_vault`, `update_vault`, `set_vault_sync`, `set_default_vault`, `delete_vault` |

Синхронизация, сопряжение, конфликты, генератор, импорт/экспорт, TOTP и PIN — следующие шаги.
Их команды **зарегистрированы** и отвечают внятным отказом: незарегистрированную команду
Tauri отвергает строкой, которую фронт показал бы как «непредвиденную ошибку ядра».

## Раскладка

```
crates/
  syncra-core/   библиотека: модель, крипта, SQLite, состояние замка.
                 Про Tauri не знает — проверяется обычным `cargo test`.
  syncra-app/    оболочка Tauri 2: окно и обёртки команд. Логики нет.
```

## Как устроено хранилище

- SQLite (`rusqlite`, bundled). Файл — `syncra.db` в папке данных приложения.
- Секреты (`password`, `notes`, `totp_secret`) шифруются **по полям**:
  XChaCha20-Poly1305, ключ выводится из мастер-пароля через Argon2id.
- AAD каждого поля включает `record_id` и имя поля — шифротекст нельзя
  переставить в другую запись или на место другого поля.
- Мастер-пароль проверяется распечаткой контрольного значения; отдельного хеша
  пароля в файле нет.

> **Граница шага:** метаданные (`service_name`, `login`, `urls`) лежат в файле
> **открытым текстом**. Пополевое шифрование закрывает секреты, но не их. Тест
> `tests/at_rest.rs::metadata_is_deliberately_not_encrypted_in_this_step` держит
> это обещание видимым — когда появится шифрование файла целиком (SQLCipher или
> свой слой), он упадёт и потребует переписать себя вместе с обещанием.

## Требования к машине

- Rust stable (`rustup`), таргет `x86_64-pc-windows-msvc`.
- **MSVC Build Tools** (VS 2022, workload «Desktop development with C++») — нужны
  и для bundled SQLite, и для Tauri.
- WebView2 — есть в составе Windows 11.
- Для дев-петли: **Node ≥ 22.18** (требование `../client/package.json`).

## Команды

```sh
cargo test                                  # ядро целиком, без окна
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check

cargo tauri dev    --manifest-path crates/syncra-app/Cargo.toml   # окно + дев-сервер клиента
cargo tauri build  --manifest-path crates/syncra-app/Cargo.toml   # сборка против client/dist
```

`cargo tauri` ставится отдельно: `cargo install tauri-cli --version "^2"`.

### Мок или настоящее ядро

Переключатель живёт во фронте (`client/src/core/ipc.ts`, `resolveCoreMode()`):

- `npm run dev` — против мок-ядра (по умолчанию в деве);
- `npm run dev -- --mode tauri` — против этого ядра (через `client/.env.tauri`);
  ровно это и запускает `cargo tauri dev`;
- прод-сборка всегда идёт против настоящего ядра.

## Правила

- **Закон №1:** секреты не покидают ядро иначе как по явному действию человека.
  В этом шаге открытый текст отдаёт ровно одна команда — `get_secret`.
- Сообщения ошибок уходят прямо в UI: ни путей к хранилищу, ни ключей, ни
  текста ошибок SQLite в них быть не должно (`src/error.rs`).
- Поведение сверяется с фейк-ядром `../client/src/core/mock/index.ts`.
  Расхождение с ним — баг ядра, а не «другая трактовка».
