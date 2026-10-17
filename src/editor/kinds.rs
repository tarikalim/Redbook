//! Block-type rules: markdown shortcuts, slash items, list numbering.
use crate::model::{Annotation, BlockType};

/// Blocks whose title is edited as text.
pub fn editable(kind: &BlockType) -> bool {
    use BlockType::*;
    matches!(
        kind,
        Text | Header
            | SubHeader
            | SubSubHeader
            | Header4
            | ToDo
            | BulletedList
            | NumberedList
            | Toggle
            | Quote
            | Callout
            | Code
            | Equation
    )
}

/// Enter on these continues the list with the same type.
pub fn is_list(kind: &BlockType) -> bool {
    use BlockType::*;
    matches!(kind, ToDo | BulletedList | NumberedList | Toggle)
}

pub fn can_have_children(kind: &BlockType) -> bool {
    use BlockType::*;
    matches!(
        kind,
        Text | ToDo | BulletedList | NumberedList | Toggle | Callout | Quote
    )
}

pub fn is_monospace(kind: &BlockType) -> bool {
    matches!(kind, BlockType::Code | BlockType::Equation)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shortcut {
    pub kind: BlockType,
    pub checked: bool,
    pub start: Option<u64>,
}

/// Matches the text typed before the caret at the start of a block ("# ", "1. ", "```", ...).
pub fn detect_shortcut(prefix: &str) -> Option<Shortcut> {
    use BlockType::*;
    let plain = |kind| {
        Some(Shortcut {
            kind,
            checked: false,
            start: None,
        })
    };
    match prefix {
        "# " => plain(Header),
        "## " => plain(SubHeader),
        "### " => plain(SubSubHeader),
        "#### " => plain(Header4),
        "[] " | "[ ] " => plain(ToDo),
        "[x] " | "[X] " => Some(Shortcut {
            kind: ToDo,
            checked: true,
            start: None,
        }),
        "- " | "* " | "+ " | "• " => plain(BulletedList),
        "> " => plain(Toggle),
        "\" " | "“ " | "| " => plain(Quote),
        "```" => plain(Code),
        "---" | "—-" => plain(Divider),
        _ => {
            let num = prefix
                .strip_suffix(". ")
                .or_else(|| prefix.strip_suffix(") "))?;
            let start = num.parse::<u64>().ok()?;
            Some(Shortcut {
                kind: NumberedList,
                checked: false,
                start: (start != 1).then_some(start),
            })
        }
    }
}

/// Inline markdown closed at the caret (`**b**`, `*i*`, `` `c` ``, `~s~`): (opening offset, delimiter length, annotation).
pub fn inline_markdown(before: &str) -> Option<(usize, usize, Annotation)> {
    let rules = [
        ("**", Annotation::Bold),
        ("__", Annotation::Bold),
        ("*", Annotation::Italic),
        ("_", Annotation::Italic),
        ("~", Annotation::Strike),
        ("`", Annotation::Code),
    ];
    for (delim, ann) in rules {
        let Some(body) = before.strip_suffix(delim) else {
            continue;
        };
        let Some(open) = body.rfind(delim) else {
            continue;
        };
        let inner = &body[open + delim.len()..];
        let boundary = body[..open].chars().last().is_none_or(char::is_whitespace);
        if inner.is_empty() || inner.starts_with(' ') || inner.ends_with(' ') || !boundary {
            continue;
        }
        if delim.len() == 1 && inner.starts_with(delim) {
            continue;
        }
        return Some((open, delim.len(), ann));
    }
    None
}

/// Number of each sibling: runs of numbered items count up from their first item's start.
pub fn list_numbers(items: &[(bool, Option<u64>)]) -> Vec<u64> {
    let mut out = Vec::with_capacity(items.len());
    let mut n = 0;
    let mut prev = false;
    for &(numbered, start) in items {
        n = match (numbered, prev) {
            (false, _) => 0,
            (true, false) => start.unwrap_or(1),
            (true, true) => n + 1,
        };
        prev = numbered;
        out.push(n);
    }
    out
}

/// "1." / "a." / "i." by nesting depth.
pub fn list_marker(n: u64, depth: usize) -> String {
    match depth % 3 {
        0 => format!("{n}."),
        1 => format!("{}.", letters(n)),
        _ => format!("{}.", roman(n)),
    }
}

fn letters(mut n: u64) -> String {
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push(b'a' + (n % 26) as u8);
        n /= 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

fn roman(mut n: u64) -> String {
    const R: [(u64, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut s = String::new();
    for (v, r) in R {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

pub fn bullet(depth: usize) -> &'static str {
    ["•", "◦", "▪"][depth % 3]
}

pub fn placeholder(kind: &BlockType) -> &'static str {
    use BlockType::*;
    match kind {
        Header => "Heading 1",
        SubHeader => "Heading 2",
        SubSubHeader => "Heading 3",
        Header4 => "Heading 4",
        ToDo => "To-do",
        BulletedList | NumberedList => "List",
        Toggle => "Toggle",
        Quote => "Empty quote",
        Code | Equation => "",
        _ => "Type '/' for commands",
    }
}

/// Slash menu entry: (title, type id, keywords).
pub const SLASH_ITEMS: &[(&str, &str, &str)] = &[
    ("Text", "text", "text plain"),
    ("Page", "page", "page"),
    ("To-do list", "to_do", "todo checkbox"),
    ("Heading 1", "header", "h1 heading"),
    ("Heading 2", "sub_header", "h2 heading"),
    ("Heading 3", "sub_sub_header", "h3 heading"),
    ("Bulleted list", "bulleted_list", "bullet ul"),
    ("Numbered list", "numbered_list", "num ol"),
    ("Toggle list", "toggle", "toggle expand collapse"),
    ("Quote", "quote", "quote"),
    ("Callout", "callout", "callout"),
    ("Divider", "divider", "div hr separator"),
    ("Code", "code", "code"),
    ("Block equation", "equation", "equation latex math"),
    ("Image", "image", "image picture"),
    ("Table", "table", "table grid simple"),
    (
        "Database - Inline",
        "collection_view",
        "database table inline",
    ),
    (
        "Database - Full page",
        "collection_view_page",
        "database table full page",
    ),
];

/// Slash items whose title or keywords contain every word of `query`.
pub fn slash_matches(query: &str) -> Vec<usize> {
    let q = query.to_lowercase();
    SLASH_ITEMS
        .iter()
        .enumerate()
        .filter(|(_, (title, _, keys))| {
            let hay = format!("{} {keys}", title.to_lowercase());
            q.split_whitespace().all(|w| hay.contains(w))
        })
        .map(|(i, _)| i)
        .collect()
}

/// "Turn into" targets in menu order (cmd-alt-0..9 use the first ten).
pub const TURN_INTO: &[(&str, &str)] = &[
    ("Text", "text"),
    ("Heading 1", "header"),
    ("Heading 2", "sub_header"),
    ("Heading 3", "sub_sub_header"),
    ("To-do list", "to_do"),
    ("Bulleted list", "bulleted_list"),
    ("Numbered list", "numbered_list"),
    ("Toggle list", "toggle"),
    ("Code", "code"),
    ("Page", "page"),
    ("Quote", "quote"),
    ("Callout", "callout"),
];

/// Color ids (`teal` is shown as Green), light-theme hex.
pub const COLORS: &[(&str, &str, u32)] = &[
    ("default", "Default", 0x000000),
    ("gray", "Gray", 0x787774),
    ("brown", "Brown", 0x9f6b53),
    ("orange", "Orange", 0xd9730d),
    ("yellow", "Yellow", 0xcb912f),
    ("teal", "Green", 0x448361),
    ("blue", "Blue", 0x337ea9),
    ("purple", "Purple", 0x9065b0),
    ("pink", "Pink", 0xc14c8a),
    ("red", "Red", 0xd44c47),
];

/// Base hex of a color id (`red` or `red_background`).
pub fn color_hex(name: &str) -> Option<u32> {
    let base = name.strip_suffix("_background").unwrap_or(name);
    let base = match base {
        "green" => "teal",
        "grey" => "gray",
        b => b,
    };
    COLORS
        .iter()
        .find(|(id, _, _)| *id == base && *id != "default")
        .map(|c| c.2)
}

pub const LANGUAGES: &[&str] = &[
    "ABAP",
    "Agda",
    "Arduino",
    "Assembly",
    "Bash",
    "BASIC",
    "BNF",
    "C",
    "C#",
    "C++",
    "Clojure",
    "CoffeeScript",
    "Coq",
    "CSS",
    "Dart",
    "Dhall",
    "Diff",
    "Docker",
    "EBNF",
    "Elixir",
    "Elm",
    "Erlang",
    "F#",
    "Flow",
    "Fortran",
    "Gherkin",
    "GLSL",
    "Go",
    "GraphQL",
    "Groovy",
    "Haskell",
    "HCL",
    "HTML",
    "Idris",
    "Java",
    "JavaScript",
    "JSON",
    "Julia",
    "Kotlin",
    "LaTeX",
    "Less",
    "Lisp",
    "LiveScript",
    "LLVM IR",
    "Lua",
    "Makefile",
    "Markdown",
    "Markup",
    "MATLAB",
    "Mathematica",
    "Mermaid",
    "Nix",
    "Objective-C",
    "OCaml",
    "Pascal",
    "Perl",
    "PHP",
    "Plain Text",
    "PowerShell",
    "Prolog",
    "Protobuf",
    "PureScript",
    "Python",
    "R",
    "Racket",
    "Reason",
    "Ruby",
    "Rust",
    "Sass",
    "Scala",
    "Scheme",
    "Scss",
    "Shell",
    "Smalltalk",
    "Solidity",
    "SQL",
    "Swift",
    "TOML",
    "TypeScript",
    "VB.Net",
    "Verilog",
    "VHDL",
    "Visual Basic",
    "WebAssembly",
    "XML",
    "YAML",
];

pub const EMOJIS: &[&str] = &[
    "📄", "📝", "📌", "📎", "📚", "📖", "📒", "📓", "📅", "📊", "📈", "🗂️", "🗒️", "🗃️", "💡", "🔥",
    "⭐", "✨", "✅", "❗", "❓", "⚠️", "🚀", "🎯", "🏆", "🎉", "🎨", "🎵", "💻", "🖥️", "⌨️", "🔧",
    "🔨", "⚙️", "🧪", "🔬", "🧠", "💬", "📣", "🔔", "🔒", "🔑", "🏠", "🏢", "🌍", "✈️", "🚗", "☕",
    "🍎", "🍕", "🌱", "🌸", "🌙", "☀️", "❤️", "💙", "💚", "💛", "😀", "😎", "🤔", "👍", "👀", "🙌",
    "💰", "🛒", "🎁", "🧭",
];

/// Cover gradients (from, to) used for `cover = "gradient_<i>"`.
pub const GRADIENTS: &[(u32, u32)] = &[
    (0xf6d365, 0xfda085),
    (0x84fab0, 0x8fd3f4),
    (0xa18cd1, 0xfbc2eb),
    (0xfccb90, 0xd57eeb),
    (0x43e97b, 0x38f9d7),
    (0x667eea, 0x764ba2),
    (0xff9a9e, 0xfecfef),
    (0x30cfd0, 0x330867),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_shortcuts() {
        let kind = |p| detect_shortcut(p).map(|s| s.kind);
        assert_eq!(kind("# "), Some(BlockType::Header));
        assert_eq!(kind("### "), Some(BlockType::SubSubHeader));
        assert_eq!(kind("- "), Some(BlockType::BulletedList));
        assert_eq!(kind("> "), Some(BlockType::Toggle));
        assert_eq!(kind("\" "), Some(BlockType::Quote));
        assert_eq!(kind("```"), Some(BlockType::Code));
        assert_eq!(kind("---"), Some(BlockType::Divider));
        assert_eq!(kind("#"), None);
        assert_eq!(kind("hello # "), None);
        assert!(detect_shortcut("[x] ").unwrap().checked);
        assert_eq!(detect_shortcut("1. ").unwrap().start, None);
        assert_eq!(detect_shortcut("7) ").unwrap().start, Some(7));
        assert_eq!(kind("a. "), None);
    }

    #[test]
    fn numbering_restarts_after_other_blocks() {
        let items = [
            (true, None),
            (true, None),
            (false, None),
            (true, Some(5)),
            (true, None),
        ];
        assert_eq!(list_numbers(&items), vec![1, 2, 0, 5, 6]);
        assert_eq!(list_marker(3, 0), "3.");
        assert_eq!(list_marker(28, 1), "ab.");
        assert_eq!(list_marker(4, 2), "iv.");
    }

    #[test]
    fn slash_filter() {
        let titles: Vec<_> = slash_matches("h2")
            .iter()
            .map(|&i| SLASH_ITEMS[i].0)
            .collect();
        assert_eq!(titles, vec!["Heading 2"]);
        assert_eq!(slash_matches("").len(), SLASH_ITEMS.len());
        assert!(slash_matches("zzz").is_empty());
    }

    #[test]
    fn inline_markdown_rules() {
        assert_eq!(
            inline_markdown("a **bold**"),
            Some((2, 2, Annotation::Bold))
        );
        assert_eq!(inline_markdown("**bold*"), None);
        assert_eq!(inline_markdown("*it*"), Some((0, 1, Annotation::Italic)));
        assert_eq!(inline_markdown("x `c`"), Some((2, 1, Annotation::Code)));
        assert_eq!(inline_markdown("snake_case_"), None);
        assert_eq!(inline_markdown("* a *"), None);
    }
}
