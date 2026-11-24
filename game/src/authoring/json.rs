//! The engine's one JSON layout in Rust: exactly what `tools/jsonio.py` `dump` writes (indent 2, a list of numbers on one line,
//! non-ASCII escaped as Python's `ensure_ascii` does, key order kept, a final newline), so a tool that saves a file leaves it
//! byte-for-byte as the importer would have written it, apart from what it changed.
//!
//! Known difference, by design: Python's writer also rewrites the whitespace of text that merely looks like a number list inside a
//! string value (`"[1,  2]"` becomes `"[1, 2]"`); this one never touches the inside of a string.

use serde_json::{Number, Value};
use std::fmt::Write;

/// `value` in the layout of `tools/jsonio.py`, ending with a newline. Key order is the order of the `Value` (the crate builds
/// `serde_json` with `preserve_order`).
pub fn to_json_string(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, 0, &mut out);
    out.push('\n');
    out
}

fn write_value(v: &Value, depth: usize, out: &mut String) {
    match v {
        Value::Array(a) if a.is_empty() => out.push_str("[]"),
        Value::Object(m) if m.is_empty() => out.push_str("{}"),
        // A list of numbers and nothing else stays on one line (`jsonio.NUM`); a list holding anything else, even a list, breaks.
        Value::Array(a) if a.iter().all(Value::is_number) => {
            out.push('[');
            for (i, n) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_value(n, depth, out);
            }
            out.push(']');
        }
        Value::Array(a) => {
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                indent(depth + 1, out);
                write_value(x, depth + 1, out);
                out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
            }
            indent(depth, out);
            out.push(']');
        }
        Value::Object(m) => {
            out.push_str("{\n");
            for (i, (k, x)) in m.iter().enumerate() {
                indent(depth + 1, out);
                write_string(k, out);
                out.push_str(": ");
                write_value(x, depth + 1, out);
                out.push_str(if i + 1 < m.len() { ",\n" } else { "\n" });
            }
            indent(depth, out);
            out.push('}');
        }
        Value::String(s) => write_string(s, out),
        Value::Number(n) => out.push_str(&number(n)),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Null => out.push_str("null"),
    }
}

fn indent(depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// A number as Python's `json.dumps` writes it. Integers are plain. A float has the shortest digits that read back (`repr`), in
/// fixed notation from 1e-4 up to 1e16 and in exponent notation (`1e-05`, `1.5e+16`: a sign and two digits at least) outside it;
/// serde's own text differs from that in both places.
fn number(n: &Number) -> String {
    match n.as_f64().filter(|_| n.is_f64()) {
        Some(f) => float(f),
        None => n.to_string(),
    }
}

fn float(f: f64) -> String {
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    // `{:e}` has the shortest round-trip digits: `-1.5e-7`.
    let text = format!("{f:e}");
    let (mantissa, exponent) = text.split_once('e').expect("LowerExp has an exponent");
    let exp: i32 = exponent.parse().expect("an integer exponent");
    let (sign, mantissa) = mantissa.strip_prefix('-').map_or(("", mantissa), |m| ("-", m));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    if !(-4..16).contains(&exp) {
        let lead = if digits.len() == 1 { digits.clone() } else { format!("{}.{}", &digits[..1], &digits[1..]) };
        format!("{sign}{lead}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    } else if exp < 0 {
        format!("{sign}0.{}{digits}", "0".repeat((-exp - 1) as usize))
    } else {
        let whole = exp as usize + 1;
        if digits.len() <= whole { format!("{sign}{digits}{}.0", "0".repeat(whole - digits.len())) } else { format!("{sign}{}.{}", &digits[..whole], &digits[whole..]) }
    }
}

/// A JSON string as Python's `json.dumps(ensure_ascii=True)` writes it: everything outside printable ASCII (space to `~`) is a
/// `\uXXXX` escape in lower case, characters beyond the BMP as a surrogate pair.
fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\u{u:04x}");
                }
            }
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// What `tools/jsonio.py` `dump` wrote for `edge()` (a synthetic value of our own, no game files). Regenerate it with
    /// `jsonio.dump(json.loads(<the text in edge()>))` when the layout is changed on purpose.
    const EDGE: &str = "{\n  \"small\": 1e-05,\n  \"big\": 1e+16,\n  \"floats\": [1e+22, 1.5e-07, 0.0001, 100000.0, -0.0, 0.5, 1000000000000000.0, 1.2345678901234568e+17, 5e-324, 1.7976931348623157e+308, 0.1, 1234.5678, 0.00012345, 2.5e-05, 1.5e+16, 100.0],\n  \"text\": \"\\u007f\\u00e9\\ud83d\\ude00\\u0001 \\\"q\\\" \\\\ \\n\\t/ ~\",\n  \"nested\": [\n    [1, 2],\n    [3]\n  ],\n  \"empty_list\": [],\n  \"empty_map\": {},\n  \"mixed\": [\n    1,\n    \"a\"\n  ],\n  \"flags\": [\n    true,\n    null\n  ],\n  \"huge\": 12345678901234567890,\n  \"deep\": {\n    \"pos\": [0, -1.25, 3],\n    \"list\": [\n      {\n        \"name\": \"A\",\n        \"v\": [1]\n      },\n      {}\n    ]\n  }\n}\n";

    fn edge() -> Value {
        serde_json::from_str(
            r#"{"small": 1e-5, "big": 1e16, "floats": [1e22, 1.5e-7, 0.0001, 100000.0, -0.0, 0.5, 1e15, 123456789012345680.0, 5e-324, 1.7976931348623157e308, 0.1, 1234.5678, 0.00012345, 2.5e-5, 1.5e16, 100.0],
                "text": "\u007f\u00e9\ud83d\ude00\u0001 \"q\" \\ \n\t/ ~", "nested": [[1, 2], [3]], "empty_list": [], "empty_map": {},
                "mixed": [1, "a"], "flags": [true, null], "huge": 12345678901234567890,
                "deep": {"pos": [0, -1.25, 3], "list": [{"name": "A", "v": [1]}, {}]}}"#,
        )
        .unwrap()
    }

    #[test]
    fn byte_identical_to_tools_jsonio_on_the_edge_cases() {
        assert_eq!(to_json_string(&edge()), EDGE);
    }

    #[test]
    fn a_file_in_the_layout_is_written_back_unchanged() {
        let back: Value = serde_json::from_str(EDGE).unwrap();
        assert_eq!(to_json_string(&back), EDGE);
        // key order is the document's, not sorted
        assert_eq!(to_json_string(&json!({"z": 1, "a": [1.5, 2]})), "{\n  \"z\": 1,\n  \"a\": [1.5, 2]\n}\n");
    }

    /// Floats as Python's `json.dumps` writes them (expected texts produced once with python3 for each input).
    #[test]
    fn floats_are_written_as_python_repr_does() {
        for (input, expected) in [
            ("2e-06", "2e-06"),
            ("1e-05", "1e-05"),
            ("0.0001", "0.0001"),
            ("1e16", "1e+16"),
            ("1.5e+300", "1.5e+300"),
            ("-0.0", "-0.0"),
            ("123456789012345678.0", "1.2345678901234568e+17"),
        ] {
            let v: Value = serde_json::from_str(input).unwrap();
            assert_eq!(to_json_string(&v), format!("{expected}\n"), "{input}");
            assert_eq!(to_json_string(&json!([v])), format!("[{expected}]\n"), "{input} in a list");
        }
    }

    #[test]
    fn scalars_and_empties() {
        assert_eq!(to_json_string(&json!(null)), "null\n");
        assert_eq!(to_json_string(&json!([])), "[]\n");
        assert_eq!(to_json_string(&json!("é")), "\"\\u00e9\"\n");
        assert_eq!(to_json_string(&json!(2.0)), "2.0\n");
    }
}
