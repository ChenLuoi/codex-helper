use serde::Serialize;

pub fn to_pretty_json<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(value)
}

pub fn format_integer(value: i64) -> String {
    add_group_separators(&value.to_string())
}

pub fn round_credits(value: f64) -> f64 {
    ((value + f64::EPSILON) * 1_000_000.0).round() / 1_000_000.0
}

pub fn credits_to_usd(credits: f64) -> f64 {
    (((credits / 25.0) + f64::EPSILON) * 1_000_000.0).round() / 1_000_000.0
}

pub fn format_credits(value: f64) -> String {
    format_decimal_2(value)
}

pub fn format_usd(value: f64) -> String {
    format!("${}", format_decimal_2(value))
}

pub fn format_csv(rows: &[Vec<String>]) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|cell| escape_csv_cell(cell))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_markdown_table(rows: &[Vec<String>]) -> String {
    let Some((header, body)) = rows.split_first() else {
        return String::new();
    };

    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(markdown_row(header));
    lines.push(markdown_row(
        &header.iter().map(|_| "---".to_string()).collect::<Vec<_>>(),
    ));
    lines.extend(body.iter().map(|row| markdown_row(row)));
    lines.join("\n")
}

pub fn format_plain_table(rows: &[Vec<String>]) -> String {
    let Some(header) = rows.first() else {
        return String::new();
    };
    let widths = (0..header.len())
        .map(|column| {
            rows.iter()
                .map(|row| row.get(column).map(|cell| cell.len()).unwrap_or(0))
                .max()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();

    rows.iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(column, cell)| {
                    let width = widths[column];
                    if column == 0 {
                        format!("{cell:<width$}")
                    } else {
                        format!("{cell:>width$}")
                    }
                })
                .collect::<Vec<_>>()
                .join("  ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_decimal_2(value: f64) -> String {
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let formatted = format!("{:.2}", value.abs());
    let (integer, fractional) = formatted
        .split_once('.')
        .expect("formatted number has decimal point");
    format!("{sign}{}.{}", add_group_separators(integer), fractional)
}

fn add_group_separators(value: &str) -> String {
    let (sign, digits) = value
        .strip_prefix('-')
        .map_or(("", value), |rest| ("-", rest));
    let mut output = String::new();
    for (index, char) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            output.push(',');
        }
        output.push(char);
    }
    format!("{sign}{}", output.chars().rev().collect::<String>())
}

fn escape_csv_cell(value: &str) -> String {
    if value.contains('"') || value.contains(',') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn markdown_row(row: &[String]) -> String {
    format!(
        "| {} |",
        row.iter()
            .map(|cell| cell.replace('|', "\\|"))
            .collect::<Vec<_>>()
            .join(" | ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct JsonFixture {
        name: &'static str,
        calls: u32,
    }

    #[test]
    fn formats_numbers_like_the_typescript_cli() {
        assert_eq!(format_integer(1_234_567), "1,234,567");
        assert_eq!(format_credits(1234.5), "1,234.50");
        assert_eq!(format_usd(49.5), "$49.50");
        assert_eq!(round_credits(0.1234567), 0.123457);
        assert_eq!(credits_to_usd(25.0), 1.0);
    }

    #[test]
    fn formats_csv_and_markdown_with_escaping() {
        let rows = vec![
            vec!["Name".to_string(), "Value".to_string()],
            vec!["alpha,beta".to_string(), "x|y".to_string()],
            vec!["quote\"cell".to_string(), "line\nbreak".to_string()],
        ];

        assert_eq!(
            format_csv(&rows),
            "Name,Value\n\"alpha,beta\",x|y\n\"quote\"\"cell\",\"line\nbreak\""
        );
        assert_eq!(
            format_markdown_table(&rows),
            "| Name | Value |\n| --- | --- |\n| alpha,beta | x\\|y |\n| quote\"cell | line\nbreak |"
        );
    }

    #[test]
    fn formats_plain_table_with_right_aligned_numeric_columns() {
        let rows = vec![
            vec!["Group".to_string(), "Calls".to_string()],
            vec!["day".to_string(), "12".to_string()],
            vec!["month".to_string(), "1,234".to_string()],
        ];

        assert_eq!(
            format_plain_table(&rows),
            "Group  Calls\nday       12\nmonth  1,234"
        );
    }

    #[test]
    fn formats_pretty_json() {
        let json = to_pretty_json(&JsonFixture {
            name: "fixture",
            calls: 2,
        })
        .expect("json");

        assert_eq!(json, "{\n  \"name\": \"fixture\",\n  \"calls\": 2\n}");
    }
}
