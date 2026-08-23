//! Минимальный CSV: чтение и запись (§6.2).
//!
//! Свой, а не крейт, по той же причине, по какой в `[workspace.dependencies]`
//! не досыпают: список curated, а нужного здесь — полторы страницы правил из
//! RFC 4180. Криптографии тут нет, изобретать нечего.
//!
//! Что поддерживается: кавычки вокруг поля, удвоенная кавычка внутри него,
//! запятая и перевод строки внутри кавычек, `CRLF` и `LF` вперемешку, BOM в
//! начале файла (его ставит Excel и почти всё, что экспортирует пароли под
//! Windows). Чего нет: смены разделителя и кодировок, кроме UTF-8 — файл,
//! который не UTF-8, отвергается выше, при чтении.

/// Разобрать таблицу целиком. Пустых строк в ответе нет: экспорт чужого
/// менеджера кончается переводом строки, и «последняя пустая запись» — это
/// артефакт формата, а не строка файла.
pub fn parse(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    // Пустое поле и поле `""` неразличимы по содержимому, но `""` — это
    // настоящее поле, а голая пустая строка в конце файла — нет.
    let mut had_quotes = false;

    let mut chars = strip_bom(text).chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            match ch {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                _ => field.push(ch),
            }
            continue;
        }

        match ch {
            '"' => {
                quoted = true;
                had_quotes = true;
            }
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {
                // `\r\n` и одинокий `\r` (старые экспорты с macOS) — один конец
                // строки, а не два.
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                finish_row(&mut rows, &mut row, &mut field, &mut had_quotes);
            }
            '\n' => finish_row(&mut rows, &mut row, &mut field, &mut had_quotes),
            _ => field.push(ch),
        }
    }

    finish_row(&mut rows, &mut row, &mut field, &mut had_quotes);
    rows
}

fn finish_row(
    rows: &mut Vec<Vec<String>>,
    row: &mut Vec<String>,
    field: &mut String,
    had_quotes: &mut bool,
) {
    let empty_tail = row.is_empty() && field.is_empty() && !*had_quotes;
    row.push(std::mem::take(field));
    *had_quotes = false;
    if empty_tail {
        row.clear();
        return;
    }
    rows.push(std::mem::take(row));
}

/// Собрать таблицу обратно. Конец строки — `CRLF`: так требует RFC 4180, и так
/// файл открывается в Excel без единой строки в одну.
pub fn write(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for row in rows {
        for (index, field) in row.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&escape(field));
        }
        out.push_str("\r\n");
    }
    out
}

/// Кавычки ставятся только там, где без них строка перестала бы читаться:
/// лишние кавычки вокруг каждого поля некоторые импортёры принимают за часть
/// значения.
fn escape(field: &str) -> String {
    let needs_quotes = field.contains([',', '"', '\r', '\n']);
    if !needs_quotes {
        return field.to_owned();
    }
    let mut out = String::with_capacity(field.len() + 2);
    out.push('"');
    for ch in field.chars() {
        if ch == '"' {
            out.push('"');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_table_reads_row_by_row() {
        assert_eq!(
            parse("name,url\nGitHub,github.com\n"),
            vec![
                vec!["name".to_owned(), "url".to_owned()],
                vec!["GitHub".to_owned(), "github.com".to_owned()],
            ]
        );
    }

    #[test]
    fn quotes_hide_commas_and_newlines() {
        let rows = parse("a,\"one,two\",\"строка\nвторая\"\n");
        assert_eq!(
            rows,
            vec![vec![
                "a".to_owned(),
                "one,two".to_owned(),
                "строка\nвторая".to_owned(),
            ]]
        );
    }

    #[test]
    fn a_doubled_quote_is_one_quote() {
        // Пароль вида `he said "hi"` — обычное дело, и терять кавычки нельзя.
        assert_eq!(
            parse("\"he said \"\"hi\"\"\"\n"),
            vec![vec!["he said \"hi\"".to_owned()]]
        );
    }

    #[test]
    fn crlf_and_lf_are_the_same_end_of_line() {
        assert_eq!(parse("a,b\r\nc,d\r\n").len(), 2);
        assert_eq!(parse("a,b\rc,d\r").len(), 2);
    }

    #[test]
    fn a_bom_is_not_part_of_the_first_column_name() {
        let rows = parse("\u{feff}name,url\n");
        assert_eq!(rows[0][0], "name");
    }

    #[test]
    fn an_empty_tail_is_not_a_row_but_an_empty_field_is() {
        // Файл кончается переводом строки — это не пустая запись.
        assert_eq!(parse("a\n").len(), 1);
        // А вот строка из одного пустого поля в кавычках — запись.
        assert_eq!(parse("a\n\"\"\n").len(), 2);
    }

    #[test]
    fn writing_and_reading_bring_back_the_same_table() {
        let table = vec![
            vec!["name".to_owned(), "note".to_owned()],
            vec![
                "Ozon".to_owned(),
                "запятая, кавычка \" и\r\nперевод строки".to_owned(),
            ],
        ];

        assert_eq!(parse(&write(&table)), table);
    }

    #[test]
    fn only_the_fields_that_need_quotes_get_them() {
        assert_eq!(write(&[vec!["простое".to_owned()]]), "простое\r\n");
        assert_eq!(write(&[vec!["с,запятой".to_owned()]]), "\"с,запятой\"\r\n");
    }
}
