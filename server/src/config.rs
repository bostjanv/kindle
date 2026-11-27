//! Lenient accessors for TOML tables (a number may be written as an int, a float or a string).

use toml::{Table, Value};

pub fn str_or(t: &Table, key: &str, default: &str) -> String {
    match t.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => default.to_string(),
    }
}

pub fn opt_f64(t: &Table, key: &str) -> Option<f64> {
    match t.get(key)? {
        Value::Integer(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub fn f64_or(t: &Table, key: &str, default: f64) -> f64 {
    opt_f64(t, key).unwrap_or(default)
}

pub fn i64_or(t: &Table, key: &str, default: i64) -> i64 {
    opt_f64(t, key).map(|f| f as i64).unwrap_or(default)
}

pub fn bool_or(t: &Table, key: &str, default: bool) -> bool {
    match t.get(key) {
        Some(Value::Boolean(b)) => *b,
        Some(Value::Integer(i)) => *i != 0,
        _ => default,
    }
}

pub fn table<'a>(t: &'a Table, key: &str) -> Option<&'a Table> {
    t.get(key).and_then(Value::as_table)
}
