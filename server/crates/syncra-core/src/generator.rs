//! Генератор паролей (F6, §6.1, §8.3 «Generator»).
//!
//! Правила пользователь настраивает ОДИН РАЗ — профиль лежит в хранилище, а не
//! приезжает с каждым запросом (§6.1). Разовый профиль в `generate_passwords`
//! существует ради предпросмотра на экране настроек и сохранённый не меняет.
//!
//! Исполняемая спецификация поведения — `client/src/core/mock/generator.ts`:
//! алфавиты, формулы энтропии и тексты ошибок здесь повторяют её намеренно.
//!
//! Случайность берётся у ОС через [`crypto::random_bytes`]. Своего генератора
//! псевдослучайных чисел у ядра нет и не будет: предсказуемая последовательность,
//! выданная за пароль, — это худшее, что генератор может сделать.

use serde::{Deserialize, Serialize};

use crate::crypto;
use crate::error::{CoreError, CoreResult};

/// `GENERATOR_LIMITS.length` (`contract.ts:483`).
pub const LENGTH_MIN: i64 = 8;
pub const LENGTH_MAX: i64 = 40;
/// `GENERATOR_LIMITS.words`.
pub const WORDS_MIN: i64 = 3;
pub const WORDS_MAX: i64 = 7;
/// `GENERATOR_LIMITS.count` — сколько вариантов можно попросить за раз.
pub const COUNT_MIN: i64 = 1;
pub const COUNT_MAX: i64 = 10;

/// Буквы без похожих начертаний: ни `l`, ни `I`, ни `O`.
const LETTERS_CLEAR: &str = "abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ";
/// Те самые спорные символы — добавляются, когда фильтр выключен.
const LETTERS_AMBIGUOUS: &str = "iloIO";
const DIGITS_CLEAR: &str = "23456789";
const DIGITS_AMBIGUOUS: &str = "01";
const SYMBOLS: &str = "!@#$%^&*-_=+?";

/// Диапазон приписываемого к фразе числа — двузначное, как в макете.
const APPENDED_NUMBER_MIN: u32 = 10;
const APPENDED_NUMBER_RANGE: u32 = 90;

/// Словарь режима `words`. Русские слова — за фейк-ядром (`mock/generator.ts:34`),
/// на котором собран весь фронт; латинский список можно добавить отдельным
/// режимом, если окажется, что сайты не принимают кириллицу в поле пароля.
const WORDS_SOURCE: &str = include_str!("generator/words.ru.txt");

/// Словарь виден и модулю доверия: отпечаток устройства (§2.2) складывается из
/// тех же слов, и заводить под него второй список русских слов незачем.
pub(crate) fn words() -> &'static [&'static str] {
    static WORDS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    WORDS.get_or_init(|| {
        WORDS_SOURCE
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect()
    })
}

// ---------------------------------------------------------------------------
// Профиль на проводе
// ---------------------------------------------------------------------------

/// Сохранённый профиль генерации (`GeneratorProfile`, `contract.ts:458`).
///
/// `mode` и `separator` — строки, а числа знаковые, намеренно: всё, что пришло
/// снаружи, должно доехать до нашей проверки и получить `VALIDATION` с внятным
/// текстом. Строгие типы отвергали бы такое ещё в serde, и до UI доезжала бы
/// «непредвиденная ошибка ядра» вместо объяснения (`errors.ts:33`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratorProfile {
    pub mode: String,
    pub length: i64,
    pub digits: bool,
    pub symbols: bool,
    pub avoid_ambiguous: bool,
    pub words: i64,
    pub separator: String,
    pub append_number: bool,
}

impl Default for GeneratorProfile {
    /// Профиль, с которым ядро отдаёт свежесозданное хранилище
    /// (`DEFAULT_GENERATOR_PROFILE`, `mock/generator.ts:60`).
    fn default() -> Self {
        Self {
            mode: "chars".to_owned(),
            length: 20,
            digits: true,
            symbols: true,
            avoid_ambiguous: true,
            words: 4,
            separator: "-".to_owned(),
            append_number: false,
        }
    }
}

/// Ответ `generate_passwords` (`contract.ts:517`).
///
/// ВНИМАНИЕ: единственный ответ ядра кроме `get_secret`, несущий пароли открытым
/// текстом. Это ещё ничей пароль — но обращение с ним такое же.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GeneratedPasswords {
    pub passwords: Vec<String>,
    /// Оценка стойкости. Считается по тому же алфавиту/словарю, которым пароль
    /// на самом деле собран, — иначе цифра рядом с паролем была бы украшением.
    pub entropy_bits: i64,
}

// ---------------------------------------------------------------------------
// Проверенные правила
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Chars,
    Words,
}

/// Профиль после проверки. Дальше по коду ходит только он: из этого типа уже
/// нельзя достать ни неизвестный режим, ни длину вне границ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    mode: Mode,
    length: usize,
    digits: bool,
    symbols: bool,
    avoid_ambiguous: bool,
    words: usize,
    separator: char,
    append_number: bool,
}

impl Rules {
    /// Проверяет профиль так же придирчиво, как чужой ввод, — потому что это он
    /// и есть: UI мог прислать что угодно.
    pub fn parse(profile: &GeneratorProfile) -> CoreResult<Self> {
        let mode = match profile.mode.as_str() {
            "chars" => Mode::Chars,
            "words" => Mode::Words,
            _ => return Err(CoreError::validation("Неизвестный режим генерации.")),
        };

        if !(LENGTH_MIN..=LENGTH_MAX).contains(&profile.length) {
            return Err(CoreError::validation(format!(
                "Длина пароля — от {LENGTH_MIN} до {LENGTH_MAX} символов."
            )));
        }
        if !(WORDS_MIN..=WORDS_MAX).contains(&profile.words) {
            return Err(CoreError::validation(format!(
                "Слов во фразе — от {WORDS_MIN} до {WORDS_MAX}."
            )));
        }

        // Оба режима проверяются всегда, даже неактивный: профиль хранит настройки
        // обоих, и переключение режима не должно вдруг открывать негодные правила.
        let separator = match profile.separator.as_str() {
            "-" => '-',
            "." => '.',
            " " => ' ',
            _ => return Err(CoreError::validation("Недопустимый разделитель слов.")),
        };

        Ok(Self {
            mode,
            length: profile.length as usize,
            digits: profile.digits,
            symbols: profile.symbols,
            avoid_ambiguous: profile.avoid_ambiguous,
            words: profile.words as usize,
            separator,
            append_number: profile.append_number,
        })
    }

    /// Обратно на провод: ядро возвращает профиль после нормализации, и показывает
    /// UI именно его (`contract.ts:500`).
    pub fn to_profile(&self) -> GeneratorProfile {
        GeneratorProfile {
            mode: match self.mode {
                Mode::Chars => "chars".to_owned(),
                Mode::Words => "words".to_owned(),
            },
            length: self.length as i64,
            digits: self.digits,
            symbols: self.symbols,
            avoid_ambiguous: self.avoid_ambiguous,
            words: self.words as i64,
            separator: self.separator.to_string(),
            append_number: self.append_number,
        }
    }

    /// Алфавит режима `chars`. Буквы есть всегда — профиля с пустым алфавитом
    /// не существует.
    fn alphabet(&self) -> Vec<char> {
        let mut chars: Vec<char> = LETTERS_CLEAR.chars().collect();
        if !self.avoid_ambiguous {
            chars.extend(LETTERS_AMBIGUOUS.chars());
        }
        if self.digits {
            chars.extend(DIGITS_CLEAR.chars());
            if !self.avoid_ambiguous {
                chars.extend(DIGITS_AMBIGUOUS.chars());
            }
        }
        if self.symbols {
            chars.extend(SYMBOLS.chars());
        }
        chars
    }

    /// Оценка стойкости в битах — по реальному алфавиту и реальному словарю.
    pub fn entropy_bits(&self) -> i64 {
        let bits = match self.mode {
            Mode::Chars => self.length as f64 * (self.alphabet().len() as f64).log2(),
            Mode::Words => {
                let from_words = self.words as f64 * (words().len() as f64).log2();
                let from_number = if self.append_number {
                    (APPENDED_NUMBER_RANGE as f64).log2()
                } else {
                    0.0
                };
                from_words + from_number
            }
        };
        bits.round() as i64
    }

    fn generate_one(&self) -> CoreResult<String> {
        match self.mode {
            Mode::Chars => {
                let alphabet = self.alphabet();
                let mut password = String::with_capacity(self.length);
                for _ in 0..self.length {
                    password.push(alphabet[random_index(alphabet.len())?]);
                }
                Ok(password)
            }
            Mode::Words => {
                let bank = words();
                let mut parts: Vec<String> = Vec::with_capacity(self.words + 1);
                for _ in 0..self.words {
                    parts.push(bank[random_index(bank.len())?].to_owned());
                }
                if self.append_number {
                    let number =
                        random_index(APPENDED_NUMBER_RANGE as usize)? as u32 + APPENDED_NUMBER_MIN;
                    parts.push(number.to_string());
                }
                Ok(parts.join(&self.separator.to_string()))
            }
        }
    }
}

/// `count` вариантов по уже проверенным правилам.
pub fn generate(rules: &Rules, count: i64) -> CoreResult<GeneratedPasswords> {
    if !(COUNT_MIN..=COUNT_MAX).contains(&count) {
        return Err(CoreError::validation(format!(
            "Вариантов можно попросить от {COUNT_MIN} до {COUNT_MAX}."
        )));
    }

    let mut passwords = Vec::with_capacity(count as usize);
    for _ in 0..count {
        passwords.push(rules.generate_one()?);
    }

    Ok(GeneratedPasswords {
        passwords,
        entropy_bits: rules.entropy_bits(),
    })
}

/// Равномерное целое из `[0, max_exclusive)`.
///
/// Через отбраковку хвоста, а не через остаток от деления: `% max` даёт перекос
/// в сторону младших вариантов, и алфавит перестаёт быть равновероятным. Для
/// генератора паролей это прямая потеря стойкости — ровно той, про которую мы
/// рядом печатаем «≈ N бит».
fn random_index(max_exclusive: usize) -> CoreResult<usize> {
    debug_assert!(max_exclusive > 0);
    let max = max_exclusive as u64;
    let limit = (u32::MAX as u64 + 1) / max * max;

    loop {
        let value = u32::from_le_bytes(crypto::random_bytes::<4>()?) as u64;
        if value < limit {
            return Ok((value % max) as usize);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(profile: GeneratorProfile) -> Rules {
        Rules::parse(&profile).expect("профиль")
    }

    fn words_profile() -> GeneratorProfile {
        GeneratorProfile {
            mode: "words".to_owned(),
            ..GeneratorProfile::default()
        }
    }

    #[test]
    fn wordlist_has_no_duplicates() {
        let bank = words();
        let unique: std::collections::BTreeSet<_> = bank.iter().collect();

        // Повтор в словаре тихо снизил бы настоящую энтропию фразы, а `entropy_bits`
        // об этом не узнала бы: она считает по длине списка.
        assert_eq!(unique.len(), bank.len(), "в словаре есть повторы");
        // Ниже этого списка фраза из четырёх слов перестаёт быть серьёзной.
        assert!(bank.len() >= 1024, "словарь слишком мал: {}", bank.len());
    }

    #[test]
    fn wordlist_has_no_separator_characters() {
        // Слово с дефисом, точкой или пробелом внутри разваливало бы фразу на
        // слух и глазом: тот же символ склеивает слова снаружи.
        for word in words() {
            assert!(
                !word.contains(['-', '.', ' ']),
                "слово «{word}» содержит разделитель"
            );
            assert_eq!(word.to_lowercase(), *word, "слово «{word}» не строчное");
        }
    }

    #[test]
    fn alphabet_follows_the_profile() {
        let ambiguous = rules(GeneratorProfile {
            avoid_ambiguous: false,
            ..GeneratorProfile::default()
        })
        .alphabet();
        assert!(ambiguous.contains(&'O') && ambiguous.contains(&'0'));

        let clear = rules(GeneratorProfile::default()).alphabet();
        for char in ['0', 'O', '1', 'l', 'I'] {
            assert!(!clear.contains(&char), "спорный символ {char} в алфавите");
        }

        let letters_only = rules(GeneratorProfile {
            digits: false,
            symbols: false,
            ..GeneratorProfile::default()
        })
        .alphabet();
        assert_eq!(letters_only.len(), LETTERS_CLEAR.chars().count());
    }

    #[test]
    fn chars_password_has_exact_length_and_known_alphabet() {
        let rules = rules(GeneratorProfile {
            length: 32,
            ..GeneratorProfile::default()
        });
        let alphabet = rules.alphabet();

        for password in generate(&rules, 10).unwrap().passwords {
            assert_eq!(password.chars().count(), 32);
            assert!(password.chars().all(|char| alphabet.contains(&char)));
        }
    }

    #[test]
    fn words_password_is_built_from_the_dictionary() {
        let rules = rules(GeneratorProfile {
            words: 5,
            append_number: true,
            separator: ".".to_owned(),
            ..words_profile()
        });

        let password = generate(&rules, 1).unwrap().passwords.remove(0);
        let parts: Vec<&str> = password.split('.').collect();

        assert_eq!(parts.len(), 6, "пять слов и число");
        assert!(parts[..5].iter().all(|part| words().contains(part)));
        let number: u32 = parts[5].parse().expect("число в конце");
        assert!(
            (APPENDED_NUMBER_MIN..APPENDED_NUMBER_MIN + APPENDED_NUMBER_RANGE).contains(&number)
        );
    }

    #[test]
    fn entropy_matches_the_alphabet_and_the_dictionary() {
        let chars = rules(GeneratorProfile::default());
        let expected = (20.0 * (chars.alphabet().len() as f64).log2()).round() as i64;
        assert_eq!(chars.entropy_bits(), expected);

        let phrase = rules(words_profile());
        assert_eq!(
            phrase.entropy_bits(),
            (4.0 * (words().len() as f64).log2()).round() as i64
        );

        // Приписанное число добавляет ровно те биты, что даёт его диапазон.
        let with_number = rules(GeneratorProfile {
            append_number: true,
            ..words_profile()
        });
        assert!(with_number.entropy_bits() > phrase.entropy_bits());
    }

    #[test]
    fn two_runs_do_not_repeat_themselves() {
        let rules = rules(GeneratorProfile::default());
        let batch = generate(&rules, 10).unwrap().passwords;

        let unique: std::collections::BTreeSet<_> = batch.iter().collect();
        assert_eq!(unique.len(), batch.len(), "варианты повторяются");
    }

    #[test]
    fn random_index_covers_its_whole_range() {
        // Не проверка равномерности (для неё нужна статистика), а защита от
        // сдвига на единицу: значение вне диапазона — сразу выход за алфавит.
        let mut seen = [false; 5];
        for _ in 0..500 {
            seen[random_index(5).unwrap()] = true;
        }
        assert!(seen.iter().all(|hit| *hit));
    }
}
