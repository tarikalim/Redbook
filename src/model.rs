//! Data model shared by the store, UI, importer and MCP server.
#![allow(dead_code)] // UI wiring comes later

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub sort: f64,
    pub created: i64,
}

/// Block type, stored as its type id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum BlockType {
    Page,
    Text,
    Header,
    SubHeader,
    SubSubHeader,
    Header4,
    ToDo,
    Toggle,
    BulletedList,
    NumberedList,
    Quote,
    Callout,
    Code,
    Divider,
    Image,
    Video,
    Audio,
    File,
    Pdf,
    Embed,
    Bookmark,
    Equation,
    TableOfContents,
    ColumnList,
    Column,
    Table,
    TableRow,
    Breadcrumb,
    LinkToPage,
    CollectionView,
    CollectionViewPage,
    Unknown(String),
}

const BLOCK_TYPE_IDS: &[(BlockType, &str)] = &[
    (BlockType::Page, "page"),
    (BlockType::Text, "text"),
    (BlockType::Header, "header"),
    (BlockType::SubHeader, "sub_header"),
    (BlockType::SubSubHeader, "sub_sub_header"),
    (BlockType::Header4, "header_4"),
    (BlockType::ToDo, "to_do"),
    (BlockType::Toggle, "toggle"),
    (BlockType::BulletedList, "bulleted_list"),
    (BlockType::NumberedList, "numbered_list"),
    (BlockType::Quote, "quote"),
    (BlockType::Callout, "callout"),
    (BlockType::Code, "code"),
    (BlockType::Divider, "divider"),
    (BlockType::Image, "image"),
    (BlockType::Video, "video"),
    (BlockType::Audio, "audio"),
    (BlockType::File, "file"),
    (BlockType::Pdf, "pdf"),
    (BlockType::Embed, "embed"),
    (BlockType::Bookmark, "bookmark"),
    (BlockType::Equation, "equation"),
    (BlockType::TableOfContents, "table_of_contents"),
    (BlockType::ColumnList, "column_list"),
    (BlockType::Column, "column"),
    (BlockType::Table, "table"),
    (BlockType::TableRow, "table_row"),
    (BlockType::Breadcrumb, "breadcrumb"),
    (BlockType::LinkToPage, "link_to_page"),
    (BlockType::CollectionView, "collection_view"),
    (BlockType::CollectionViewPage, "collection_view_page"),
];

impl BlockType {
    pub fn as_str(&self) -> &str {
        if let BlockType::Unknown(s) = self {
            return s;
        }
        BLOCK_TYPE_IDS
            .iter()
            .find(|(t, _)| t == self)
            .map(|(_, s)| *s)
            .unwrap()
    }

    pub fn parse(s: &str) -> Self {
        BLOCK_TYPE_IDS
            .iter()
            .find(|(_, id)| *id == s)
            .map(|(t, _)| t.clone())
            .unwrap_or_else(|| BlockType::Unknown(s.to_string()))
    }

    /// Shows up in the sidebar tree and breadcrumbs.
    pub fn is_page_like(&self) -> bool {
        matches!(
            self,
            BlockType::Page | BlockType::CollectionView | BlockType::CollectionViewPage
        )
    }

    pub fn is_collection(&self) -> bool {
        matches!(
            self,
            BlockType::CollectionView | BlockType::CollectionViewPage
        )
    }
}

impl From<String> for BlockType {
    fn from(s: String) -> Self {
        BlockType::parse(&s)
    }
}

impl From<BlockType> for String {
    fn from(t: BlockType) -> Self {
        t.as_str().to_string()
    }
}

/// Inline formatting; serialized as `["b"]`, `["a", url]`, ...
#[derive(Clone, Debug, PartialEq)]
pub enum Annotation {
    Bold,
    Italic,
    Strike,
    Underline,
    Code,
    Link(String),
    Color(String),
    /// Inline LaTeX; segment text is "⁍".
    Equation(String),
    /// Page mention; segment text is "‣".
    PageMention(String),
    /// Date mention `{type, start_date, ...}`; segment text is "‣".
    Date(Value),
    Other(Vec<Value>),
}

impl Annotation {
    fn to_json(&self) -> Value {
        match self {
            Annotation::Bold => json!(["b"]),
            Annotation::Italic => json!(["i"]),
            Annotation::Strike => json!(["s"]),
            Annotation::Underline => json!(["_"]),
            Annotation::Code => json!(["c"]),
            Annotation::Link(u) => json!(["a", u]),
            Annotation::Color(c) => json!(["h", c]),
            Annotation::Equation(e) => json!(["e", e]),
            Annotation::PageMention(p) => json!(["p", p]),
            Annotation::Date(d) => json!(["d", d]),
            Annotation::Other(v) => Value::Array(v.clone()),
        }
    }

    fn from_json(v: &Value) -> Option<Self> {
        let arr = v.as_array()?;
        let arg = || {
            arr.get(1)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        Some(match arr.first()?.as_str()? {
            "b" => Annotation::Bold,
            "i" => Annotation::Italic,
            "s" => Annotation::Strike,
            "_" => Annotation::Underline,
            "c" => Annotation::Code,
            "a" => Annotation::Link(arg()),
            "h" => Annotation::Color(arg()),
            "e" => Annotation::Equation(arg()),
            "p" => Annotation::PageMention(arg()),
            "d" => Annotation::Date(arr.get(1).cloned().unwrap_or(Value::Null)),
            _ => Annotation::Other(arr.clone()),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Segment {
    pub text: String,
    pub annotations: Vec<Annotation>,
}

impl Segment {
    pub fn has(&self, a: &Annotation) -> bool {
        self.annotations.contains(a)
    }

    pub fn link(&self) -> Option<&str> {
        self.annotations.iter().find_map(|a| match a {
            Annotation::Link(u) => Some(u.as_str()),
            _ => None,
        })
    }
}

/// Rich text as an array of segments: `[["plain"],["bold",[["b"]]]]`.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(from = "Value", into = "Value")]
pub struct RichText(pub Vec<Segment>);

impl RichText {
    pub fn from_plain(s: &str) -> Self {
        if s.is_empty() {
            return RichText::default();
        }
        RichText(vec![Segment {
            text: s.to_string(),
            annotations: vec![],
        }])
    }

    pub fn plain_text(&self) -> String {
        self.0.iter().map(|s| s.text.as_str()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|s| s.text.is_empty())
    }

    /// Merges adjacent segments with identical annotations and drops empty ones.
    pub fn normalized(mut self) -> Self {
        let mut out: Vec<Segment> = Vec::with_capacity(self.0.len());
        for seg in self.0.drain(..).filter(|s| !s.text.is_empty()) {
            match out.last_mut() {
                Some(last) if last.annotations == seg.annotations => last.text.push_str(&seg.text),
                _ => out.push(seg),
            }
        }
        RichText(out)
    }

    pub fn to_json_string(&self) -> String {
        Value::from(self.clone()).to_string()
    }

    pub fn from_json_str(s: &str) -> Self {
        serde_json::from_str::<Value>(s)
            .map(RichText::from)
            .unwrap_or_default()
    }
}

impl From<Value> for RichText {
    fn from(v: Value) -> Self {
        let segs = v.as_array().map(|a| a.as_slice()).unwrap_or_default();
        RichText(
            segs.iter()
                .filter_map(|seg| {
                    let seg = seg.as_array()?;
                    let text = seg.first()?.as_str()?.to_string();
                    let annotations = seg
                        .get(1)
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(Annotation::from_json).collect())
                        .unwrap_or_default();
                    Some(Segment { text, annotations })
                })
                .collect(),
        )
    }
}

impl From<RichText> for Value {
    fn from(rt: RichText) -> Self {
        Value::Array(
            rt.0.iter()
                .map(|s| {
                    if s.annotations.is_empty() {
                        json!([s.text])
                    } else {
                        let anns: Vec<Value> =
                            s.annotations.iter().map(Annotation::to_json).collect();
                        json!([s.text, anns])
                    }
                })
                .collect(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub id: String,
    pub workspace_id: String,
    pub parent_id: Option<String>,
    #[serde(rename = "type")]
    pub kind: BlockType,
    pub sort: f64,
    pub title: RichText,
    /// checked, language, icon, cover, color, source; db rows keep values in `properties`.
    pub props: Value,
    pub created: i64,
    pub edited: i64,
    pub trashed: Option<i64>,
    pub favorite: bool,
}

impl Block {
    pub fn icon(&self) -> Option<&str> {
        self.props.get("icon").and_then(Value::as_str)
    }

    pub fn checked(&self) -> bool {
        self.props
            .get("checked")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub fn str_prop(&self, key: &str) -> Option<&str> {
        self.props.get(key).and_then(Value::as_str)
    }

    /// Cells of a `table_row` (`props.cells`, rich text each).
    pub fn cells(&self) -> Vec<RichText> {
        self.props
            .get("cells")
            .and_then(Value::as_array)
            .map(|a| a.iter().cloned().map(RichText::from).collect())
            .unwrap_or_default()
    }

    /// Value of a database property for this row (title and timestamps included).
    pub fn row_value(&self, prop_id: &str, prop: &Property) -> Value {
        match prop.kind {
            PropType::Title => Value::String(self.title.plain_text()),
            PropType::CreatedTime => json!(self.created),
            PropType::LastEditedTime => json!(self.edited),
            _ => self
                .props
                .get("properties")
                .and_then(|p| p.get(prop_id))
                .cloned()
                .unwrap_or(Value::Null),
        }
    }
}

/// Database cell access shared by `Block` and the compact `DbRow`.
pub trait RowValue {
    fn row_value(&self, prop_id: &str, prop: &Property) -> Value;
}

impl RowValue for Block {
    fn row_value(&self, prop_id: &str, prop: &Property) -> Value {
        Block::row_value(self, prop_id, prop)
    }
}

impl<T: RowValue> RowValue for std::rc::Rc<T> {
    fn row_value(&self, prop_id: &str, prop: &Property) -> Value {
        (**self).row_value(prop_id, prop)
    }
}

/// Database row as views hold it: cells stay raw JSON (a parsed `Value` is ~10x larger).
#[derive(Clone, Debug, PartialEq)]
pub struct DbRow {
    pub id: String,
    pub title: RichText,
    pub created: i64,
    pub edited: i64,
    /// `props.properties` as JSON text.
    pub cells: Box<str>,
}

impl DbRow {
    pub fn row_value(&self, prop_id: &str, prop: &Property) -> Value {
        match prop.kind {
            PropType::Title => Value::String(self.title.plain_text()),
            PropType::CreatedTime => json!(self.created),
            PropType::LastEditedTime => json!(self.edited),
            _ => cell(&self.cells, prop_id),
        }
    }
}

impl RowValue for DbRow {
    fn row_value(&self, prop_id: &str, prop: &Property) -> Value {
        DbRow::row_value(self, prop_id, prop)
    }
}

/// Parses only `key`'s value out of a JSON object; Null when absent or malformed.
fn cell(json: &str, key: &str) -> Value {
    use serde::de::{DeserializeSeed, Deserializer, IgnoredAny, MapAccess, Visitor};
    use std::fmt;

    struct Pick<'k>(&'k str);
    struct IsKey<'k>(&'k str);

    impl<'de> DeserializeSeed<'de> for IsKey<'_> {
        type Value = bool;
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<bool, D::Error> {
            d.deserialize_str(self)
        }
    }
    impl<'de> Visitor<'de> for IsKey<'_> {
        type Value = bool;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a key")
        }
        fn visit_str<E>(self, v: &str) -> Result<bool, E> {
            Ok(v == self.0)
        }
    }
    impl<'de> Visitor<'de> for Pick<'_> {
        type Value = Value;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an object")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Value, A::Error> {
            let mut out = Value::Null;
            while let Some(hit) = m.next_key_seed(IsKey(self.0))? {
                if hit {
                    out = m.next_value()?;
                } else {
                    m.next_value::<IgnoredAny>()?;
                }
            }
            Ok(out)
        }
    }
    let mut de = serde_json::Deserializer::from_str(json);
    de.deserialize_map(Pick(key)).unwrap_or(Value::Null)
}

/// Sidebar / breadcrumb entry.
#[derive(Clone, Debug, PartialEq)]
pub struct PageNode {
    pub id: String,
    /// Nearest page-like ancestor.
    pub parent_id: Option<String>,
    pub kind: BlockType,
    pub title: String,
    pub icon: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub page_id: String,
    pub title: String,
    pub snippet: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropType {
    Title,
    Text,
    Number,
    Select,
    MultiSelect,
    Status,
    Date,
    Checkbox,
    Url,
    Email,
    PhoneNumber,
    CreatedTime,
    LastEditedTime,
    Relation,
    Formula,
    Rollup,
    File,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectOption {
    pub id: String,
    pub value: String,
    #[serde(default)]
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Property {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: PropType,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SelectOption>,
    /// Type-specific config (number format, relation target, formula).
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub config: Value,
}

impl Property {
    pub fn new(name: &str, kind: PropType) -> Self {
        Property {
            name: name.to_string(),
            kind,
            options: vec![],
            config: Value::Null,
        }
    }

    pub fn option_index(&self, value: &str) -> Option<usize> {
        self.options.iter().position(|o| o.value == value)
    }
}

/// Collection schema: prop id -> property. The title prop id is `"title"`.
pub type Schema = BTreeMap<String, Property>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewType {
    Table,
    Board,
    List,
    Gallery,
    Calendar,
    Timeline,
    #[serde(other)]
    Unknown,
}

impl ViewType {
    pub fn as_str(self) -> &'static str {
        match self {
            ViewType::Table => "table",
            ViewType::Board => "board",
            ViewType::List => "list",
            ViewType::Gallery => "gallery",
            ViewType::Calendar => "calendar",
            ViewType::Timeline => "timeline",
            ViewType::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Self {
        serde_json::from_value(Value::String(s.to_string())).unwrap_or(ViewType::Unknown)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Is,
    IsNot,
    Contains,
    DoesNotContain,
    StartsWith,
    EndsWith,
    IsEmpty,
    IsNotEmpty,
    GreaterThan,
    LessThan,
    GreaterThanOrEqualTo,
    LessThanOrEqualTo,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub property: String,
    pub op: FilterOp,
    #[serde(default)]
    pub value: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOperator {
    #[default]
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Ascending,
    Descending,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sort {
    pub property: String,
    pub direction: Direction,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Query {
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub operator: FilterOperator,
    #[serde(default)]
    pub sorts: Vec<Sort>,
    #[serde(default)]
    pub group_by: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub id: String,
    pub collection_id: String,
    #[serde(rename = "type")]
    pub kind: ViewType,
    pub name: String,
    pub sort: f64,
    pub query: Query,
    /// UI layout: visible properties, column widths, card size.
    pub format: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_picks_one_key() {
        let j = r#"{"a":"x","b":{"start":"2026-10-11"},"c!":[1,2],"d":null}"#;
        assert_eq!(cell(j, "a"), json!("x"));
        assert_eq!(cell(j, "b"), json!({"start": "2026-10-11"}));
        assert_eq!(cell(j, "c!"), json!([1, 2]));
        assert_eq!(cell(j, "zz"), Value::Null);
        assert_eq!(cell("not json", "a"), Value::Null);
        assert_eq!(cell("", "a"), Value::Null);
    }
}
