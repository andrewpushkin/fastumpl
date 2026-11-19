//! Sequence diagrams, read as PlantUML reads them: participants, and the
//! messages, notes, fragments and other steps between them, top to bottom
//! (see [`umpl_core::sequence`]). Lines that every kind of diagram shares
//! (`title`, `skinparam`, `legend`, comments, …) are read by the importer
//! as for class diagrams.

use umpl_core::sequence::*;
use umpl_core::Sequence;

use super::{decorations, is_end, strip_format, Importer, Mode};
use crate::scan::Cursor;

/// A sequence diagram being read.
pub(super) struct SeqState {
    pub seq: Sequence,
    /// Calls not answered yet, the last one on top: (called, caller).
    calls: Vec<(usize, Option<MessageEnd>)>,
    /// The `box` being declared: its index in `seq.boxes`.
    open_box: Option<usize>,
    /// `autoactivate on`: a call activates whom it calls, a reply ends it.
    autoactivate: bool,
}

impl SeqState {
    pub fn new() -> SeqState {
        SeqState {
            seq: Sequence::new(),
            calls: Vec::new(),
            open_box: None,
            autoactivate: false,
        }
    }

    /// The participant called `name`, added (as a plain participant) the
    /// first time it is named.
    fn participant(&mut self, name: &str) -> usize {
        if let Some(i) = self.seq.participant(name) {
            return i;
        }
        self.seq.participants.push(Participant {
            name: name.to_string(),
            shown: name.to_string(),
            ..Participant::default()
        });
        let i = self.seq.participants.len() - 1;
        if let Some(b) = self.open_box {
            self.seq.boxes[b].last = i;
        }
        i
    }

    fn last_message(&self) -> Option<&Message> {
        self.seq.steps.iter().rev().find_map(|s| match s {
            Step::Message(m) => Some(m),
            _ => None,
        })
    }

    pub fn finish(mut self) -> Sequence {
        if let Some(b) = self.open_box.take() {
            self.close_box(b);
        }
        self.seq
    }

    fn close_box(&mut self, b: usize) {
        let n = self.seq.participants.len();
        let bx = &self.seq.boxes[b];
        if bx.first >= n || bx.last < bx.first {
            self.seq.boxes.remove(b);
        }
    }
}

/// The kinds of fragment: `alt` … `end`.
const FRAGMENTS: [&str; 8] = [
    "alt", "opt", "loop", "par", "par2", "break", "critical", "group",
];

/// A block of lines a sequence statement opens.
pub(super) enum SeqMode {
    /// A note's text, up to `end note`.
    Note {
        at: NoteAt,
        shape: NoteShape,
        color: Option<String>,
        beside: bool,
        lines: Vec<String>,
    },
    /// `ref over A, B` … `end ref`.
    Ref {
        first: usize,
        last: usize,
        lines: Vec<String>,
    },
    /// A participant's text in brackets: `participant P [` … `]`.
    Participant(usize, Vec<String>),
}

/// Characters of a participant's code name (PlantUML: letters, digits,
/// `_`, `.`, `@`).
fn name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '@' | '$')
}

/// A participant name: quoted, or a run of name characters.
fn seq_name(c: &mut Cursor) -> Option<String> {
    c.skip_ws();
    if c.rest().starts_with('"') {
        return c.string().map(|s| strip_format(&s).into_owned());
    }
    let len: usize = c
        .rest()
        .chars()
        .take_while(|&ch| name_char(ch))
        .map(char::len_utf8)
        .sum();
    if len == 0 {
        return None;
    }
    let name = c.rest()[..len].to_string();
    c.pos += len;
    Some(name)
}

/// `word` (in any case) next, ending where a name could not go on:
/// `right:`, `over#red`, `note<<style>>`.
fn word(c: &mut Cursor, w: &str) -> bool {
    c.skip_ws();
    let r = c.rest();
    let Some(head) = r.get(..w.len()) else {
        return false;
    };
    if !head.eq_ignore_ascii_case(w) {
        return false;
    }
    let after = &r[w.len()..];
    if after
        .chars()
        .next()
        .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
    {
        return false;
    }
    c.pos += w.len();
    true
}

/// A note's colour, `#pink` or `#black:` right before its text.
fn note_color(c: &mut Cursor) -> Option<String> {
    c.skip_ws();
    if !c.rest().starts_with('#') {
        return decorations(c);
    }
    let len: usize = c
        .rest()
        .chars()
        .take_while(|ch| !ch.is_whitespace() && *ch != ':')
        .map(char::len_utf8)
        .sum();
    let token = c.rest()[..len].trim_start_matches('#').to_ascii_lowercase();
    c.pos += len;
    (!token.is_empty() && token.chars().all(|ch| ch.is_ascii_alphanumeric())).then_some(token)
}

/// Text after `:` (or a note's line): `\n` breaks lines, creole
/// formatting is dropped.
fn text_of(s: &str) -> String {
    // `\l` and `\r` also break a line (aligned left or right).
    s.replace("\\n", "\n")
        .replace("\\l", "\n")
        .replace("\\r", "\n")
        .split('\n')
        .map(|l| strip_format(l.trim()).trim().to_string())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end_matches('\n')
        .to_string()
}

/// The lines of a note or ref written over several lines.
fn block_text(lines: &[String]) -> String {
    let lines: Vec<String> = lines.iter().map(|l| text_of(l)).collect();
    let first = lines.iter().position(|l| !l.is_empty()).unwrap_or(0);
    let last = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(0, |i| i + 1);
    lines[first..last.max(first)].join("\n")
}

/// An arrow of a sequence diagram, as read.
#[derive(Debug, Default, PartialEq)]
struct Arrow {
    /// Head at the left (`<-`): the message goes right to left.
    left: Option<Head>,
    right: Option<Head>,
    dashed: bool,
    color: Option<String>,
}

/// Reads an arrow at the start of `s`: `->`, `-->`, `->>`, `-\`, `-//`,
/// `->x`, `o->o`, `<->`, `<<--`, `-[#red]>`, `-[#red,bold]->`, `->(10)`.
/// Returns it and its length in bytes.
fn read_arrow(s: &str) -> Option<(Arrow, usize)> {
    let b = s.as_bytes();
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    let mut i = 0;
    let mut a = Arrow::default();
    let mut left_deco = None;
    if matches!(at(0), b'o' | b'x') && matches!(at(1), b'<' | b'-' | b'\\' | b'/') {
        left_deco = Some(if at(0) == b'x' {
            Head::Lost
        } else {
            Head::Circle
        });
        i += 1;
    }
    // `[hidden]->`, `[#red]->`: a style before the shaft.
    if at(i) == b'[' {
        let close = s[i..].find(']')?;
        if s.as_bytes().get(i + close + 1) != Some(&b'-') {
            return None;
        }
        let style = &s[i + 1..i + close];
        if let Some(color) = style.split(',').map(str::trim).find(|p| p.starts_with('#')) {
            a.color = Some(color.trim_start_matches('#').to_ascii_lowercase());
        }
        i += close + 1;
    }
    // Head at the left.
    if s[i..].starts_with("<<") {
        a.left = Some(Head::Open);
        i += 2;
    } else if at(i) == b'<' {
        a.left = Some(Head::Filled);
        i += 1;
    } else if s[i..].starts_with("\\\\") || s[i..].starts_with("//") {
        a.left = Some(Head::Half);
        i += 2;
    } else if matches!(at(i), b'\\' | b'/') {
        a.left = Some(Head::Half);
        i += 1;
    }
    // The shaft: dashes, with a style in brackets anywhere.
    let mut dashes = 0;
    loop {
        if at(i) == b'-' {
            dashes += 1;
            i += 1;
        } else if at(i) == b'[' && (dashes > 0 || a.left.is_some()) {
            let close = s[i..].find(']')?;
            let style = &s[i + 1..i + close];
            if let Some(color) = style.split(',').map(str::trim).find(|p| p.starts_with('#')) {
                a.color = Some(color.trim_start_matches('#').to_ascii_lowercase());
            }
            if style.contains("dashed") || style.contains("dotted") {
                a.dashed = true;
            }
            i += close + 1;
        } else {
            break;
        }
    }
    if dashes == 0 {
        return None;
    }
    a.dashed |= dashes >= 2;
    // Head at the right.
    if s[i..].starts_with(">>") {
        a.right = Some(Head::Open);
        i += 2;
    } else if at(i) == b'>' {
        a.right = Some(Head::Filled);
        i += 1;
    } else if s[i..].starts_with("\\\\") || s[i..].starts_with("//") {
        a.right = Some(Head::Half);
        i += 2;
    } else if matches!(at(i), b'\\' | b'/') {
        a.right = Some(Head::Half);
        i += 1;
    }
    // `->x`, `->o`: only when no name goes on right after it.
    if matches!(at(i), b'x' | b'o')
        && !s[i + 1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
    {
        let deco = if at(i) == b'x' {
            Head::Lost
        } else {
            Head::Circle
        };
        if a.right.is_none() || deco == Head::Lost {
            a.right = Some(deco);
        }
        i += 1;
    }
    // `x-`, `o-` alone: the mark is the head, at the left. With a head
    // (`o->`), it is only a circle at the start.
    if a.left.is_none() && a.right.is_none() {
        a.left = left_deco;
    }
    // `->(10)`: a slanted arrow.
    if at(i) == b'(' {
        if let Some(close) = s[i..].find(')') {
            if s[i + 1..i + close]
                .trim()
                .chars()
                .all(|c| c.is_ascii_digit())
            {
                i += close + 1;
            }
        }
    }
    if a.left.is_none() && a.right.is_none() {
        return None;
    }
    Some((a, i))
}

/// One end of a message as written: a participant's name, or an edge
/// of the diagram (`[`, `]`, `?`).
enum Written {
    Name(String),
    Left,
    Right,
    /// `?`: an edge, the one on its side of the arrow.
    Short,
}

fn written_end(c: &mut Cursor) -> Option<Written> {
    c.skip_ws();
    if c.eat("[") {
        return Some(Written::Left);
    }
    if c.eat("]") {
        return Some(Written::Right);
    }
    if c.eat("?") {
        return Some(Written::Short);
    }
    seq_name(c).map(Written::Name)
}

impl Importer {
    fn seq(&mut self) -> &mut SeqState {
        self.seq.as_mut().expect("reading a sequence diagram")
    }

    fn end_of(&mut self, w: Written, short: MessageEnd) -> MessageEnd {
        match w {
            Written::Name(n) => MessageEnd::Participant(self.seq().participant(&n)),
            Written::Left => MessageEnd::Left,
            Written::Right => MessageEnd::Right,
            Written::Short => short,
        }
    }

    /// A line of a sequence diagram. False when it is none of its own
    /// statements (it may still be a line every diagram has).
    pub(super) fn seq_statement(&mut self, t: &str) -> bool {
        self.seq_keyword(t) || self.seq_participant(t) || self.seq_note(t) || self.seq_message(t)
    }

    /// Lines of a block a sequence statement opened.
    pub(super) fn seq_mode_line(&mut self, t: &str, mode: SeqMode) {
        match mode {
            SeqMode::Note {
                at,
                shape,
                color,
                beside,
                mut lines,
            } => {
                let end = ["note", "hnote", "rnote"].iter().any(|w| is_end(t, w));
                if end {
                    self.seq().seq.steps.push(Step::Note {
                        at,
                        text: block_text(&lines),
                        shape,
                        color,
                        beside_previous: beside,
                    });
                } else {
                    lines.push(t.to_string());
                    self.mode = Some(Mode::Seq(SeqMode::Note {
                        at,
                        shape,
                        color,
                        beside,
                        lines,
                    }));
                }
            }
            SeqMode::Ref {
                first,
                last,
                mut lines,
            } => {
                // `end ref`, or `end` alone.
                if is_end(t, "ref") || t.trim().eq_ignore_ascii_case("end") {
                    let text = block_text(&lines);
                    self.seq().seq.steps.push(Step::Ref { first, last, text });
                } else {
                    lines.push(t.to_string());
                    self.mode = Some(Mode::Seq(SeqMode::Ref { first, last, lines }));
                }
            }
            SeqMode::Participant(i, mut lines) => {
                if t.trim() == "]" {
                    let shown: Vec<String> = lines
                        .iter()
                        .map(|l| l.trim())
                        .filter(|l| !l.is_empty() && !l.chars().all(|c| "-=._".contains(c)))
                        .map(|l| text_of(l.trim_start_matches('=').trim()))
                        .collect();
                    if !shown.is_empty() {
                        self.seq().seq.participants[i].shown = shown.join("\n");
                    }
                } else {
                    lines.push(t.to_string());
                    self.mode = Some(Mode::Seq(SeqMode::Participant(i, lines)));
                }
            }
        }
    }

    /// A block left open at the end of the file.
    pub(super) fn seq_unfinished(&mut self, mode: SeqMode) -> &'static str {
        let what = match &mode {
            SeqMode::Note { .. } => "missing `end note` at end of file",
            SeqMode::Ref { .. } => "missing `end ref` at end of file",
            SeqMode::Participant(..) => "missing `]` at end of file",
        };
        let end = match &mode {
            SeqMode::Note { .. } => "end note",
            SeqMode::Ref { .. } => "end ref",
            SeqMode::Participant(..) => "]",
        };
        self.seq_mode_line(end, mode);
        what
    }

    /// Statements that start with a keyword (or a mark such as `==`).
    fn seq_keyword(&mut self, t: &str) -> bool {
        let lower = t.to_ascii_lowercase();
        let first = lower
            .split(|c: char| c.is_whitespace() || c == '#' || c == ':')
            .next()
            .unwrap_or("");
        let after = t[first.len().min(t.len())..].trim();
        // A keyword used as a name (`loop -> A`) is a message.
        if read_arrow(after.trim_start()).is_some() {
            return false;
        }
        // `== Section ==`
        if t.starts_with("==") && t.ends_with("==") && t.len() >= 4 {
            let text = t.trim_matches('=').trim();
            self.seq().seq.steps.push(Step::Divider(text_of(text)));
            return true;
        }
        // `...`, `... 5 minutes later ...`
        if t.starts_with("...") {
            let text = t.trim_matches('.').trim();
            self.seq().seq.steps.push(Step::Delay(text_of(text)));
            return true;
        }
        // `|||`, `||45||`
        if t.starts_with("||") && t.ends_with("||") {
            let inner = t.trim_matches('|').trim();
            let space = inner.parse::<u32>().ok();
            if inner.is_empty() || space.is_some() {
                self.seq().seq.steps.push(Step::Space(space));
                return true;
            }
        }
        // `{start}`, `{end} <-> …`: teoz time constraints.
        if t.starts_with('{') && t.contains('}') {
            let rest = t[t.find('}').unwrap() + 1..].trim();
            if rest.is_empty() || rest.starts_with('<') {
                return true;
            }
        }
        match first {
            "activate" | "deactivate" | "destroy" => {
                let mut c = Cursor::new(after);
                let Some(name) = seq_name(&mut c) else {
                    // `deactivate` alone: the last one activated.
                    if first == "deactivate" {
                        if let Some((who, _)) = self.seq().calls.pop() {
                            self.seq().seq.steps.push(Step::Deactivate { who });
                        }
                        return true;
                    }
                    return false;
                };
                let color = decorations(&mut c);
                let who = self.seq().participant(&name);
                let st = self.seq();
                let step = match first {
                    "activate" => {
                        let caller = st
                            .last_message()
                            .filter(|m| m.to == Some(MessageEnd::Participant(who)))
                            .and_then(|m| m.from);
                        st.calls.push((who, caller));
                        Step::Activate { who, color }
                    }
                    "deactivate" => {
                        if let Some(k) = st.calls.iter().rposition(|(w, _)| *w == who) {
                            st.calls.remove(k);
                        }
                        Step::Deactivate { who }
                    }
                    _ => {
                        st.calls.retain(|(w, _)| *w != who);
                        Step::Destroy { who }
                    }
                };
                st.seq.steps.push(step);
                true
            }
            "autonumber" => {
                self.seq()
                    .seq
                    .steps
                    .push(Step::Autonumber(after.to_string()));
                true
            }
            "autoactivate" => {
                self.seq().autoactivate = after.eq_ignore_ascii_case("on");
                true
            }
            "return" => {
                let text = text_of(after);
                let st = self.seq();
                let (from, to) = match st.calls.pop() {
                    Some((callee, caller)) => {
                        // Activated without a call to it: back to whoever
                        // last sent it something, else off the left edge.
                        let caller = caller.or_else(|| {
                            st.seq.steps.iter().rev().find_map(|s| match s {
                                Step::Message(m)
                                    if m.to == Some(MessageEnd::Participant(callee)) =>
                                {
                                    m.from
                                }
                                _ => None,
                            })
                        });
                        (
                            Some(MessageEnd::Participant(callee)),
                            Some(caller.unwrap_or(MessageEnd::Left)),
                        )
                    }
                    None => match st.last_message() {
                        Some(m) => (m.to, m.from),
                        None => return false,
                    },
                };
                st.seq.steps.push(Step::Message(Message {
                    from,
                    to,
                    text,
                    dashed: true,
                    head: Head::Filled,
                    ..Message::default()
                }));
                if let Some(MessageEnd::Participant(who)) = from {
                    st.seq.steps.push(Step::Deactivate { who });
                }
                true
            }
            "ref" => {
                let Some(rest) = after
                    .strip_prefix("over")
                    .or_else(|| after.strip_prefix("OVER"))
                else {
                    return false;
                };
                let (who, text) = match rest.split_once(':') {
                    Some((w, x)) => (w, Some(x)),
                    None => (rest, None),
                };
                let mut c = Cursor::new(who);
                let mut list = Vec::new();
                while let Some(name) = seq_name(&mut c) {
                    list.push(self.seq().participant(&name));
                    c.skip_ws();
                    if !c.eat(",") {
                        break;
                    }
                }
                let (Some(&first), Some(&last)) = (list.iter().min(), list.iter().max()) else {
                    return false;
                };
                match text {
                    Some(x) => {
                        let text = text_of(x);
                        self.seq().seq.steps.push(Step::Ref { first, last, text });
                    }
                    None => {
                        self.mode = Some(Mode::Seq(SeqMode::Ref {
                            first,
                            last,
                            lines: Vec::new(),
                        }))
                    }
                }
                true
            }
            "box" => {
                let mut c = Cursor::new(after);
                c.skip_ws();
                let title = if c.rest().starts_with('"') {
                    c.string().map(|s| text_of(&s))
                } else {
                    None
                };
                let color = decorations(&mut c);
                c.skip_ws();
                let title = title.or_else(|| {
                    let rest = c.rest().trim();
                    (!rest.is_empty()).then(|| text_of(rest))
                });
                let st = self.seq();
                let first = st.seq.participants.len();
                st.seq.boxes.push(ParticipantBox {
                    title,
                    color,
                    first,
                    last: first.wrapping_sub(1),
                });
                st.open_box = Some(st.seq.boxes.len() - 1);
                true
            }
            "endbox" => {
                if let Some(b) = self.seq().open_box.take() {
                    self.seq().close_box(b);
                }
                true
            }
            "end" if lower.split_whitespace().nth(1) == Some("box") => {
                if let Some(b) = self.seq().open_box.take() {
                    self.seq().close_box(b);
                }
                true
            }
            _ if FRAGMENTS.contains(&first) => {
                let mut c = Cursor::new(after);
                let mut color = None;
                loop {
                    c.skip_ws();
                    if c.rest().starts_with('#') {
                        let start = c.pos;
                        while c.rest().starts_with(|ch: char| !ch.is_whitespace()) {
                            c.pos += c.rest().chars().next().unwrap().len_utf8();
                        }
                        let token = &after[start..c.pos];
                        color = color.or_else(|| {
                            let v = token.trim_start_matches('#');
                            (!v.is_empty()).then(|| v.to_ascii_lowercase())
                        });
                    } else {
                        break;
                    }
                }
                let label = text_of(c.rest());
                self.seq().seq.steps.push(Step::FragmentStart {
                    kind: first.trim_end_matches('2').to_string(),
                    label,
                    color,
                });
                true
            }
            "else" => {
                // `else #color label`: the colour only styles the part.
                let label = match after.strip_prefix('#') {
                    Some(rest) => rest.split_once(char::is_whitespace).map_or("", |(_, l)| l),
                    None => after,
                };
                let label = text_of(label);
                self.seq().seq.steps.push(Step::Else { label });
                true
            }
            // `end`, or `end alt`, `end loop`, … naming what it ends.
            "end"
                if after.is_empty() || FRAGMENTS.contains(&after.to_ascii_lowercase().as_str()) =>
            {
                self.seq().seq.steps.push(Step::FragmentEnd);
                true
            }
            "hide" | "show" => {
                let what = after.to_ascii_lowercase();
                if what == "footbox" {
                    self.seq().seq.footbox = first == "show";
                    return true;
                }
                // `hide unlinked` and the like only style the drawing.
                what.split_whitespace().all(|w| {
                    [
                        "unlinked",
                        "stereotype",
                        "stereotypes",
                        "circle",
                        "<<",
                        "empty",
                    ]
                    .iter()
                    .any(|k| w.starts_with(k))
                })
            }
            "ignore" if after.eq_ignore_ascii_case("newpage") => true,
            // A stray `end note` after a note already closed: PlantUML
            // passes over it.
            "end" | "endnote" | "endhnote" | "endrnote"
                if ["note", "hnote", "rnote", ""]
                    .contains(&after.to_ascii_lowercase().as_str()) =>
            {
                true
            }
            _ => false,
        }
    }

    /// `participant "Long name" as L #color`, `actor Bob`, `create C`,
    /// `participant P [` … `]`.
    fn seq_participant(&mut self, t: &str) -> bool {
        let mut c = Cursor::new(t);
        let created = c.eat_word("create");
        c.skip_ws();
        let word: String = c
            .rest()
            .chars()
            .take_while(|ch| ch.is_alphabetic())
            .collect();
        let kind = ParticipantKind::from_keyword(&word);
        if kind.is_some() {
            c.pos += word.len();
        } else if !created {
            return false;
        }
        // A keyword followed by an arrow is a participant of that name.
        if read_arrow(c.rest().trim_start()).is_some() {
            return false;
        }
        let quoted_first = c.rest().trim_start().starts_with('"');
        let Some(first) = seq_name(&mut c) else {
            return false;
        };
        c.skip_ws();
        let (name, shown) = if c.eat_word("as") {
            c.skip_ws();
            let quoted_second = c.rest().starts_with('"');
            let Some(second) = seq_name(&mut c) else {
                return false;
            };
            if quoted_second && !quoted_first {
                (first, second)
            } else {
                (second, first)
            }
        } else {
            (first.clone(), first)
        };
        let mut stereotype = None;
        let mut color = None;
        let mut bracket = false;
        loop {
            c.skip_ws();
            if c.rest().starts_with("<<") {
                stereotype = c.stereotype();
            } else if c.rest().starts_with('#') {
                color = decorations(&mut c).or(color);
            } else if c.eat_word("order") {
                c.skip_ws();
                let _ = c.int();
            } else if c.eat("[") {
                bracket = true;
            } else {
                break;
            }
        }
        if !c.at_end() {
            return false;
        }
        let st = self.seq();
        let i = st.participant(&name);
        let p = &mut st.seq.participants[i];
        p.shown = text_of(&shown);
        if let Some(k) = kind {
            p.kind = k;
        }
        p.stereotype = stereotype.or(p.stereotype.take());
        p.color = color.or(p.color.take());
        p.created |= created;
        if bracket {
            self.mode = Some(Mode::Seq(SeqMode::Participant(i, Vec::new())));
        }
        true
    }

    /// `note left of A : text`, `note over A, B`, `hnote across`, `/ note …`;
    /// over several lines up to `end note`.
    fn seq_note(&mut self, t: &str) -> bool {
        let mut c = Cursor::new(t);
        let beside = c.eat("/");
        c.skip_ws();
        let shape = if word(&mut c, "note") {
            NoteShape::Folded
        } else if word(&mut c, "hnote") {
            NoteShape::Hexagon
        } else if word(&mut c, "rnote") {
            NoteShape::Rectangle
        } else {
            return false;
        };
        // `note<<style>> right`: a style for the note only.
        c.skip_ws();
        if c.rest().starts_with("<<") {
            c.stereotype();
        }
        let side = ["left", "right", "over", "across"]
            .into_iter()
            .find(|w| word(&mut c, w));
        let Some(side) = side else {
            return false;
        };
        let st = self.seq();
        // The previous message, for a note without `of`.
        let ends: Vec<usize> = st
            .last_message()
            .map(|m| {
                [m.from, m.to]
                    .into_iter()
                    .flatten()
                    .filter_map(|e| match e {
                        MessageEnd::Participant(i) => Some(i),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let at = match side {
            "across" => NoteAt::Across,
            "over" => {
                let mut list = Vec::new();
                loop {
                    let Some(name) = seq_name(&mut c) else { break };
                    list.push(self.seq().participant(&name));
                    c.skip_ws();
                    if !c.eat(",") {
                        break;
                    }
                }
                match (list.iter().min(), list.iter().max()) {
                    (Some(&a), Some(&b)) => NoteAt::Over(a, b),
                    _ => return false,
                }
            }
            _ => {
                c.skip_ws();
                // `note right of A`, also `note right A`.
                let named = if word(&mut c, "of") {
                    match seq_name(&mut c) {
                        Some(name) => Some(name),
                        None => return false,
                    }
                } else {
                    seq_name(&mut c)
                };
                let target = if let Some(name) = named {
                    Some(self.seq().participant(&name))
                } else if side == "left" {
                    ends.iter().min().copied()
                } else {
                    ends.iter().max().copied()
                };
                let target = target.unwrap_or_else(|| {
                    if side == "left" {
                        0
                    } else {
                        self.seq().seq.participants.len().saturating_sub(1)
                    }
                });
                if self.seq().seq.participants.is_empty() {
                    NoteAt::Across
                } else if side == "left" {
                    NoteAt::Left(target)
                } else {
                    NoteAt::Right(target)
                }
            }
        };
        let color = note_color(&mut c);
        c.skip_ws();
        if c.eat(":") {
            let text = text_of(c.rest());
            self.seq().seq.steps.push(Step::Note {
                at,
                text,
                shape,
                color,
                beside_previous: beside,
            });
        } else if c.at_end() {
            self.mode = Some(Mode::Seq(SeqMode::Note {
                at,
                shape,
                color,
                beside,
                lines: Vec::new(),
            }));
        } else {
            return false;
        }
        true
    }

    /// `A -> B : text`, `B --> A`, `[-> A`, `A ->]`, `A <- B`, with
    /// `++`, `--`, `**`, `!!` after the target.
    fn seq_message(&mut self, t: &str) -> bool {
        let mut s = t.trim_start_matches('&').trim();
        // `{start} A -> B`: a teoz anchor.
        if s.starts_with('{') {
            match s.find('}') {
                Some(i) => s = s[i + 1..].trim(),
                None => return false,
            }
        }
        let mut c = Cursor::new(s);
        c.skip_ws();
        // No sender: from the left edge, as `[->`.
        let from = if read_arrow(c.rest()).is_some() {
            Written::Short
        } else {
            match written_end(&mut c) {
                Some(w) => w,
                None => return false,
            }
        };
        c.skip_ws();
        let Some((arrow, len)) = read_arrow(c.rest()) else {
            return false;
        };
        c.pos += len;
        c.skip_ws();
        let to = match written_end(&mut c) {
            Some(w) => w,
            // `A ->`: to the right edge.
            None => Written::Short,
        };
        // `++ #color`, `--`, `**`, `!!` in any order.
        let (mut activate, mut deactivate, mut create, mut destroy) = (false, false, false, false);
        let mut act_color = None;
        loop {
            c.skip_ws();
            if c.eat("++") {
                activate = true;
            } else if c.eat("--") {
                deactivate = true;
            } else if c.eat("**") {
                create = true;
            } else if c.eat("!!") {
                destroy = true;
            } else if c.rest().starts_with('#') && activate {
                act_color = decorations(&mut c);
            } else {
                break;
            }
        }
        c.skip_ws();
        let text = if c.eat(":") {
            text_of(c.rest())
        } else if c.at_end() {
            String::new()
        } else {
            return false;
        };
        let a = self.end_of(from, MessageEnd::Left);
        let b = self.end_of(to, MessageEnd::Right);
        // Written right to left (`A <- B`): B sends to A.
        let (from, to, head, both) = match (arrow.left, arrow.right) {
            (Some(l), None) => (b, a, l, false),
            (Some(_), Some(r)) => (a, b, r, true),
            (None, r) => (a, b, r.unwrap_or_default(), false),
        };
        let st = self.seq();
        if create {
            if let MessageEnd::Participant(i) = to {
                st.seq.participants[i].created = true;
            }
        }
        st.seq.steps.push(Step::Message(Message {
            from: Some(from),
            to: Some(to),
            text,
            dashed: arrow.dashed,
            head,
            both,
            color: arrow.color,
        }));
        let auto_call = st.autoactivate && !arrow.dashed;
        let auto_reply = st.autoactivate && arrow.dashed;
        if let MessageEnd::Participant(callee) = to {
            if activate || (auto_call && from != to) {
                st.calls.push((callee, Some(from)));
                st.seq.steps.push(Step::Activate {
                    who: callee,
                    color: act_color,
                });
            }
        }
        if let MessageEnd::Participant(sender) = from {
            if deactivate || (auto_reply && st.calls.last().is_some_and(|(w, _)| *w == sender)) {
                if let Some(k) = st.calls.iter().rposition(|(w, _)| *w == sender) {
                    st.calls.remove(k);
                }
                st.seq.steps.push(Step::Deactivate { who: sender });
            }
        }
        if destroy {
            if let MessageEnd::Participant(who) = to {
                st.calls.retain(|(w, _)| *w != who);
                st.seq.steps.push(Step::Destroy { who });
            }
        }
        true
    }
}

/// The participants a step names.
fn involved(step: &Step) -> Vec<usize> {
    let p = |e: &Option<MessageEnd>| match e {
        Some(MessageEnd::Participant(i)) => Some(*i),
        _ => None,
    };
    match step {
        Step::Message(m) => [p(&m.from), p(&m.to)].into_iter().flatten().collect(),
        Step::Note { at, .. } => match *at {
            NoteAt::Left(i) | NoteAt::Right(i) => vec![i],
            NoteAt::Over(a, b) => vec![a, b],
            NoteAt::Across => Vec::new(),
        },
        Step::Activate { who, .. } | Step::Deactivate { who } | Step::Destroy { who } => {
            vec![*who]
        }
        Step::Ref { first, last, .. } => vec![*first, *last],
        _ => Vec::new(),
    }
}

/// `seq` as PlantUML text, a whole `@startuml` … `@enduml` block.
pub fn print(seq: &Sequence, title: Option<&str>) -> String {
    let mut out = String::from("@startuml\n");
    if let Some(t) = title {
        out.push_str(&format!("title {}\n", t.replace('\n', "\\n")));
    }
    if !seq.footbox {
        out.push_str("hide footbox\n");
    }
    let simple = |s: &str| !s.is_empty() && s.chars().all(name_char);
    let name = |i: usize| {
        let n = &seq.participants[i].name;
        if simple(n) {
            n.clone()
        } else {
            format!("\"{n}\"")
        }
    };
    let text = |s: &str| s.replace('\n', "\\n");
    // A participant's declaration: `actor "Shown" as Name #color`.
    let declare = |i: usize| {
        let p = &seq.participants[i];
        let mut line = p.kind.keyword().to_string();
        if p.shown == p.name {
            line.push_str(&format!(" {}", name(i)));
        } else if simple(&p.name) {
            line.push_str(&format!(" \"{}\" as {}", text(&p.shown), p.name));
        } else {
            line.push_str(&format!(" \"{}\" as \"{}\"", text(&p.shown), p.name));
        }
        if let Some(s) = &p.stereotype {
            line.push_str(&format!(" <<{s}>>"));
        }
        if let Some(c) = &p.color {
            line.push_str(&format!(" #{c}"));
        }
        line
    };
    // Participants are declared in their order: up to the first one
    // created part way at the top, the others where first needed, so a
    // created one comes in its place (`create` goes right before the
    // message that creates it).
    let n = seq.participants.len();
    let declare_at = |out: &mut String, k: usize, create: bool| {
        for b in &seq.boxes {
            if b.first == k {
                out.push_str("box");
                if let Some(t) = &b.title {
                    out.push_str(&format!(" \"{}\"", text(t)));
                }
                if let Some(c) = &b.color {
                    out.push_str(&format!(" #{c}"));
                }
                out.push('\n');
            }
        }
        if create {
            out.push_str("create ");
        }
        out.push_str(&declare(k));
        out.push('\n');
        for b in &seq.boxes {
            if b.last == k {
                out.push_str("end box\n");
            }
        }
    };
    let first_created = seq.participants.iter().position(|p| p.created).unwrap_or(n);
    for k in 0..first_created {
        declare_at(&mut out, k, false);
    }
    let mut declared = first_created;
    let end = |e: Option<MessageEnd>, left: bool| match e {
        Some(MessageEnd::Participant(i)) => name(i),
        Some(MessageEnd::Left) => "[".to_string(),
        Some(MessageEnd::Right) => "]".to_string(),
        None => (if left { "[" } else { "]" }).to_string(),
    };
    let names = |a: usize, b: usize| {
        if a == b {
            name(a)
        } else {
            format!("{}, {}", name(a), name(b))
        }
    };
    let mut created = vec![false; n];
    for step in &seq.steps {
        // The participant this message creates, if any.
        let creates = match step {
            Step::Message(Message {
                to: Some(MessageEnd::Participant(j)),
                ..
            }) if seq.participants[*j].created && !created[*j] => Some(*j),
            _ => None,
        };
        if let Some(last) = involved(step).into_iter().max() {
            while declared <= last {
                let here = creates == Some(declared);
                declare_at(&mut out, declared, here);
                created[declared] |= here;
                declared += 1;
            }
        }
        if let Some(j) = creates.filter(|j| !created[*j]) {
            created[j] = true;
            out.push_str(&format!("create {}\n", declare(j)));
        }
        match step {
            Step::Message(m) => {
                let shaft = match (&m.color, m.dashed) {
                    (Some(c), true) => format!("-[#{c}]-"),
                    (Some(c), false) => format!("-[#{c}]"),
                    (None, true) => "--".to_string(),
                    (None, false) => "-".to_string(),
                };
                let head = match m.head {
                    Head::Filled => ">",
                    Head::Open => ">>",
                    Head::Half => "\\",
                    Head::Lost => ">x",
                    Head::Circle => ">o",
                };
                let back = if m.both { "<" } else { "" };
                // `[->` and `->]` keep the edge next to the arrow.
                let (a, b) = (end(m.from, true), end(m.to, false));
                let a_gap = if a == "[" { "" } else { " " };
                let b_gap = if b == "]" { "" } else { " " };
                out.push_str(&format!("{a}{a_gap}{back}{shaft}{head}{b_gap}{b}"));
                if !m.text.is_empty() {
                    out.push_str(&format!(" : {}", text(&m.text)));
                }
                out.push('\n');
            }
            Step::Note {
                at,
                text: t,
                shape,
                color,
                beside_previous,
            } => {
                let mut line = String::new();
                if *beside_previous {
                    line.push_str("/ ");
                }
                line.push_str(match shape {
                    NoteShape::Folded => "note",
                    NoteShape::Hexagon => "hnote",
                    NoteShape::Rectangle => "rnote",
                });
                match at {
                    NoteAt::Left(i) => line.push_str(&format!(" left of {}", name(*i))),
                    NoteAt::Right(i) => line.push_str(&format!(" right of {}", name(*i))),
                    NoteAt::Over(a, b) => line.push_str(&format!(" over {}", names(*a, *b))),
                    NoteAt::Across => line.push_str(" across"),
                }
                if let Some(c) = color {
                    line.push_str(&format!(" #{c}"));
                }
                if t.contains('\n') {
                    out.push_str(&format!("{line}\n"));
                    for l in t.lines() {
                        out.push_str(&format!("  {l}\n"));
                    }
                    out.push_str("end note\n");
                } else {
                    out.push_str(&format!("{line} : {t}\n"));
                }
            }
            Step::FragmentStart { kind, label, color } => {
                let mut line = kind.clone();
                if let Some(c) = color {
                    line.push_str(&format!(" #{c}"));
                }
                if !label.is_empty() {
                    line.push_str(&format!(" {}", text(label)));
                }
                out.push_str(&format!("{line}\n"));
            }
            Step::Else { label } if label.is_empty() => out.push_str("else\n"),
            Step::Else { label } => out.push_str(&format!("else {}\n", text(label))),
            Step::FragmentEnd => out.push_str("end\n"),
            Step::Divider(t) => out.push_str(&format!("== {} ==\n", text(t))),
            Step::Delay(t) if t.is_empty() => out.push_str("...\n"),
            Step::Delay(t) => out.push_str(&format!("...{}...\n", text(t))),
            Step::Space(None) => out.push_str("|||\n"),
            Step::Space(Some(px)) => out.push_str(&format!("||{px}||\n")),
            Step::Activate { who, color } => {
                out.push_str(&format!("activate {}", name(*who)));
                if let Some(c) = color {
                    out.push_str(&format!(" #{c}"));
                }
                out.push('\n');
            }
            Step::Deactivate { who } => out.push_str(&format!("deactivate {}\n", name(*who))),
            Step::Destroy { who } => out.push_str(&format!("destroy {}\n", name(*who))),
            Step::Ref {
                first,
                last,
                text: t,
            } => {
                if t.contains('\n') {
                    out.push_str(&format!("ref over {}\n", names(*first, *last)));
                    for l in t.lines() {
                        out.push_str(&format!("  {l}\n"));
                    }
                    out.push_str("end ref\n");
                } else {
                    out.push_str(&format!("ref over {} : {t}\n", names(*first, *last)));
                }
            }
            Step::Autonumber(args) if args.is_empty() => out.push_str("autonumber\n"),
            Step::Autonumber(args) => out.push_str(&format!("autonumber {args}\n")),
        }
    }
    out.push_str("@enduml\n");
    out
}
