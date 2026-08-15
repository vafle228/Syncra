//! Настройки безопасности (F13, «Настройки → Безопасность»).
//!
//! Три таймаута, между которыми выбирает пользователь. Живут они в ХРАНИЛИЩЕ, а
//! не в `localStorage` фронта, по двум причинам (`contract.ts:531`): автоблокировку
//! исполняет ядро, и настройка, от которой зависит, когда закроется хранилище, —
//! часть его защиты, а не оформление окна.
//!
//! Здесь только проверка и форма. Отсчёт бездействия ведёт [`crate::session::Core`].

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

/// Ступени из макета — ровно те, между которыми выбирает пользователь, и ничего
/// между ними (`contract.ts:563`). Свободный ввод был бы ложной свободой:
/// «автоблокировка через 3 секунды» — не настройка, а способ сломать себе работу.
pub const AUTOLOCK_OPTIONS_MS: [i64; 3] = [60_000, 300_000, 1_800_000];
pub const CLIPBOARD_CLEAR_OPTIONS_MS: [i64; 3] = [10_000, 20_000, 60_000];
pub const SECRET_REVEAL_OPTIONS_MS: [i64; 3] = [15_000, 30_000, 120_000];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecuritySettings {
    /// Через сколько бездействия ядро закрывает хранилище само.
    pub autolock_ms: i64,
    /// Через сколько скопированный пароль исчезает из буфера обмена.
    pub clipboard_clear_ms: i64,
    /// Через сколько открытый на экране секрет прячется обратно.
    pub secret_reveal_ms: i64,
}

impl Default for SecuritySettings {
    /// Средняя ступень каждой тройки (`DEFAULT_SECURITY_SETTINGS`, `contract.ts:574`).
    /// Это те же числа, которыми UI пользуется, пока ядро не ответило.
    fn default() -> Self {
        Self {
            autolock_ms: 300_000,
            clipboard_clear_ms: 20_000,
            secret_reveal_ms: 30_000,
        }
    }
}

/// Отсутствующее поле означает «не трогать»: экран меняет по одной настройке.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct SecuritySettingsPatch {
    #[serde(default)]
    pub autolock_ms: Option<i64>,
    #[serde(default)]
    pub clipboard_clear_ms: Option<i64>,
    #[serde(default)]
    pub secret_reveal_ms: Option<i64>,
}

impl SecuritySettings {
    /// Применить патч.
    ///
    /// Значение вне сетки — `VALIDATION`, а не «подберём ближайшее»: если UI
    /// прислал то, чего в макете нет, это его ошибка, и молча заменять её на
    /// похожее число значит скрыть расхождение вместо того, чтобы его показать.
    ///
    /// Патч применяется ЦЕЛИКОМ или никак — отсюда возврат нового значения, а не
    /// правка на месте: половина сохранённых настроек хуже, чем ни одной, потому
    /// что экран после этого показывал бы неправду.
    pub fn patched(&self, patch: &SecuritySettingsPatch) -> CoreResult<Self> {
        Ok(Self {
            autolock_ms: step(
                self.autolock_ms,
                patch.autolock_ms,
                &AUTOLOCK_OPTIONS_MS,
                "Автоблокировка",
            )?,
            clipboard_clear_ms: step(
                self.clipboard_clear_ms,
                patch.clipboard_clear_ms,
                &CLIPBOARD_CLEAR_OPTIONS_MS,
                "Очистка буфера",
            )?,
            secret_reveal_ms: step(
                self.secret_reveal_ms,
                patch.secret_reveal_ms,
                &SECRET_REVEAL_OPTIONS_MS,
                "Показ секрета",
            )?,
        })
    }
}

fn step(current: i64, requested: Option<i64>, options: &[i64], label: &str) -> CoreResult<i64> {
    let Some(value) = requested else {
        return Ok(current);
    };

    if !options.contains(&value) {
        return Err(CoreError::validation(format!(
            "Настройка «{label}»: значение {value} мс не из числа доступных."
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreErrorCode;

    #[test]
    fn patch_touches_only_what_it_names() {
        let settings = SecuritySettings::default();
        let next = settings
            .patched(&SecuritySettingsPatch {
                autolock_ms: Some(60_000),
                ..SecuritySettingsPatch::default()
            })
            .unwrap();

        assert_eq!(next.autolock_ms, 60_000);
        assert_eq!(next.clipboard_clear_ms, settings.clipboard_clear_ms);
        assert_eq!(next.secret_reveal_ms, settings.secret_reveal_ms);
    }

    #[test]
    fn value_outside_the_steps_is_rejected_whole() {
        let settings = SecuritySettings::default();
        let result = settings.patched(&SecuritySettingsPatch {
            // Первое поле годное, второе — нет: не должно примениться ни одно.
            autolock_ms: Some(60_000),
            clipboard_clear_ms: Some(3_000),
            secret_reveal_ms: None,
        });

        match result {
            Ok(value) => panic!("ждали VALIDATION, получили {value:?}"),
            Err(error) => assert_eq!(error.code, CoreErrorCode::Validation),
        }
        assert_eq!(settings, SecuritySettings::default());
    }

    #[test]
    fn defaults_are_the_middle_step() {
        let settings = SecuritySettings::default();
        assert_eq!(settings.autolock_ms, AUTOLOCK_OPTIONS_MS[1]);
        assert_eq!(settings.clipboard_clear_ms, CLIPBOARD_CLEAR_OPTIONS_MS[1]);
        assert_eq!(settings.secret_reveal_ms, SECRET_REVEAL_OPTIONS_MS[1]);
    }
}
