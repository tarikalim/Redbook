//! Pure database helpers: value display, number formats, footer calculations, filter operators.
use chrono::{Local, NaiveDate, TimeZone as _};
use serde_json::{Value, json};

use crate::model::*;
use crate::store::value_text;

pub const NUMBER_FORMATS: &[(&str, &str)] = &[
    ("number", "Number"),
    ("number_with_commas", "Number with commas"),
    ("percent", "Percent"),
    ("dollar", "US dollar"),
    ("euro", "Euro"),
    ("pound", "Pound"),
    ("yen", "Yen"),
    ("lira", "Lira"),
];

pub const PROP_TYPES: &[(PropType, &str)] = &[
    (PropType::Text, "Text"),
    (PropType::Number, "Number"),
    (PropType::Select, "Select"),
    (PropType::MultiSelect, "Multi-select"),
    (PropType::Status, "Status"),
    (PropType::Date, "Date"),
    (PropType::Checkbox, "Checkbox"),
    (PropType::Url, "URL"),
    (PropType::Email, "Email"),
    (PropType::PhoneNumber, "Phone"),
    (PropType::CreatedTime, "Created time"),
    (PropType::LastEditedTime, "Last edited time"),
];

pub fn type_label(kind: PropType) -> &'static str {
    match kind {
        PropType::Title => "Title",
        k => PROP_TYPES
            .iter()
            .find(|(t, _)| *t == k)
            .map_or("Unsupported", |(_, l)| *l),
    }
}

pub fn is_options(kind: PropType) -> bool {
    matches!(
        kind,
        PropType::Select | PropType::MultiSelect | PropType::Status
    )
}

pub fn is_timestamp(kind: PropType) -> bool {
    matches!(kind, PropType::CreatedTime | PropType::LastEditedTime)
}

/// Option colors, in menu order.
pub const COLORS: &[&str] = &[
    "default", "gray", "brown", "orange", "yellow", "green", "blue", "purple", "pink", "red",
];

pub fn number_text(n: f64, format: &str) -> String {
    let plain = |n: f64| {
        let s = format!("{:.2}", n);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    let commas = |n: f64| {
        let s = plain(n.abs());
        let (int, frac) = s
            .split_once('.')
            .map_or((s.as_str(), None), |(a, b)| (a, Some(b)));
        let mut out = String::new();
        for (i, c) in int.chars().enumerate() {
            if i > 0 && (int.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(c);
        }
        if let Some(f) = frac {
            out = format!("{out}.{f}");
        }
        if n < 0. { format!("-{out}") } else { out }
    };
    match format {
        "number_with_commas" => commas(n),
        "percent" => format!("{}%", plain(n * 100.)),
        "dollar" => format!("${}", commas(n)),
        "euro" => format!("€{}", commas(n)),
        "pound" => format!("£{}", commas(n)),
        "yen" => format!("¥{}", commas(n)),
        "lira" => format!("₺{}", commas(n)),
        _ => plain(n),
    }
}

pub fn number_format(p: &Property) -> &str {
    p.config
        .get("number_format")
        .and_then(Value::as_str)
        .unwrap_or("number")
}

pub fn ms_text(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.format("%b %-d, %Y %H:%M").to_string())
        .unwrap_or_default()
}

/// Date value start as a NaiveDate (`{"start":"YYYY-MM-DD"}` or a plain string).
pub fn date_start(v: &Value) -> Option<NaiveDate> {
    let s = v.get("start").unwrap_or(v).as_str()?;
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

pub fn date_value(d: NaiveDate) -> Value {
    json!({ "start": d.format("%Y-%m-%d").to_string() })
}

/// Local midnight of `d` in ms, for filtering timestamp properties.
pub fn day_ms(d: NaiveDate) -> i64 {
    Local
        .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap_or_default())
        .earliest()
        .map_or(0, |t| t.timestamp_millis())
}

/// Cell text for non-chip properties.
pub fn display_text(p: &Property, v: &Value) -> String {
    match p.kind {
        PropType::Number => v
            .as_f64()
            .map_or_else(|| value_text(v), |n| number_text(n, number_format(p))),
        PropType::Date => match (date_start(v), v.get("end").and_then(date_start)) {
            (Some(s), Some(e)) => {
                format!("{} → {}", s.format("%b %-d, %Y"), e.format("%b %-d, %Y"))
            }
            (Some(s), None) => s.format("%b %-d, %Y").to_string(),
            _ => value_text(v),
        },
        PropType::CreatedTime | PropType::LastEditedTime => {
            v.as_i64().map(ms_text).unwrap_or_default()
        }
        PropType::Checkbox => String::new(),
        _ => value_text(v),
    }
}

/// Text typed into a number cell; empty or invalid clears it.
pub fn parse_number(s: &str) -> Value {
    let s: String = s
        .chars()
        .filter(|c| !matches!(c, ',' | '$' | '€' | '£' | '¥' | '₺' | '%' | ' '))
        .collect();
    match s.parse::<f64>() {
        Ok(n) if n.fract() == 0. && n.abs() < 9e15 => json!(n as i64),
        Ok(n) => json!(n),
        Err(_) => Value::Null,
    }
}

/// Option values of a select-like cell.
pub fn option_values(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        Value::String(s) if !s.is_empty() => vec![s.clone()],
        _ => vec![],
    }
}

pub fn is_empty_value(v: &Value) -> bool {
    match v.get("start").unwrap_or(v) {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Bool(b) => !b,
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Calc {
    Count,
    CountValues,
    Unique,
    Empty,
    NotEmpty,
    PercentEmpty,
    PercentNotEmpty,
    Checked,
    Unchecked,
    PercentChecked,
    Sum,
    Average,
    Median,
    Min,
    Max,
    Range,
}

const CALCS: &[(Calc, &str, &str)] = &[
    (Calc::Count, "count", "Count all"),
    (Calc::CountValues, "count_values", "Count values"),
    (Calc::Unique, "unique", "Count unique"),
    (Calc::Empty, "empty", "Count empty"),
    (Calc::NotEmpty, "not_empty", "Count not empty"),
    (Calc::PercentEmpty, "percent_empty", "Percent empty"),
    (
        Calc::PercentNotEmpty,
        "percent_not_empty",
        "Percent not empty",
    ),
    (Calc::Checked, "checked", "Checked"),
    (Calc::Unchecked, "unchecked", "Unchecked"),
    (Calc::PercentChecked, "percent_checked", "Percent checked"),
    (Calc::Sum, "sum", "Sum"),
    (Calc::Average, "average", "Average"),
    (Calc::Median, "median", "Median"),
    (Calc::Min, "min", "Min"),
    (Calc::Max, "max", "Max"),
    (Calc::Range, "range", "Range"),
];

impl Calc {
    pub fn id(self) -> &'static str {
        CALCS.iter().find(|c| c.0 == self).map_or("", |c| c.1)
    }

    pub fn label(self) -> &'static str {
        CALCS.iter().find(|c| c.0 == self).map_or("", |c| c.2)
    }

    pub fn parse(s: &str) -> Option<Calc> {
        CALCS.iter().find(|c| c.1 == s).map(|c| c.0)
    }

    /// Calculations offered for a property type.
    pub fn for_kind(kind: PropType) -> Vec<Calc> {
        use Calc::*;
        let mut v = vec![Count];
        match kind {
            PropType::Checkbox => v.extend([Checked, Unchecked, PercentChecked]),
            PropType::Number => v.extend([
                CountValues,
                Unique,
                Empty,
                NotEmpty,
                PercentEmpty,
                PercentNotEmpty,
                Sum,
                Average,
                Median,
                Min,
                Max,
                Range,
            ]),
            _ => v.extend([
                CountValues,
                Unique,
                Empty,
                NotEmpty,
                PercentEmpty,
                PercentNotEmpty,
            ]),
        }
        v
    }
}

/// Footer result over a column's values (one per shown row).
pub fn calculate(calc: Calc, p: &Property, values: &[Value]) -> String {
    let total = values.len();
    let filled = values.iter().filter(|v| !is_empty_value(v)).count();
    let pct = |n: usize| {
        if total == 0 {
            "0%".to_string()
        } else {
            number_text(n as f64 / total as f64, "percent")
        }
    };
    let mut nums: Vec<f64> = values.iter().filter_map(Value::as_f64).collect();
    nums.sort_by(|a, b| a.total_cmp(b));
    let fmt = |n: f64| number_text(n, number_format(p));
    let checked = values.iter().filter(|v| v.as_bool() == Some(true)).count();
    match calc {
        Calc::Count => total.to_string(),
        Calc::CountValues => values
            .iter()
            .map(|v| match v {
                Value::Array(a) => a.len(),
                v if is_empty_value(v) => 0,
                _ => 1,
            })
            .sum::<usize>()
            .to_string(),
        Calc::Unique => {
            let mut seen: Vec<String> = values
                .iter()
                .filter(|v| !is_empty_value(v))
                .flat_map(|v| match v {
                    Value::Array(a) => a.iter().map(value_text).collect(),
                    v => vec![value_text(v.get("start").unwrap_or(v))],
                })
                .collect();
            seen.sort();
            seen.dedup();
            seen.len().to_string()
        }
        Calc::Empty => (total - filled).to_string(),
        Calc::NotEmpty => filled.to_string(),
        Calc::PercentEmpty => pct(total - filled),
        Calc::PercentNotEmpty => pct(filled),
        Calc::Checked => checked.to_string(),
        Calc::Unchecked => (total - checked).to_string(),
        Calc::PercentChecked => pct(checked),
        _ if nums.is_empty() => String::new(),
        Calc::Sum => fmt(nums.iter().sum()),
        Calc::Average => fmt(nums.iter().sum::<f64>() / nums.len() as f64),
        Calc::Median => {
            let m = nums.len() / 2;
            fmt(if nums.len().is_multiple_of(2) {
                (nums[m - 1] + nums[m]) / 2.
            } else {
                nums[m]
            })
        }
        Calc::Min => fmt(nums[0]),
        Calc::Max => fmt(nums[nums.len() - 1]),
        Calc::Range => fmt(nums[nums.len() - 1] - nums[0]),
    }
}

/// Filter operators offered per property type, with their menu labels.
pub fn filter_ops(kind: PropType) -> &'static [(FilterOp, &'static str)] {
    use FilterOp::*;
    match kind {
        PropType::Number => &[
            (Is, "="),
            (IsNot, "≠"),
            (GreaterThan, ">"),
            (LessThan, "<"),
            (GreaterThanOrEqualTo, "≥"),
            (LessThanOrEqualTo, "≤"),
            (IsEmpty, "Is empty"),
            (IsNotEmpty, "Is not empty"),
        ],
        PropType::Select | PropType::Status => &[
            (Is, "Is"),
            (IsNot, "Is not"),
            (IsEmpty, "Is empty"),
            (IsNotEmpty, "Is not empty"),
        ],
        PropType::MultiSelect => &[
            (Contains, "Contains"),
            (DoesNotContain, "Does not contain"),
            (IsEmpty, "Is empty"),
            (IsNotEmpty, "Is not empty"),
        ],
        PropType::Checkbox => &[(Is, "Is")],
        PropType::Date => &[
            (Is, "Is"),
            (LessThan, "Is before"),
            (GreaterThan, "Is after"),
            (LessThanOrEqualTo, "Is on or before"),
            (GreaterThanOrEqualTo, "Is on or after"),
            (IsEmpty, "Is empty"),
            (IsNotEmpty, "Is not empty"),
        ],
        PropType::CreatedTime | PropType::LastEditedTime => &[
            (LessThan, "Is before"),
            (GreaterThanOrEqualTo, "Is on or after"),
        ],
        _ => &[
            (Is, "Is"),
            (IsNot, "Is not"),
            (Contains, "Contains"),
            (DoesNotContain, "Does not contain"),
            (StartsWith, "Starts with"),
            (EndsWith, "Ends with"),
            (IsEmpty, "Is empty"),
            (IsNotEmpty, "Is not empty"),
        ],
    }
}

pub fn op_label(kind: PropType, op: FilterOp) -> &'static str {
    filter_ops(kind)
        .iter()
        .find(|(o, _)| *o == op)
        .map_or("?", |(_, l)| *l)
}

/// Default value for a fresh filter of this kind.
pub fn default_filter(prop_id: &str, kind: PropType) -> Filter {
    let (op, value) = match kind {
        PropType::Checkbox => (FilterOp::Is, json!(true)),
        _ => (filter_ops(kind)[0].0, Value::Null),
    };
    Filter {
        property: prop_id.to_string(),
        op,
        value,
    }
}

/// Filter value from the text typed in the filter box.
pub fn filter_value(kind: PropType, s: &str) -> Value {
    let s = s.trim();
    if s.is_empty() {
        return Value::Null;
    }
    match kind {
        PropType::Number => match parse_number(s) {
            Value::Null => json!(s),
            n => n,
        },
        PropType::CreatedTime | PropType::LastEditedTime => {
            NaiveDate::parse_from_str(s, "%Y-%m-%d").map_or(Value::Null, |d| json!(day_ms(d)))
        }
        _ => json!(s),
    }
}

/// Filters whose value is still unset don't narrow the rows.
pub fn effective_query(q: &Query) -> Query {
    let mut q = q.clone();
    q.filters
        .retain(|f| matches!(f.op, FilterOp::IsEmpty | FilterOp::IsNotEmpty) || !f.value.is_null());
    q
}

/// Case-insensitive search over title and text-like values.
pub fn search_match(row: &impl RowValue, schema: &Schema, needle: &str) -> bool {
    needle.is_empty()
        || schema.iter().any(|(id, p)| {
            !is_timestamp(p.kind)
                && value_text(&row.row_value(id, p))
                    .to_lowercase()
                    .contains(needle)
        })
}

/// Property ids in view order: title first, then `format.order`, then the rest.
pub fn ordered_props(schema: &Schema, format: &Value) -> Vec<String> {
    let mut out = vec!["title".to_string()];
    let order = format.get("order").and_then(Value::as_array);
    for id in order.into_iter().flatten().filter_map(Value::as_str) {
        if id != "title" && schema.contains_key(id) && !out.iter().any(|o| o == id) {
            out.push(id.to_string());
        }
    }
    for id in schema.keys() {
        if !out.contains(id) {
            out.push(id.clone());
        }
    }
    out.retain(|id| schema.contains_key(id));
    out
}

pub fn is_hidden(format: &Value, prop: &str) -> bool {
    format
        .get("hidden")
        .and_then(Value::as_array)
        .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(prop)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::apply_query;

    fn prop(kind: PropType) -> Property {
        Property::new("p", kind)
    }

    fn row(title: &str, values: Value) -> Block {
        Block {
            id: title.into(),
            workspace_id: "w".into(),
            parent_id: Some("db".into()),
            kind: BlockType::Page,
            sort: 0.,
            title: RichText::from_plain(title),
            props: json!({ "properties": values }),
            created: 0,
            edited: 0,
            trashed: None,
            favorite: false,
        }
    }

    fn schema() -> Schema {
        let mut s = Schema::new();
        s.insert("title".into(), Property::new("Name", PropType::Title));
        s.insert("n".into(), Property::new("Points", PropType::Number));
        s.insert("c".into(), Property::new("Done", PropType::Checkbox));
        s.insert("d".into(), Property::new("Due", PropType::Date));
        s.insert("t".into(), Property::new("Notes", PropType::Text));
        s
    }

    fn rows() -> Vec<Block> {
        vec![
            row(
                "a",
                json!({"n": 5, "c": true, "d": {"start": "2026-01-10"}, "t": "alpha"}),
            ),
            row("b", json!({"n": 1, "d": {"start": "2026-03-01"}})),
            row("c", json!({"n": 3, "c": false, "t": "Beta gamma"})),
            row("d", json!({"c": true, "d": {"start": "2025-12-31"}})),
        ]
    }

    fn titles(q: &Query) -> Vec<String> {
        apply_query(rows(), &effective_query(q), &schema())
            .iter()
            .map(|b| b.title.plain_text())
            .collect()
    }

    fn f(property: &str, op: FilterOp, value: Value) -> Filter {
        Filter {
            property: property.into(),
            op,
            value,
        }
    }

    #[test]
    fn filters_combine_with_and() {
        let mut q = Query {
            filters: vec![f("c", FilterOp::Is, json!(true))],
            ..Default::default()
        };
        assert_eq!(titles(&q), ["a", "d"]);
        q.filters = vec![f("c", FilterOp::Is, json!(false))];
        assert_eq!(titles(&q), ["b", "c"]);
        q.filters = vec![
            f("d", FilterOp::GreaterThan, json!("2026-01-01")),
            f("n", FilterOp::GreaterThanOrEqualTo, json!(2)),
        ];
        assert_eq!(titles(&q), ["a"]);
        q.filters = vec![f("d", FilterOp::Is, json!("2025-12-31"))];
        assert_eq!(titles(&q), ["d"]);
        q.filters = vec![f("d", FilterOp::IsEmpty, Value::Null)];
        assert_eq!(titles(&q), ["c"]);
        q.filters = vec![f("t", FilterOp::Contains, json!("GAM"))];
        assert_eq!(titles(&q), ["c"]);
        q.filters = vec![f("t", FilterOp::StartsWith, json!("al"))];
        assert_eq!(titles(&q), ["a"]);
        // Unset values are ignored instead of hiding every row.
        q.filters = vec![f("t", FilterOp::Is, Value::Null)];
        assert_eq!(titles(&q).len(), 4);
    }

    #[test]
    fn multi_level_sort_puts_empty_last() {
        let mut q = Query {
            sorts: vec![
                Sort {
                    property: "c".into(),
                    direction: Direction::Descending,
                },
                Sort {
                    property: "n".into(),
                    direction: Direction::Ascending,
                },
            ],
            ..Default::default()
        };
        // Checked first (a, d), then unchecked (c false, b missing); n ascending, empty last.
        assert_eq!(titles(&q), ["a", "d", "c", "b"]);
        q.sorts = vec![Sort {
            property: "d".into(),
            direction: Direction::Ascending,
        }];
        assert_eq!(titles(&q), ["d", "a", "b", "c"]);
    }

    #[test]
    fn calculations() {
        let n = prop(PropType::Number);
        let vals = [json!(5), json!(1), Value::Null, json!(3)];
        assert_eq!(calculate(Calc::Count, &n, &vals), "4");
        assert_eq!(calculate(Calc::CountValues, &n, &vals), "3");
        assert_eq!(calculate(Calc::Empty, &n, &vals), "1");
        assert_eq!(calculate(Calc::PercentNotEmpty, &n, &vals), "75%");
        assert_eq!(calculate(Calc::Sum, &n, &vals), "9");
        assert_eq!(calculate(Calc::Average, &n, &vals), "3");
        assert_eq!(calculate(Calc::Median, &n, &vals), "3");
        assert_eq!(calculate(Calc::Min, &n, &vals), "1");
        assert_eq!(calculate(Calc::Max, &n, &vals), "5");
        assert_eq!(calculate(Calc::Range, &n, &vals), "4");
        assert_eq!(calculate(Calc::Sum, &n, &[]), "");

        let c = prop(PropType::Checkbox);
        let vals = [json!(true), json!(false), Value::Null, json!(true)];
        assert_eq!(calculate(Calc::Checked, &c, &vals), "2");
        assert_eq!(calculate(Calc::Unchecked, &c, &vals), "2");
        assert_eq!(calculate(Calc::PercentChecked, &c, &vals), "50%");

        let m = prop(PropType::MultiSelect);
        let vals = [json!(["x", "y"]), json!(["x"]), json!([])];
        assert_eq!(calculate(Calc::CountValues, &m, &vals), "3");
        assert_eq!(calculate(Calc::Unique, &m, &vals), "2");
        assert_eq!(calculate(Calc::PercentEmpty, &m, &vals), "33.33%");
        assert!(
            Calc::for_kind(PropType::Text)
                .iter()
                .all(|c| *c != Calc::Sum)
        );
        assert_eq!(Calc::parse(Calc::Median.id()), Some(Calc::Median));
    }

    #[test]
    fn number_formats() {
        assert_eq!(number_text(1234567.5, "number_with_commas"), "1,234,567.5");
        assert_eq!(number_text(-1234.0, "dollar"), "$-1,234");
        assert_eq!(number_text(0.256, "percent"), "25.6%");
        assert_eq!(number_text(12.0, "number"), "12");
        assert_eq!(number_text(999.0, "euro"), "€999");
        assert_eq!(parse_number("$1,200"), json!(1200));
        assert_eq!(parse_number("2.5"), json!(2.5));
        assert_eq!(parse_number("abc"), Value::Null);
        let mut p = prop(PropType::Number);
        p.config = json!({"number_format": "pound"});
        assert_eq!(display_text(&p, &json!(1500)), "£1,500");
    }

    #[test]
    fn display_search_and_order() {
        let d = prop(PropType::Date);
        assert_eq!(
            display_text(&d, &json!({"start": "2026-10-11"})),
            "Oct 11, 2026"
        );
        assert_eq!(
            date_start(&date_value(NaiveDate::from_ymd_opt(2026, 2, 3).unwrap()))
                .unwrap()
                .to_string(),
            "2026-02-03"
        );
        let s = schema();
        let r = &rows()[2];
        assert!(search_match(r, &s, "gamma"));
        assert!(search_match(r, &s, "c"));
        assert!(!search_match(r, &s, "zeta"));
        let order = ordered_props(&s, &json!({"order": ["t", "gone", "n"]}));
        assert_eq!(order, ["title", "t", "n", "c", "d"]);
        assert!(is_hidden(&json!({"hidden": ["n"]}), "n"));
        assert_eq!(filter_value(PropType::Number, "4"), json!(4));
        assert_eq!(filter_value(PropType::Text, "  "), Value::Null);
    }
}
