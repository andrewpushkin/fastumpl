//! Which kind of diagram a PlantUML file holds.
//!
//! PlantUML tries its readers in turn (sequence first, then class,
//! activity, description, state, …) and uses the first that accepts every
//! line. This copies that order with the lines that tell the kinds apart:
//! a file is a sequence diagram when it has sequence lines and nothing a
//! sequence diagram cannot hold, and so on. Lines that any kind accepts
//! (notes, titles, styling, plain arrows) decide nothing.

/// The kind of a PlantUML diagram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagramKind {
    /// Class or object diagram.
    Class,
    /// Component, deployment or use case diagram (also C4): boxes and
    /// lines, read as classes with the kind as stereotype.
    Description,
    Sequence,
    Activity,
    State,
    Timing,
    /// Not a UML diagram: `@startmindmap`, `@startgantt`, `@startjson`, …
    /// The word after `@start`.
    Other(&'static str),
    /// No `@start` line and nothing to read.
    Empty,
}

impl DiagramKind {
    pub fn name(self) -> &'static str {
        match self {
            DiagramKind::Class => "class diagram",
            DiagramKind::Description => "component, deployment or use case diagram",
            DiagramKind::Sequence => "sequence diagram",
            DiagramKind::Activity => "activity diagram",
            DiagramKind::State => "state diagram",
            DiagramKind::Timing => "timing diagram",
            DiagramKind::Other(w) => w,
            DiagramKind::Empty => "empty",
        }
    }

    /// Whether fastumpl reads this kind into boxes and lines.
    pub fn readable(self) -> bool {
        matches!(
            self,
            DiagramKind::Class
                | DiagramKind::Description
                | DiagramKind::Sequence
                | DiagramKind::Empty
        )
    }
}

/// The kinds of `@start…` other than `@startuml`.
const OTHER: [&str; 20] = [
    "mindmap",
    "gantt",
    "wbs",
    "json",
    "yaml",
    "salt",
    "ditaa",
    "dot",
    "math",
    "latex",
    "ebnf",
    "regex",
    "chen",
    "files",
    "chronology",
    "board",
    "wire",
    "creole",
    "flow",
    "nwdiag",
];

/// The kind of the first diagram in `src`, before preprocessing.
pub fn diagram_kind(src: &str) -> DiagramKind {
    let src = super::one_line_end(src);
    let mut lines = src.lines().map(str::trim);
    let has_start = src.lines().any(|l| l.trim_start().starts_with("@start"));
    let tag = if has_start {
        let start = lines.find(|l| l.starts_with("@start")).unwrap_or("");
        start_tag(start)
    } else {
        "uml"
    };
    if tag != "uml" {
        return other(tag);
    }
    let body: Vec<&str> = lines.take_while(|l| !l.starts_with("@end")).collect();
    kind_of_lines(&body)
}

/// `uml` for `@startuml`, `mindmap` for `@startmindmap(id=x)`, …
pub fn start_tag(line: &str) -> &str {
    let rest = line.trim().trim_start_matches("@start");
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    &rest[..end]
}

pub fn other(tag: &str) -> DiagramKind {
    let lower = tag.to_ascii_lowercase();
    match OTHER.iter().find(|w| **w == lower) {
        Some(w) => DiagramKind::Other(w),
        None if lower == "uml" => DiagramKind::Class,
        None => DiagramKind::Other("unknown diagram"),
    }
}

const SEQUENCE: [&str; 20] = [
    "participant",
    "activate",
    "deactivate",
    "destroy",
    "autonumber",
    "alt",
    "loop",
    "opt",
    "par",
    "break",
    "critical",
    "group",
    "ref",
    "return",
    "create",
    "box",
    "hnote",
    "rnote",
    "newpage",
    "ignore",
];
/// Participants of sequence diagrams that also name boxes elsewhere.
const PARTICIPANT: [&str; 8] = [
    "actor",
    "boundary",
    "control",
    "entity",
    "database",
    "collections",
    "queue",
    "participant",
];
const CLASS: [&str; 15] = [
    "class",
    "abstract",
    "interface",
    "enum",
    "annotation",
    "struct",
    "record",
    "exception",
    "metaclass",
    "protocol",
    "dataclass",
    "object",
    "map",
    "package",
    "namespace",
];
const ACTIVITY: [&str; 16] = [
    "start",
    "stop",
    "endif",
    "while",
    "endwhile",
    "repeat",
    "fork",
    "split",
    "detach",
    "kill",
    "elseif",
    "switch",
    "case",
    "endswitch",
    "backward",
    "partition",
];
const DESCRIPTION: [&str; 16] = [
    "usecase",
    "component",
    "node",
    "artifact",
    "cloud",
    "folder",
    "frame",
    "storage",
    "agent",
    "rectangle",
    "card",
    "file",
    "stack",
    "hexagon",
    "person",
    "port",
];

#[derive(Default)]
struct Signals {
    sequence: usize,
    /// Lines a sequence diagram cannot hold.
    not_sequence: usize,
    class: usize,
    /// Declarations only class diagrams have (`class X`, `enum E`, …), as
    /// opposed to arrows other diagrams share.
    class_decl: usize,
    activity: usize,
    state: usize,
    timing: usize,
    description: usize,
    /// Lines a class diagram rejects (unless `allowmixing`) and a
    /// component, deployment or use case diagram reads: boxes such as
    /// `node N` or `actor A` (not the groups `node N {`), and the short
    /// forms `[C]`, `(U)`, `:A:`.
    desc_only: usize,
    /// `allowmixing`: a class diagram that also holds those.
    mixing: bool,
}

/// The kind of a diagram from the lines between `@startuml` and `@enduml`.
pub fn kind_of_lines(lines: &[&str]) -> DiagramKind {
    let mut s = Signals::default();
    let mut in_comment = false;
    let mut depth_body = 0usize;
    // The closing line of a block whose text says nothing about the kind:
    // a note, a legend, a style sheet.
    let mut skip_until: Option<&str> = None;
    let mut skip_braces = 0usize;
    // Inside an activity's action over several lines: `:Look for` … `it);`.
    let mut in_action = false;
    let action_ends = |t: &str| t.ends_with([';', '|', '<', '>', '/', '\\', ']', '}']);
    // Graphviz text: `digraph G {` as the first statement.
    let first_statement = lines.iter().map(|l| l.trim()).find(|t| {
        !t.is_empty() && !t.starts_with('\'') && !t.starts_with('!') && !t.starts_with("/'")
    });
    let is_dot = |f: &str| {
        let f = f.to_ascii_lowercase();
        let f = f.strip_prefix("strict ").unwrap_or(&f);
        (f.starts_with("digraph") || f.starts_with("graph "))
            && f.ends_with('{')
            && f.split_whitespace().count() <= 3
    };
    if first_statement.is_some_and(is_dot) || lines.iter().any(|l| is_dot(l.trim())) {
        return DiagramKind::Other("dot");
    }
    // Lines ending in `\` go on on the next one.
    let mut joined: Vec<String> = Vec::with_capacity(lines.len());
    let mut open = false;
    for l in lines {
        let t = l.trim();
        if open {
            let last = joined.last_mut().unwrap();
            last.push_str(t);
        } else {
            joined.push(t.to_string());
        }
        open = t.ends_with('\\') && !t.ends_with("\\\\");
        if open {
            joined.last_mut().unwrap().pop();
        }
    }
    for l in &joined {
        let t = l.trim();
        if in_comment {
            in_comment = !t.contains("'/");
            continue;
        }
        if let Some(end) = skip_until {
            let lower = t.to_ascii_lowercase();
            if lower == end || lower.replace(' ', "") == end.replace(' ', "") {
                skip_until = None;
            }
            continue;
        }
        if in_action {
            in_action = !action_ends(t);
            continue;
        }
        if skip_braces > 0 {
            skip_braces =
                (skip_braces + t.matches('{').count()).saturating_sub(t.matches('}').count());
            continue;
        }
        {
            let lower = t.to_ascii_lowercase();
            let w = lower.split_whitespace().next().unwrap_or("");
            if ["note", "hnote", "rnote"].contains(&w)
                && !lower.contains(':')
                && !lower.contains(" as ")
            {
                skip_until = Some("end note");
                // Still a sequence line when it is over participants.
                if lower.starts_with("note over") || w != "note" {
                    s.sequence += 1;
                }
                continue;
            }
            if w == "legend" {
                skip_until = Some("endlegend");
                continue;
            }
            if lower.starts_with("<style>") {
                if !lower.contains("</style>") {
                    skip_until = Some("</style>");
                }
                continue;
            }
            if w == "skinparam" && t.ends_with('{') {
                skip_braces = 1;
                continue;
            }
        }
        if let Some(rest) = t.strip_prefix("/'") {
            in_comment = !rest.contains("'/");
            continue;
        }
        if t.is_empty() || t.starts_with('\'') || t.starts_with('!') {
            continue;
        }
        // `+class Inner {`: a visibility before the keyword.
        let t = match t.strip_prefix(['+', '-', '#', '~']) {
            Some(rest) if rest.starts_with(|c: char| c.is_ascii_lowercase()) => rest,
            _ => t,
        };
        let lower = t.to_ascii_lowercase();
        if lower == "allowmixing" || lower == "allow_mixing" {
            s.mixing = true;
            continue;
        }
        let first = lower
            .split(|c: char| c.is_whitespace() || c == '{' || c == '(' || c == '"')
            .next()
            .unwrap_or("");
        // Inside a `{ … }` body of a class: members, not statements.
        if depth_body > 0 {
            depth_body =
                depth_body + t.matches('{').count() - t.matches('}').count().min(depth_body);
            continue;
        }
        // A keyword followed by an arrow is a name: `Interface -> UI`,
        // `Storage --> Backend`, `Case "1" *-- File`, `State ..> AI`.
        let names_a_box = arrow_of(t).is_some();
        let first = if names_a_box { "" } else { first };
        if CLASS.contains(&first) {
            let container = first == "package" || first == "namespace";
            s.class += 1;
            // Component diagrams have `interface I` too (not with a body).
            let shared = first == "interface" && !t.contains('{');
            s.class_decl += usize::from(!container && !shared);
            s.not_sequence += 1;
            // A class body holds members; a package body statements.
            if t.ends_with('{') && !container {
                depth_body = 1;
            }
            // `package P` without a body is a box, which only component
            // diagrams have.
            s.desc_only += usize::from(first == "package" && !t.contains('{'));
            continue;
        }
        if PARTICIPANT.contains(&first) {
            if t.ends_with('{') {
                // `entity X {` (ER) or `database X {` (deployment).
                s.class += usize::from(first == "entity");
                s.class_decl += usize::from(first == "entity");
                s.not_sequence += 1;
                depth_body = usize::from(first == "entity");
            } else {
                // A participant of a sequence diagram, or else a box of a
                // component diagram (`entity E` is a class too).
                s.sequence += 1;
                s.desc_only += usize::from(first != "entity" && first != "participant");
            }
            continue;
        }
        // Messages from or to outside: `[-> A`, `[o->> A`, `A ->o]`.
        let outside = (t.starts_with('[') && t[1..].starts_with(['-', '<', 'o', 'x', '/', '\\']))
            || (t.ends_with(']') && {
                let body = t.trim_end_matches(']');
                body.ends_with(['>', 'o', 'x', '/', '\\']) && body.contains('-')
            });
        if SEQUENCE.contains(&first)
            || outside
            || lower.starts_with("==")
            || lower.starts_with("...")
            || lower.starts_with("|||")
            || lower.starts_with("note over")
            || lower == "end"
            || lower.starts_with("end box")
            || (first == "else" && !else_activity(&lower))
        {
            s.sequence += 1;
            continue;
        }
        // `|Lane|` or `|#pink|Lane|`, not a table row `|= A |= B |` or
        // rule `|----|----|`.
        let swimlane = t.starts_with('|')
            && !t.starts_with("||")
            && !t[1..].starts_with([' ', '='])
            && t.ends_with('|')
            && (2..=3).contains(&t.matches('|').count())
            && t.chars().any(char::is_alphanumeric);
        // `:action;`, or `#pink:action;` in a colour; not a label
        // `: "text"` on its own line.
        let action = {
            let a = match t.strip_prefix('#') {
                Some(rest) => rest.find(':').map_or("", |i| &rest[i..]),
                None => t,
            };
            a.starts_with(':')
                && !a[1..].starts_with(char::is_whitespace)
                && (a.ends_with(';') || !a[1..].contains(':'))
        };
        if action && !action_ends(t) {
            in_action = true;
        }
        if (ACTIVITY.contains(&first) && !names_a_box)
            || action
            || (first == "if" && lower.contains("then"))
            || (first == "else" && else_activity(&lower))
            || swimlane
            || t.starts_with("(*)")
            || lower.starts_with("end fork")
            || lower.starts_with("end split")
        {
            s.activity += 1;
            s.not_sequence += 1;
            continue;
        }
        if (first == "state" && !names_a_box) || t.contains("[*]") {
            s.state += 1;
            s.not_sequence += 1;
            continue;
        }
        if ["robust", "concise", "clock", "binary", "analog"].contains(&first) || t.starts_with('@')
        {
            s.timing += 1;
            s.not_sequence += 1;
            continue;
        }
        // `(A, B) .. C`: an association class, in a class diagram.
        let association_class =
            t.starts_with('(') && t[..t.find(')').unwrap_or(t.len())].contains(',');
        if DESCRIPTION.contains(&first)
            || (t.starts_with('(') && !association_class)
            || t.starts_with('[')
            || (t.starts_with(':') && t[1..].contains(':'))
            || is_macro_call(t)
        {
            s.description += 1;
            s.not_sequence += 1;
            // Groups (`node N {`) are read by class diagrams too.
            s.desc_only += usize::from(!t.ends_with('{'));
            continue;
        }
        if let Some(arrow) = arrow_of(t) {
            // `A --> [Comp]`, `User --> (Use case)`.
            let target = t
                .find(arrow.as_str())
                .map(|i| t[i + arrow.len()..].trim_start())
                .unwrap_or("");
            let target = match target.strip_prefix('"').and_then(|r| r.split_once('"')) {
                Some((_, after)) => after.trim_start(),
                None => target,
            };
            if target.starts_with(['[', '(']) {
                s.desc_only += 1;
                s.not_sequence += 1;
            }
            if class_arrow(&arrow) {
                s.class += 1;
                s.not_sequence += 1;
            } else if t.contains(':') {
                // `A -> B : message` reads as a message or a labelled line.
                s.sequence += 1;
            }
            if t.contains("-(")
                || t.contains("- (")
                || t.contains("-[") && t.contains(']') && !arrow.contains('[')
            {
                s.description += 1;
            }
        }
    }
    if s.sequence > 0 && s.not_sequence == 0 {
        return DiagramKind::Sequence;
    }
    // A class diagram cannot hold activity or state lines; arrows alone
    // do not make one.
    if s.class_decl == 0 && s.activity > 0 {
        return DiagramKind::Activity;
    }
    if s.class_decl == 0 && s.state > 0 {
        return DiagramKind::State;
    }
    // PlantUML tries the class reader before the component one: a class
    // diagram unless a line only the component reader takes.
    if s.desc_only > 0 && !s.mixing && s.class_decl == 0 {
        return DiagramKind::Description;
    }
    if s.class > 0 {
        return DiagramKind::Class;
    }
    if s.activity > 0 {
        return DiagramKind::Activity;
    }
    if s.description > 0 {
        return DiagramKind::Class;
    }
    if s.state > 0 {
        return DiagramKind::State;
    }
    if s.timing > 0 {
        return DiagramKind::Timing;
    }
    if s.class == 0
        && s.sequence == 0
        && lines.iter().all(|l| {
            let t = l.trim();
            t.is_empty() || t.starts_with('\'')
        })
    {
        return DiagramKind::Empty;
    }
    DiagramKind::Class
}

/// `Person(user, "User")`, `Rel(a, b, "uses")`: a call of a procedure
/// (C4 and other libraries), at the start of a line.
fn is_macro_call(t: &str) -> bool {
    let t = t.strip_prefix('$').unwrap_or(t);
    let name_end = t
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(t.len());
    name_end > 0
        && t[name_end..].starts_with('(')
        && t.chars().next().is_some_and(|c| c.is_alphabetic())
        && (t.trim_end().ends_with(')') || t.trim_end().ends_with('{'))
}

/// The arrow of a relation line, roughly: the first run of `-`, `.`, `<`,
/// `>`, `|`, `*`, `o`, `x`, `#` around at least one `-` or `.`.
fn arrow_of(t: &str) -> Option<String> {
    let chars: Vec<char> = t.chars().collect();
    let mut i = 0;
    // Skip the first name (quoted or plain).
    if chars.first() == Some(&'"') {
        i = 1 + chars[1..].iter().position(|c| *c == '"')? + 1;
    } else {
        while i < chars.len() && (chars[i].is_alphanumeric() || "_.:$".contains(chars[i])) {
            i += 1;
        }
    }
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    // A multiplicity in quotes.
    if chars.get(i) == Some(&'"') {
        i += 1 + chars[i + 1..].iter().position(|c| *c == '"')? + 1;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
    }
    let start = i;
    let mut seen_line = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '-' || c == '.' {
            seen_line = true;
        } else if c == '[' {
            match chars[i..].iter().position(|c| *c == ']') {
                Some(j) => i += j,
                None => break,
            }
        } else if !("<>|*ox#\\/+^{}".contains(c)
            || c.is_alphabetic()
                && seen_line
                && chars.get(i + 1).is_some_and(|n| *n == '-' || *n == '.'))
        {
            break;
        }
        i += 1;
    }
    let arrow: String = chars[start..i].iter().collect();
    (seen_line && !arrow.is_empty()).then_some(arrow)
}

/// An arrow only class diagrams have: inheritance, composition,
/// aggregation, dotted lines, or a line without heads.
/// `else (no)` of an activity diagram, as opposed to `else other case`
/// of a sequence diagram.
fn else_activity(lower: &str) -> bool {
    lower
        .strip_prefix("else")
        .is_some_and(|r| r.trim_start().starts_with('('))
}

fn class_arrow(a: &str) -> bool {
    // Styles such as `-[#red]->` say nothing.
    let mut plain = String::new();
    let mut depth = 0;
    for c in a.chars() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    let a = plain.as_str();
    // A lost message: `->x`, `-x`.
    if a.ends_with('x') || a.starts_with('x') {
        return false;
    }
    a.contains('|')
        || a.contains('*')
        || a.contains('.')
        || a.starts_with('o')
        || a.ends_with('o') && !a.contains('>')
        || a.contains('{') // `}o--o{` crow's feet
        || !(a.contains('>') || a.contains('<'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(src: &str) -> DiagramKind {
        diagram_kind(&format!("@startuml\n{src}\n@enduml\n"))
    }

    #[test]
    fn tells_kinds_apart() {
        use DiagramKind::*;
        assert_eq!(kind("class A\nA --> B"), Class);
        assert_eq!(kind("A <|-- B"), Class);
        assert_eq!(kind("A --> B : uses\nclass A {\n  +x: int\n}"), Class);
        assert_eq!(kind("entity User {\n  id\n}\nUser ||--o{ Order"), Class);
        assert_eq!(kind("Alice -> Bob : hello\nBob --> Alice : hi"), Sequence);
        assert_eq!(kind("actor User\nparticipant App\nUser -> App : login\nactivate App\nalt ok\nApp --> User\nend"), Sequence);
        assert_eq!(
            kind("start\n:Hello;\nif (x) then (yes)\n:A;\nendif\nstop"),
            Activity
        );
        assert_eq!(kind("[*] --> Idle\nstate Idle\nIdle --> Busy : go"), State);
        assert_eq!(
            kind("actor User\nUser --> (Login)\nusecase (Logout)"),
            Description
        );
        assert_eq!(kind(":Admin: as A\nA --> (Use)"), Description);
        assert_eq!(kind("start\n:multi\nline;\nstop"), Activity);
        assert_eq!(
            kind("start\n:Look at each\nclass that has it);\n#pink:Done|\nstop"),
            Activity
        );
        // Classes named like keywords, with lines to them.
        assert_eq!(kind("Case \"1\" *-- \"many\" CaseFile : contains"), Class);
        assert_eq!(kind("State ..> StateAI : Own\nState <|-- Menu"), Class);
        assert_eq!(kind("STOP --> CLI : await\nclass CLI"), Class);
        assert_eq!(kind("start -[#blue;dashed]- CS\nclass CS"), Class);
        // A table in a class diagram is not a swimlane; a label on its own
        // line is not an action.
        assert_eq!(kind("class A\n|= Color |= Layer |\n| red | api |"), Class);
        assert_eq!(kind("A --> B\n: \"<<inject>>\"\nclass A"), Class);
        assert_eq!(kind("|Lane|\nstart\n:a;\nstop"), Activity);
        assert_eq!(kind("entity A {\n  id\n}\n|----|----|"), Class);
        // As PlantUML decides: the class reader takes groups (`node N {`)
        // but not boxes (`node N`, `[C]`) unless `allowmixing`.
        assert_eq!(
            kind("package P {\n  node N {\n    [S]\n  }\n}"),
            Description
        );
        assert_eq!(kind("package P {\n  database D\n}\nP --> D"), Description);
        assert_eq!(kind("node N {\n  A --> B\n}"), Class);
        assert_eq!(kind("allowmixing\nclass A\nactor U\nU --> A"), Class);
        assert_eq!(kind("actor X\nA --> X"), Sequence);
        assert_eq!(kind("class A\n(A, B) .. C"), Class);
        assert_eq!(kind("storage S\ninterface I\nS --> I"), Description);
        assert_eq!(
            kind("a.B -- c.D\npackage a {\n  +class B {\n  }\n}\nnode N"),
            Class
        );
        // Classes all inside packages; a byte order mark; old Mac line ends.
        assert_eq!(kind("package p {\n  class A\n}\nstart"), Class);
        assert_eq!(diagram_kind("\u{feff}@startuml\nclass A\n@enduml\n"), Class);
        assert_eq!(
            diagram_kind("@startuml\rclass A\rA --> B\r@enduml\r"),
            Class
        );
        assert_eq!(
            kind("participant UI\nInterface -> UI : GET\nStorage --> UI: path\nalt ok\nUI -> UI\nelse failed (late)\nUI -> UI\nend"),
            Sequence
        );
        assert_eq!(
            kind("[o->> Alice : found\nactivate Alice\nAlice ->o] : lost"),
            Sequence
        );
        assert_eq!(kind("box \"x\"\n  participant A\nend box\nnote right of A\n  {\n  \"a\": 1,\n  }\nend note\nA -[#red]x B : no"), Sequence);
        assert_eq!(kind("digraph G {\n  a -> b\n}"), Other("dot"));
        assert_eq!(
            kind("title x\nskinparam a b\ndigraph g {\n  rankdir=\"TB\"\n}"),
            Other("dot")
        );
        assert_eq!(
            kind("state \"New\" as N\n[*] -right-> N\nN --> Done : go\nN -[#red]-> X"),
            State
        );
        assert_eq!(
            kind("[Web] --> [Db]\nnode Server {\n  [App]\n}"),
            Description
        );
        assert_eq!(
            kind("Person(user, \"User\")\nSystem(sys, \"Sys\")\nRel(user, sys, \"Uses\")"),
            Description
        );
        assert_eq!(kind("robust \"Web\" as WB\n@0\nWB is Idle"), Timing);
        assert_eq!(kind(""), Empty);
        assert_eq!(
            diagram_kind("@startmindmap\n* a\n@endmindmap\n"),
            Other("mindmap")
        );
        assert_eq!(diagram_kind("@startgantt\n@endgantt\n"), Other("gantt"));
        assert_eq!(diagram_kind("class A\n"), Class);
        assert!(Class.readable() && Description.readable() && Sequence.readable());
        assert!(!Activity.readable());
    }
}
