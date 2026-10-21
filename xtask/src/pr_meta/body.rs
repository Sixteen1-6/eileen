//! The pull request description as check 8 reads it: HTML comments removed, sections split at
//! `## ` headings outside fenced code, the Type line, and the Evidence grammar.

/// The field lines the Evidence section may hold, at most one of each.
pub(crate) const FIELDS: [&str; 7] =
    ["tests", "scenarios", "numbers", "replay", "incident", "owner-run", "override"];

/// The pull request types, as written on the `type:` line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrType {
    /// New behaviour.
    Feature,
    /// A bug fix; it must carry a regression.
    Fix,
    /// Behaviour kept, code changed.
    Refactor,
    /// Documentation only.
    Docs,
    /// Scenario files only.
    Scenario,
    /// A change to a file another package owns or to the specification.
    Amendment,
    /// Dependency manifests, the lockfile and action pins only.
    Deps,
}

impl PrType {
    /// Every type, in the order the template lists them.
    pub(crate) const ALL: [Self; 7] = [
        Self::Feature,
        Self::Fix,
        Self::Refactor,
        Self::Docs,
        Self::Scenario,
        Self::Amendment,
        Self::Deps,
    ];

    /// The word on the `type:` line.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Feature => "feature",
            Self::Fix => "fix",
            Self::Refactor => "refactor",
            Self::Docs => "docs",
            Self::Scenario => "scenario",
            Self::Amendment => "amendment",
            Self::Deps => "deps",
        }
    }

    fn parse(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == word)
    }
}

/// A problem found while reading the description: the line it is on (1-based, if any), what is
/// wrong and how to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Problem {
    pub(crate) line: Option<usize>,
    pub(crate) what: String,
    pub(crate) fix: String,
}

impl Problem {
    pub(crate) fn new(
        line: Option<usize>,
        what: impl Into<String>,
        fix: impl Into<String>,
    ) -> Self {
        Self { line, what: what.into(), fix: fix.into() }
    }
}

/// One line of the description after comment removal.
#[derive(Debug, Clone)]
pub(crate) struct Line {
    /// 1-based line number in the description as given.
    pub(crate) number: usize,
    pub(crate) text: String,
    /// Inside a fenced code block, or a fence line itself.
    pub(crate) fenced: bool,
}

impl Line {
    /// Whether the line says anything: at least one letter or digit.
    fn has_words(&self) -> bool {
        self.text.chars().any(char::is_alphanumeric)
    }
}

/// A `## ` section: its heading text, the heading's line number and the lines under it.
#[derive(Debug, Clone)]
pub(crate) struct Section {
    pub(crate) heading: String,
    pub(crate) line: usize,
    pub(crate) lines: Vec<Line>,
}

impl Section {
    /// Whether any line under the heading says anything.
    pub(crate) fn has_words(&self) -> bool {
        self.lines.iter().any(Line::has_words)
    }
}

/// The description split into the text before the first heading and its sections.
#[derive(Debug, Clone)]
pub(crate) struct Description {
    pub(crate) preamble: Vec<Line>,
    pub(crate) sections: Vec<Section>,
}

impl Description {
    /// Parses a description: line endings normalised, HTML comments removed (keeping line
    /// numbers), and `## ` headings outside fenced code starting sections.
    pub(crate) fn parse(raw: &str) -> Self {
        let text = strip_comments(&raw.replace("\r\n", "\n").replace('\r', "\n"));
        let mut preamble = Vec::new();
        let mut sections: Vec<Section> = Vec::new();
        let mut fence: Option<(char, usize)> = None;
        for (index, text) in text.lines().enumerate() {
            let number = index + 1;
            let fenced = step_fence(&mut fence, text);
            if !fenced && let Some(heading) = heading_text(text) {
                sections.push(Section {
                    heading: heading.to_owned(),
                    line: number,
                    lines: Vec::new(),
                });
                continue;
            }
            let line = Line { number, text: text.to_owned(), fenced };
            match sections.last_mut() {
                Some(section) => section.lines.push(line),
                None => preamble.push(line),
            }
        }
        Self { preamble, sections }
    }

    /// Whether the text before the first heading says anything.
    pub(crate) fn preamble_has_words(&self) -> bool {
        self.preamble.iter().any(Line::has_words)
    }

    /// The first section with this heading.
    pub(crate) fn section(&self, heading: &str) -> Option<&Section> {
        self.sections.iter().find(|section| section.heading == heading)
    }
}

/// Removes `<!-- ... -->` comments, keeping their newlines so line numbers stay true. An
/// unterminated comment runs to the end, as GitHub renders it.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        let (before, comment) = rest.split_at(start);
        out.push_str(before);
        let comment = comment.strip_prefix("<!--").unwrap_or(comment);
        let (inside, after) = match comment.find("-->") {
            Some(end) => {
                let (inside, tail) = comment.split_at(end);
                (inside, tail.strip_prefix("-->").unwrap_or(tail))
            }
            None => (comment, ""),
        };
        out.extend(inside.chars().filter(|c| *c == '\n'));
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Tracks fenced code blocks (three or more backticks or tildes). Returns whether `line` is a
/// fence line or inside a block.
fn step_fence(fence: &mut Option<(char, usize)>, line: &str) -> bool {
    match (*fence, fence_marker(line)) {
        (None, Some((mark, len, _))) => {
            *fence = Some((mark, len));
            true
        }
        (Some((kind, len)), Some((mark, mark_len, true))) if kind == mark && mark_len >= len => {
            *fence = None;
            true
        }
        (Some(_), Some(_) | None) => true,
        (None, None) => false,
    }
}

/// The fence character, its run length and whether nothing but spaces follows it.
fn fence_marker(line: &str) -> Option<(char, usize, bool)> {
    let trimmed = line.trim_start_matches(' ');
    let mark = trimmed.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let len = trimmed.chars().take_while(|c| *c == mark).count();
    (len >= 3).then(|| {
        let rest = trimmed.trim_start_matches(mark);
        (mark, len, rest.trim().is_empty())
    })
}

/// The text of a `## ` heading (up to three leading spaces, as Markdown allows).
fn heading_text(line: &str) -> Option<&str> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let text = line.trim_start_matches(' ').strip_prefix("## ")?;
    Some(text.trim())
}

/// A field line of the Evidence section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Field {
    pub(crate) name: &'static str,
    pub(crate) value: String,
    pub(crate) line: usize,
}

/// The Evidence section read by its grammar: field lines, the count of prose lines that say
/// something, and the grammar problems found.
#[derive(Debug, Clone, Default)]
pub(crate) struct Evidence {
    pub(crate) fields: Vec<Field>,
    pub(crate) prose: usize,
    pub(crate) problems: Vec<Problem>,
}

impl Evidence {
    /// Reads the section. A field line starts at the beginning of a line, outside fenced code;
    /// everything else is prose. A `type:` line is reported and never counts as evidence.
    pub(crate) fn read(section: &Section) -> Self {
        let mut evidence = Self::default();
        for line in &section.lines {
            if !line.fenced {
                if let Some((name, value)) = field_line(&line.text) {
                    evidence.add_field(name, value, line.number);
                    continue;
                }
                if line.text.starts_with("type:") {
                    evidence.problems.push(Problem::new(
                        Some(line.number),
                        "a `type:` line sits in the Evidence section, and it is not evidence",
                        "move it under `## Type`",
                    ));
                    continue;
                }
            }
            if line.has_words() {
                evidence.prose += 1;
            }
        }
        evidence
    }

    fn add_field(&mut self, name: &'static str, value: String, line: usize) {
        if self.fields.iter().any(|field| field.name == name) {
            self.problems.push(Problem::new(
                Some(line),
                format!("the `{name}:` field appears more than once"),
                format!("merge them into one `{name}:` line, comma-separated where it lists ids"),
            ));
        } else if value.is_empty() {
            self.problems.push(Problem::new(
                Some(line),
                format!("the `{name}:` field is empty"),
                format!("fill it in or delete the `{name}:` line"),
            ));
        } else {
            self.fields.push(Field { name, value, line });
        }
    }

    /// The field with this name, if present.
    pub(crate) fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|field| field.name == name)
    }

    /// No prose that says anything and no field line: empty, or only the template's comment.
    pub(crate) const fn is_empty(&self) -> bool {
        self.prose == 0 && self.fields.is_empty()
    }
}

fn field_line(text: &str) -> Option<(&'static str, String)> {
    FIELDS.into_iter().find_map(|name| {
        let value = text.strip_prefix(name)?.strip_prefix(':')?;
        Some((name, value.trim().to_owned()))
    })
}

/// The type list as the fix messages print it.
pub(crate) fn type_list() -> String {
    let words: Vec<&str> = PrType::ALL.iter().map(|kind| kind.as_str()).collect();
    words.join(", ")
}

/// Reads the Type section: exactly one line, `type: <value>`.
pub(crate) fn read_type(section: &Section) -> Result<PrType, Problem> {
    let said: Vec<&Line> =
        section.lines.iter().filter(|line| !line.text.trim().is_empty()).collect();
    let fix = format!("write one line, `type: <value>`, with one of: {}", type_list());
    match said.as_slice() {
        [] => Err(Problem::new(Some(section.line), "the Type section is empty", fix)),
        [line] => {
            let Some(value) = line.text.trim().strip_prefix("type:").map(str::trim) else {
                return Err(Problem::new(
                    Some(line.number),
                    "the Type section's line is not `type: <value>`",
                    fix,
                ));
            };
            if value.is_empty() {
                return Err(Problem::new(Some(line.number), "no type is chosen", fix));
            }
            PrType::parse(value).ok_or_else(|| {
                Problem::new(Some(line.number), format!("`{value}` is not a type"), fix)
            })
        }
        [_, extra, ..] => Err(Problem::new(
            Some(extra.number),
            "the Type section holds more than its one `type:` line",
            "keep only `type: <value>` there; explanations belong under What or Why",
        )),
    }
}
