use serde_json::Value;

/// Print JSON output. If `tagged` is true, print each key on its own line (p4 -ztag style).
pub fn print_json(value: &Value, tagged: bool) {
    if tagged {
        print_tagged(value, "");
    } else {
        println!("{}", serde_json::to_string_pretty(value).unwrap_or_default());
    }
}

fn print_tagged(value: &Value, prefix: &str) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}{k}")
                };
                match v {
                    Value::Array(arr) => {
                        for (i, item) in arr.iter().enumerate() {
                            print_tagged(item, &format!("{key}{i} "));
                        }
                    }
                    Value::Object(_) => print_tagged(v, &format!("{key} ")),
                    _ => println!("... {key} {v}"),
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                print_tagged(item, prefix);
            }
        }
        Value::String(s) => println!("... {prefix}{s}"),
        _ => println!("... {prefix}{value}"),
    }
}

/// Print a simple table: header row followed by data rows.
pub fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    if rows.is_empty() {
        return;
    }
    // Compute column widths
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }
    // Print header
    let header_line: String = headers
        .iter()
        .enumerate()
        .map(|(i, h)| format!("{:<width$}", h, width = widths[i]))
        .collect::<Vec<_>>()
        .join("  ");
    println!("{header_line}");
    // Print rows
    for row in rows {
        let line: String = row
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{:<width$}", c, width = widths.get(i).copied().unwrap_or(10)))
            .collect::<Vec<_>>()
            .join("  ");
        println!("{line}");
    }
}

/// Print a simple info line (key: value).
pub fn print_info(key: &str, value: &str) {
    println!("{key}: {value}");
}
