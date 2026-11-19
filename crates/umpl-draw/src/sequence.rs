//! Draws a sequence diagram: participants in a row, each with a dashed
//! lifeline, and the steps below them, one row each, top to bottom.
//!
//! Nothing overlaps by construction. Each step gets its own row, as tall
//! as its text. The gap between two lifelines is the widest of what must
//! fit between them: the boxes, a message's label, a note beside one of
//! them, a self-message. Positions follow the longest-path rule: going left
//! to right, each lifeline sits as far left as every need towards a
//! lifeline before it allows. Text that a lifeline it does not belong to
//! crosses (a long message over several participants) is drawn on a patch
//! of the background, so the line stops short of it.

use std::collections::HashMap;

use umpl_core::sequence::*;
use umpl_core::{Element, ElementKind, Member, Sequence};

use crate::measure::{box_content, text_width, LABEL_LINE, LABEL_SIZE};
use crate::scene::{draw_box, RectF, Scene, Shape, TextStyle, TITLE_ROOM, TITLE_SIZE};
use crate::theme::{parse_paint, Color, Paint, Theme};
use crate::GRID;

const MARGIN: f32 = 24.0;
/// Least room between two participants' boxes.
const HEAD_GAP: f32 = 36.0;
/// Room around a message's label, inside the two lifelines.
const LABEL_PAD: f32 = 14.0;
/// Width of an activation bar; a nested one is shifted by half of it.
const BAR_W: f32 = 10.0;
const ROW_GAP: f32 = 14.0;
const SELF_W: f32 = 34.0;
const SELF_H: f32 = 16.0;
const NOTE_GAP: f32 = 8.0;
/// Height of a fragment's or ref's name tab.
const TAB_H: f32 = 20.0;
/// Room a fragment's frame leaves around what is inside.
const FRAME_PAD: f32 = 12.0;
const HEAD_SIZE: f32 = 9.0;
const STROKE: f32 = 1.3;

/// The box a participant is drawn as: a component-style box with an icon
/// for its kind.
fn head_element(p: &Participant) -> Element {
    let mut lines = p.shown.lines();
    let mut el = Element::new(ElementKind::Class, lines.next().unwrap_or(&p.name));
    el.members = lines.map(|l| Member::plain(l.to_string())).collect();
    let look = match p.kind {
        ParticipantKind::Participant | ParticipantKind::Entity => "rectangle",
        ParticipantKind::Actor | ParticipantKind::Boundary | ParticipantKind::Control => "actor",
        ParticipantKind::Database => "database",
        ParticipantKind::Collections | ParticipantKind::Queue => "queue",
    };
    el.stereotype = Some(look.to_string());
    el.color = p.color.clone();
    el
}

fn note_element(text: &str, color: &Option<String>) -> Element {
    let mut el = Element::new(ElementKind::Note, "");
    el.members = text.lines().map(|l| Member::plain(l.to_string())).collect();
    el.color = color.clone();
    el
}

fn size_px(el: &Element) -> (f32, f32) {
    let (w, h) = box_content(el).size();
    (w as f32 * GRID, h as f32 * GRID)
}

fn label_w(text: &str) -> f32 {
    text.lines()
        .map(|l| text_width(l, LABEL_SIZE))
        .fold(0.0, f32::max)
}

fn label_h(text: &str) -> f32 {
    if text.is_empty() {
        0.0
    } else {
        text.lines().count() as f32 * LABEL_LINE
    }
}

/// `autonumber` and its arguments: the number of the next message.
#[derive(Clone)]
struct Numbering {
    on: bool,
    next: i64,
    step: i64,
    format: Option<String>,
}

impl Numbering {
    fn apply(&mut self, args: &str) {
        let args = args.trim();
        let lower = args.to_ascii_lowercase();
        if lower == "stop" {
            self.on = false;
            return;
        }
        let mut rest = args;
        if lower.starts_with("resume") {
            rest = args[6..].trim();
        } else {
            self.next = 1;
            self.step = 1;
            self.format = None;
        }
        let mut numbers = Vec::new();
        for word in rest.split_whitespace() {
            if word.starts_with('"') {
                break;
            }
            match word.parse::<i64>() {
                Ok(v) => numbers.push(v),
                Err(_) => break,
            }
        }
        if lower.starts_with("resume") {
            if let Some(&s) = numbers.first() {
                self.step = s;
            }
        } else {
            if let Some(&v) = numbers.first() {
                self.next = v;
            }
            if let Some(&s) = numbers.get(1) {
                self.step = s;
            }
        }
        if let (Some(a), Some(b)) = (rest.find('"'), rest.rfind('"')) {
            if b > a {
                self.format = Some(rest[a + 1..b].to_string());
            }
        }
        self.on = true;
    }

    /// The next number, as its format shows it (`<b>[000]` gives `[007]`).
    fn take(&mut self) -> Option<String> {
        if !self.on {
            return None;
        }
        let n = self.next;
        self.next += self.step;
        let Some(format) = &self.format else {
            return Some(n.to_string());
        };
        let plain = strip_tags(format);
        let start = plain.find(['0', '#']);
        Some(match start {
            Some(s) => {
                let len = plain[s..]
                    .chars()
                    .take_while(|c| *c == '0' || *c == '#')
                    .count();
                let zeros = plain[s..s + len].matches('0').count();
                format!(
                    "{}{:0width$}{}",
                    &plain[..s],
                    n,
                    &plain[s + len..],
                    width = zeros
                )
            }
            None => format!("{plain}{n}"),
        })
    }
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in s.chars() {
        match c {
            '<' => inside = true,
            '>' if inside => inside = false,
            c if !inside => out.push(c),
            _ => {}
        }
    }
    out
}

/// Lifeline x positions, and the diagram's left and right edges.
struct Columns {
    x: Vec<f32>,
    left: f32,
    right: f32,
}

fn columns(seq: &Sequence, heads: &[(f32, f32)], notes: &HashMap<usize, (f32, f32)>) -> Columns {
    let n = seq.participants.len();
    // need[(i, j)]: least distance from lifeline i to lifeline j (i < j).
    let mut need: HashMap<(usize, usize), f32> = HashMap::new();
    let mut want = |i: usize, j: usize, d: f32| {
        let (a, b) = (i.min(j), i.max(j));
        if a != b {
            let e = need.entry((a, b)).or_insert(0.0);
            *e = e.max(d);
        }
    };
    for i in 1..n {
        want(i - 1, i, heads[i - 1].0 / 2.0 + heads[i].0 / 2.0 + HEAD_GAP);
    }
    // Room beyond the first and last lifeline.
    let mut left_room = heads.first().map_or(0.0, |h| h.0 / 2.0);
    let mut right_room = heads.last().map_or(0.0, |h| h.0 / 2.0);
    let mut numbering = Numbering {
        on: false,
        next: 1,
        step: 1,
        format: None,
    };
    for (k, step) in seq.steps.iter().enumerate() {
        match step {
            Step::Message(m) => {
                let number = numbering.take();
                let text_w = label_w(&m.text)
                    + number
                        .as_deref()
                        .map_or(0.0, |s| text_width(s, LABEL_SIZE) + 6.0);
                let room = text_w + 2.0 * LABEL_PAD + BAR_W;
                match (m.from, m.to) {
                    (Some(MessageEnd::Participant(a)), Some(MessageEnd::Participant(b)))
                        if a == b =>
                    {
                        let w = SELF_W.max(text_w) + LABEL_PAD + BAR_W;
                        if a + 1 < n {
                            want(a, a + 1, w);
                        } else {
                            right_room = right_room.max(w);
                        }
                    }
                    (Some(MessageEnd::Participant(a)), Some(MessageEnd::Participant(b))) => {
                        want(a, b, room)
                    }
                    (Some(MessageEnd::Participant(_)), _)
                    | (_, Some(MessageEnd::Participant(_))) => {
                        let to_left = matches!(m.from, Some(MessageEnd::Left))
                            || matches!(m.to, Some(MessageEnd::Left));
                        if to_left {
                            left_room = left_room.max(room + MARGIN);
                        } else {
                            right_room = right_room.max(room + MARGIN);
                        }
                    }
                    _ => {}
                }
            }
            Step::Autonumber(args) => numbering.apply(args),
            Step::Note { at, .. } => {
                let (w, _) = notes[&k];
                match *at {
                    NoteAt::Right(i) => {
                        let room = w + 2.0 * NOTE_GAP + BAR_W;
                        if i + 1 < n {
                            want(i, i + 1, room + 6.0);
                        } else {
                            right_room = right_room.max(room);
                        }
                    }
                    NoteAt::Left(i) => {
                        let room = w + 2.0 * NOTE_GAP + BAR_W;
                        if i > 0 {
                            want(i - 1, i, room + 6.0);
                        } else {
                            left_room = left_room.max(room);
                        }
                    }
                    NoteAt::Over(a, b) => {
                        if a == b {
                            if a > 0 {
                                want(a - 1, a, w / 2.0 + NOTE_GAP + 6.0);
                            } else {
                                left_room = left_room.max(w / 2.0);
                            }
                            if a + 1 < n {
                                want(a, a + 1, w / 2.0 + NOTE_GAP + 6.0);
                            } else {
                                right_room = right_room.max(w / 2.0);
                            }
                        } else {
                            want(a, b, w - 2.0 * NOTE_GAP);
                        }
                    }
                    NoteAt::Across => {}
                }
            }
            Step::Ref { first, last, text } => {
                let w = label_w(text) + 40.0;
                if first == last {
                    left_room = left_room.max(w / 2.0);
                    right_room = right_room.max(w / 2.0);
                } else {
                    want(*first, *last, w - 30.0);
                }
            }
            _ => {}
        }
    }
    let mut x = vec![0.0f32; n];
    for j in 1..n {
        let best = (0..j)
            .filter_map(|i| need.get(&(i, j)).map(|d| x[i] + d))
            .fold(0.0f32, f32::max);
        x[j] = best;
    }
    let left = -left_room - MARGIN;
    let right = x.last().copied().unwrap_or(0.0) + right_room + MARGIN;
    Columns { x, left, right }
}

/// An arrowhead at `tip`, the line coming from `from_x` (horizontal).
fn head(shapes: &mut Vec<Shape>, tip: (f32, f32), from_x: f32, kind: Head, color: Color) {
    let dir = if from_x <= tip.0 { 1.0 } else { -1.0 };
    let back = tip.0 - dir * HEAD_SIZE;
    let half = HEAD_SIZE * 0.45;
    match kind {
        Head::Filled => shapes.push(Shape::Path {
            points: vec![tip, (back, tip.1 - half), (back, tip.1 + half)],
            stroke: Some((color, 1.0)),
            fill: Some(color),
            closed: true,
            dashed: false,
        }),
        Head::Open => shapes.push(Shape::Path {
            points: vec![(back, tip.1 - half), tip, (back, tip.1 + half)],
            stroke: Some((color, STROKE)),
            fill: None,
            closed: false,
            dashed: false,
        }),
        Head::Half => shapes.push(Shape::Path {
            points: vec![(back, tip.1 - half), tip],
            stroke: Some((color, STROKE)),
            fill: None,
            closed: false,
            dashed: false,
        }),
        Head::Lost => {
            let c = (tip.0 - dir * 4.0, tip.1);
            let r = 4.0;
            for (dx, dy) in [(r, r), (r, -r)] {
                shapes.push(Shape::Path {
                    points: vec![(c.0 - dx, c.1 - dy), (c.0 + dx, c.1 + dy)],
                    stroke: Some((color, STROKE)),
                    fill: None,
                    closed: false,
                    dashed: false,
                });
            }
        }
        Head::Circle => shapes.push(Shape::Circle {
            center: (tip.0 - dir * 4.0, tip.1),
            radius: 4.0,
            fill: color,
        }),
    }
}

/// The colour a message's `-[#red]>` gives its line.
fn line_color(color: Option<&str>) -> Option<Color> {
    match parse_paint(color?)? {
        Paint::Color(c) => Some(c),
        Paint::Transparent => None,
    }
}

fn line(points: Vec<(f32, f32)>, color: Color, width: f32, dashed: bool) -> Shape {
    Shape::Path {
        points,
        stroke: Some((color, width)),
        fill: None,
        closed: false,
        dashed,
    }
}

/// Text lines starting at `(x, top)`, on a patch of the background so
/// lines behind them stop short.
fn text_block(
    shapes: &mut Vec<Shape>,
    x: f32,
    top: f32,
    text: &str,
    style: TextStyle,
    patch: Option<Color>,
) {
    if text.is_empty() {
        return;
    }
    if let Some(bg) = patch {
        shapes.push(Shape::Rect {
            rect: RectF::new(x - 3.0, top, label_w(text) + 6.0, label_h(text)),
            fill: Some(bg),
            stroke: None,
            radius: 3.0,
        });
    }
    for (i, l) in text.lines().enumerate() {
        shapes.push(Shape::Text {
            pos: (x, top + LABEL_LINE * (i as f32 + 0.5)),
            text: l.to_string(),
            style,
        });
    }
}

struct Frame {
    kind: String,
    label: String,
    color: Option<String>,
    top: f32,
    elses: Vec<(f32, String)>,
    /// Left and right of what is inside, so far.
    span: Option<(f32, f32)>,
}

impl Frame {
    fn cover(&mut self, a: f32, b: f32) {
        let (a, b) = (a.min(b), a.max(b));
        self.span = Some(match self.span {
            Some((l, r)) => (l.min(a), r.max(b)),
            None => (a, b),
        });
    }
}

/// The shapes of a sequence diagram (and its title).
pub fn scene(seq: &Sequence, title: Option<&str>, theme: &Theme) -> Scene {
    let mut s = Scene::default();
    let n = seq.participants.len();
    let heads_el: Vec<Element> = seq.participants.iter().map(head_element).collect();
    let heads: Vec<(f32, f32)> = heads_el
        .iter()
        .map(|e| {
            let (w, h) = size_px(e);
            (w.max(80.0), h)
        })
        .collect();
    let notes: HashMap<usize, (f32, f32)> = seq
        .steps
        .iter()
        .enumerate()
        .filter_map(|(k, st)| match st {
            Step::Note { text, color, .. } => Some((k, size_px(&note_element(text, color)))),
            _ => None,
        })
        .collect();
    let cols = columns(seq, &heads, &notes);
    let x = &cols.x;

    let title_line = title
        .and_then(|t| t.lines().next())
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let mut top = MARGIN;
    if title_line.is_some() {
        top += TITLE_ROOM;
    }
    // Boxes around participants: room for their title above the heads.
    if seq.boxes.iter().any(|b| b.title.is_some()) {
        top += LABEL_LINE + 6.0;
    }
    let head_h = seq
        .participants
        .iter()
        .zip(&heads)
        .filter(|(p, _)| !p.created)
        .map(|(_, h)| h.1)
        .fold(0.0, f32::max);
    let head_bottom = top + head_h;

    // Lifelines start under each head; created ones where they arrive.
    let mut life_top: Vec<f32> = vec![head_bottom; n];
    let mut life_end: Vec<Option<f32>> = vec![None; n];
    let mut created_at: Vec<Option<f32>> = vec![None; n];

    let mut under: Vec<Shape> = Vec::new(); // frames, behind everything
    let mut bars_shapes: Vec<Shape> = Vec::new();
    let mut over: Vec<Shape> = Vec::new(); // messages, notes, text
    let mut gaps: Vec<(f32, f32)> = Vec::new(); // delays: dotted lifelines
    let mut bars: Vec<Vec<(f32, Option<String>)>> = vec![Vec::new(); n];
    let mut frames: Vec<Frame> = Vec::new();
    let mut numbering = Numbering {
        on: false,
        next: 1,
        step: 1,
        format: None,
    };
    let label_style = TextStyle::new(LABEL_SIZE, theme.text);
    let mut y = head_bottom + ROW_GAP + 4.0;
    // Where the last message ran: activations start and end there.
    let mut last_arrow: Option<f32> = None;

    let bar_x = |bars: &Vec<Vec<(f32, Option<String>)>>, i: usize, from_left: bool| {
        let depth = bars[i].len();
        if depth == 0 {
            x[i]
        } else {
            let left = x[i] - BAR_W / 2.0 + (depth - 1) as f32 * BAR_W / 2.0;
            if from_left {
                left
            } else {
                left + BAR_W
            }
        }
    };

    for (k, step) in seq.steps.iter().enumerate() {
        let was_message = matches!(step, Step::Message(_));
        match step {
            Step::Message(m) => {
                let number = numbering.take();
                let color = line_color(m.color.as_deref()).unwrap_or(theme.line);
                let text_h = label_h(&m.text).max(if number.is_some() { LABEL_LINE } else { 0.0 });
                let mut arrow_y = y + text_h + 4.0;
                let ends = (m.from, m.to);
                let self_call = matches!(ends, (Some(MessageEnd::Participant(a)), Some(MessageEnd::Participant(b))) if a == b);
                // A participant created by this message: its box meets the arrow.
                let created = match m.to {
                    Some(MessageEnd::Participant(j))
                        if seq.participants[j].created && created_at[j].is_none() =>
                    {
                        arrow_y = arrow_y.max(y + heads[j].1 / 2.0);
                        Some(j)
                    }
                    _ => None,
                };
                let edge = |e: Option<MessageEnd>| match e {
                    Some(MessageEnd::Participant(i)) => x[i],
                    Some(MessageEnd::Left) | None => cols.left + 4.0,
                    Some(MessageEnd::Right) => cols.right - 4.0,
                };
                let (xa, xb) = (edge(m.from), edge(m.to));
                let mut text = m.text.clone();
                if let Some(num) = &number {
                    text = if text.is_empty() {
                        num.clone()
                    } else {
                        format!("{num} {text}")
                    };
                }
                if self_call {
                    let i = match m.from {
                        Some(MessageEnd::Participant(i)) => i,
                        _ => unreachable!(),
                    };
                    let x0 = bar_x(&bars, i, false);
                    let x1 = x[i] + SELF_W + BAR_W / 2.0;
                    let y1 = arrow_y + SELF_H;
                    over.push(line(
                        vec![(x0, arrow_y), (x1, arrow_y), (x1, y1), (x0 + 1.0, y1)],
                        color,
                        STROKE,
                        m.dashed,
                    ));
                    head(&mut over, (x0, y1), x1, m.head, color);
                    text_block(
                        &mut over,
                        x[i] + BAR_W,
                        y,
                        &text,
                        label_style,
                        Some(theme.background),
                    );
                    for f in frames.iter_mut() {
                        f.cover(x[i], x1.max(x[i] + BAR_W + label_w(&text)));
                    }
                    last_arrow = Some(arrow_y);
                    y = y1 + ROW_GAP;
                } else {
                    let from_left = xa < xb;
                    let mut tip_x = match m.to {
                        Some(MessageEnd::Participant(j)) => bar_x(&bars, j, from_left),
                        _ => xb,
                    };
                    let start_x = match m.from {
                        Some(MessageEnd::Participant(i)) => bar_x(&bars, i, !from_left),
                        _ => xa,
                    };
                    if let Some(j) = created {
                        // The arrow ends at the side of the new box.
                        let half = heads[j].0 / 2.0;
                        tip_x = if from_left { x[j] - half } else { x[j] + half };
                        let r = RectF::new(
                            x[j] - half,
                            arrow_y - heads[j].1 / 2.0,
                            heads[j].0,
                            heads[j].1,
                        );
                        let content = box_content(&heads_el[j]);
                        draw_box(&mut over, &content, r, theme, false);
                        created_at[j] = Some(r.y);
                        life_top[j] = r.bottom();
                    }
                    over.push(line(
                        vec![(start_x, arrow_y), (tip_x, arrow_y)],
                        color,
                        STROKE,
                        m.dashed,
                    ));
                    head(&mut over, (tip_x, arrow_y), start_x, m.head, color);
                    if m.both {
                        head(&mut over, (start_x, arrow_y), tip_x, m.head, color);
                    }
                    let tx = start_x.min(tip_x) + LABEL_PAD;
                    text_block(&mut over, tx, y, &text, label_style, Some(theme.background));
                    for f in frames.iter_mut() {
                        f.cover(start_x, tip_x);
                    }
                    last_arrow = Some(arrow_y);
                    y = arrow_y
                        + match created {
                            Some(j) => heads[j].1 / 2.0 + ROW_GAP,
                            None => ROW_GAP,
                        };
                }
            }
            Step::Autonumber(args) => numbering.apply(args),
            Step::Activate { who, color } => {
                let at = last_arrow
                    .filter(|_| prev_is_message(seq, k))
                    .unwrap_or(y - ROW_GAP / 2.0);
                bars[*who].push((at, color.clone()));
            }
            Step::Deactivate { who } => {
                let at = last_arrow
                    .filter(|_| prev_is_message(seq, k))
                    .unwrap_or(y - ROW_GAP / 2.0);
                if let Some((from, color)) = bars[*who].pop() {
                    let depth = bars[*who].len();
                    let bx = x[*who] - BAR_W / 2.0 + depth as f32 * BAR_W / 2.0;
                    bars_shapes.push(bar(theme, bx, from, at.max(from + 6.0), &color));
                }
            }
            Step::Destroy { who } => {
                let at = last_arrow.filter(|_| prev_is_message(seq, k)).unwrap_or(y);
                while let Some((from, color)) = bars[*who].pop() {
                    let depth = bars[*who].len();
                    let bx = x[*who] - BAR_W / 2.0 + depth as f32 * BAR_W / 2.0;
                    bars_shapes.push(bar(theme, bx, from, at.max(from + 6.0), &color));
                }
                life_end[*who] = Some(at);
                let c = (x[*who], at);
                for (dx, dy) in [(7.0, 7.0), (7.0, -7.0)] {
                    over.push(line(
                        vec![(c.0 - dx, c.1 - dy), (c.0 + dx, c.1 + dy)],
                        theme.line,
                        2.0,
                        false,
                    ));
                }
                if at >= y - ROW_GAP {
                    y = at + 10.0 + ROW_GAP;
                }
            }
            Step::Note {
                at,
                text,
                shape,
                color,
                ..
            } => {
                let (w, h) = notes[&k];
                let left = match *at {
                    NoteAt::Left(i) => x[i] - BAR_W / 2.0 - NOTE_GAP - w,
                    NoteAt::Right(i) => bar_x(&bars, i, false).max(x[i] + BAR_W / 2.0) + NOTE_GAP,
                    NoteAt::Over(a, b) => {
                        let (l, r) = (x[a] - 20.0, x[b] + 20.0);
                        let span = r - l;
                        if span >= w {
                            l
                        } else {
                            (l + r) / 2.0 - w / 2.0
                        }
                    }
                    NoteAt::Across => cols.left + MARGIN / 2.0,
                };
                let width = match *at {
                    NoteAt::Over(a, b) => w.max(x[b] - x[a] + 40.0),
                    NoteAt::Across => (cols.right - cols.left - MARGIN).max(w),
                    _ => w,
                };
                let r = RectF::new(left, y, width, h);
                let el = note_element(text, color);
                let content = box_content(&el);
                match shape {
                    NoteShape::Folded => draw_box(&mut over, &content, r, theme, false),
                    _ => {
                        // `hnote`, `rnote`: a plain or six-sided box.
                        let fill = theme
                            .user_fill(theme.note_fill, color.as_deref())
                            .unwrap_or(theme.background);
                        let cut = if *shape == NoteShape::Hexagon {
                            8.0
                        } else {
                            0.0
                        };
                        over.push(Shape::Path {
                            points: vec![
                                (r.x + cut, r.y),
                                (r.right() - cut, r.y),
                                (r.right(), r.y + r.h / 2.0),
                                (r.right() - cut, r.bottom()),
                                (r.x + cut, r.bottom()),
                                (r.x, r.y + r.h / 2.0),
                            ],
                            stroke: Some((theme.note_stroke, 1.0)),
                            fill: Some(fill),
                            closed: true,
                            dashed: false,
                        });
                        let st = TextStyle::new(LABEL_SIZE, theme.text);
                        text_block(&mut over, r.x + cut + 8.0, r.y + 6.0, text, st, None);
                    }
                }
                for f in frames.iter_mut() {
                    f.cover(r.x, r.right());
                }
                y += h + ROW_GAP;
            }
            Step::FragmentStart { kind, label, color } => {
                frames.push(Frame {
                    kind: kind.clone(),
                    label: label.clone(),
                    color: color.clone(),
                    top: y,
                    elses: Vec::new(),
                    span: None,
                });
                y += TAB_H.max(label_h(label) + 4.0) + 8.0;
            }
            Step::Else { label } => {
                if let Some(f) = frames.last_mut() {
                    f.elses.push((y, label.clone()));
                }
                y += label_h(label).max(LABEL_LINE) + 10.0;
            }
            Step::FragmentEnd => {
                if let Some(f) = frames.pop() {
                    y += 4.0;
                    let (l, r) = f.span.unwrap_or((
                        x.first().copied().unwrap_or(0.0),
                        x.last().copied().unwrap_or(0.0),
                    ));
                    let tab_w = text_width(&f.kind, LABEL_SIZE) + 18.0;
                    let label_room = tab_w + 10.0 + label_w(&format!("[{}]", f.label));
                    let else_room = f
                        .elses
                        .iter()
                        .map(|(_, l)| label_w(&format!("[{l}]")) + 20.0)
                        .fold(0.0, f32::max);
                    let left = l - FRAME_PAD;
                    let right = (r + FRAME_PAD).max(left + label_room.max(else_room) + 10.0);
                    let rect = RectF::new(left, f.top, right - left, y - f.top);
                    frame_shapes(&mut under, &mut over, theme, &f, rect, tab_w);
                    if let Some(parent) = frames.last_mut() {
                        parent.cover(rect.x - 2.0, rect.right() + 2.0);
                    }
                    y += ROW_GAP;
                }
            }
            Step::Divider(text) => {
                y += 6.0;
                let mid = y + 10.0;
                over.push(line(
                    vec![(cols.left + 6.0, mid), (cols.right - 6.0, mid)],
                    theme.muted,
                    1.0,
                    false,
                ));
                over.push(line(
                    vec![(cols.left + 6.0, mid + 3.0), (cols.right - 6.0, mid + 3.0)],
                    theme.muted,
                    1.0,
                    false,
                ));
                if !text.is_empty() {
                    let w = label_w(text) + 20.0;
                    let cx = (cols.left + cols.right) / 2.0;
                    let r = RectF::new(
                        cx - w / 2.0,
                        mid - label_h(text) / 2.0 - 3.0,
                        w,
                        label_h(text) + 6.0,
                    );
                    over.push(Shape::Rect {
                        rect: r,
                        fill: Some(theme.package_fill),
                        stroke: Some((theme.package_stroke, 1.0)),
                        radius: 4.0,
                    });
                    let mut st = TextStyle::new(LABEL_SIZE, theme.text);
                    st.bold = true;
                    text_block(&mut over, r.x + 10.0, r.y + 3.0, text, st, None);
                }
                y = mid + 10.0 + label_h(text) / 2.0 + ROW_GAP;
            }
            Step::Delay(text) => {
                let h = 24.0 + label_h(text);
                gaps.push((y, y + h));
                if !text.is_empty() {
                    let cx = (cols.left + cols.right) / 2.0;
                    let mut st = TextStyle::new(LABEL_SIZE, theme.muted);
                    st.italic = true;
                    text_block(
                        &mut over,
                        cx - label_w(text) / 2.0,
                        y + 12.0,
                        text,
                        st,
                        Some(theme.background),
                    );
                }
                y += h + ROW_GAP / 2.0;
            }
            Step::Space(px) => y += px.map_or(24.0, |p| p as f32),
            Step::Ref { first, last, text } => {
                let w = label_w(text) + 40.0;
                let (l, r) = (x[*first] - 24.0, x[*last] + 24.0);
                let (l, r) = if r - l >= w {
                    (l, r)
                } else {
                    ((l + r - w) / 2.0, (l + r + w) / 2.0)
                };
                let h = TAB_H + label_h(text) + 14.0;
                let rect = RectF::new(l, y, r - l, h);
                over.push(Shape::Rect {
                    rect,
                    fill: Some(theme.box_fill),
                    stroke: Some((theme.box_stroke, STROKE)),
                    radius: 2.0,
                });
                tab(
                    &mut over,
                    theme,
                    rect,
                    "ref",
                    text_width("ref", LABEL_SIZE) + 18.0,
                );
                let st = TextStyle::new(LABEL_SIZE, theme.text);
                text_block(
                    &mut over,
                    (l + r) / 2.0 - label_w(text) / 2.0,
                    y + TAB_H + 6.0,
                    text,
                    st,
                    None,
                );
                for f in frames.iter_mut() {
                    f.cover(l, r);
                }
                y += h + ROW_GAP;
            }
        }
        if !was_message
            && !matches!(
                step,
                Step::Activate { .. } | Step::Deactivate { .. } | Step::Autonumber(_)
            )
        {
            last_arrow = None;
        }
    }
    // Fragments left open close at the end.
    while let Some(f) = frames.pop() {
        let (l, r) = f.span.unwrap_or((0.0, x.last().copied().unwrap_or(0.0)));
        let tab_w = text_width(&f.kind, LABEL_SIZE) + 18.0;
        let rect = RectF::new(l - FRAME_PAD, f.top, r - l + 2.0 * FRAME_PAD, y - f.top);
        frame_shapes(&mut under, &mut over, theme, &f, rect, tab_w);
    }
    let bottom = y + 4.0;
    for (i, stack) in bars.iter_mut().enumerate() {
        while let Some((from, color)) = stack.pop() {
            let depth = stack.len();
            let bx = x[i] - BAR_W / 2.0 + depth as f32 * BAR_W / 2.0;
            bars_shapes.push(bar(theme, bx, from, bottom, &color));
        }
    }

    // Lifelines, with gaps dotted where time passes.
    let mut lifelines = Vec::new();
    for i in 0..n {
        let from = life_top[i];
        let to = life_end[i].unwrap_or(bottom);
        let mut cuts = vec![from];
        for &(a, b) in &gaps {
            if a > from && b < to {
                cuts.push(a);
                cuts.push(b);
            }
        }
        cuts.push(to);
        for (k, w) in cuts.windows(2).enumerate() {
            lifelines.push(Shape::Path {
                points: vec![(x[i], w[0]), (x[i], w[1])],
                stroke: Some((theme.muted, if k % 2 == 0 { 1.0 } else { 0.6 })),
                fill: None,
                closed: false,
                dashed: true,
            });
        }
    }

    // Boxes around participants.
    for b in &seq.boxes {
        if b.first >= n || b.last >= n || b.last < b.first {
            continue;
        }
        let l = x[b.first] - heads[b.first].0 / 2.0 - 10.0;
        let r = x[b.last] + heads[b.last].0 / 2.0 + 10.0;
        let t = top
            - if b.title.is_some() {
                LABEL_LINE + 6.0
            } else {
                6.0
            };
        let fill = theme
            .user_fill(theme.package_fill, b.color.as_deref())
            .unwrap_or(theme.background);
        s.shapes.push(Shape::Rect {
            rect: RectF::new(
                l,
                t,
                r - l,
                bottom + if seq.footbox { head_h + 6.0 } else { 0.0 } - t + 6.0,
            ),
            fill: Some(fill),
            stroke: Some((theme.package_stroke, 1.0)),
            radius: 6.0,
        });
        if let Some(title) = &b.title {
            let mut st = TextStyle::new(LABEL_SIZE, theme.text);
            st.bold = true;
            text_block(&mut s.shapes, l + 8.0, t + 3.0, title, st, None);
        }
    }

    s.shapes.extend(under);
    s.shapes.extend(lifelines);
    s.shapes.extend(bars_shapes);
    // Heads at the top and, unless hidden, at the bottom.
    for i in 0..n {
        let content = box_content(&heads_el[i]);
        let (w, h) = heads[i];
        if created_at[i].is_none() {
            let r = RectF::new(x[i] - w / 2.0, head_bottom - h, w, h);
            draw_box(&mut s.shapes, &content, r, theme, false);
        }
        if seq.footbox && life_end[i].is_none() {
            let r = RectF::new(x[i] - w / 2.0, bottom, w, h);
            draw_box(&mut s.shapes, &content, r, theme, false);
        }
    }
    s.shapes.extend(over);
    let mut bounds_bottom = bottom + if seq.footbox { head_h } else { 0.0 };
    bounds_bottom += 4.0;
    let mut bounds = RectF::new(
        cols.left,
        top - TITLE_ROOM.min(top),
        cols.right - cols.left,
        bounds_bottom - (top - TITLE_ROOM.min(top)),
    );
    for sh in &s.shapes {
        if let Shape::Rect { rect, .. } = sh {
            bounds = bounds.union(rect);
        }
    }
    if let Some(t) = title_line {
        let mut st = TextStyle::new(TITLE_SIZE, theme.text);
        st.bold = true;
        s.shapes.push(Shape::Text {
            pos: (cols.left, MARGIN + TITLE_ROOM / 2.0 - 6.0),
            text: t.to_string(),
            style: st,
        });
        bounds = bounds.union(&RectF::new(
            cols.left,
            MARGIN,
            text_width(t, TITLE_SIZE),
            TITLE_ROOM,
        ));
    }
    s.bounds = bounds.expand(16.0);
    s
}

fn prev_is_message(seq: &Sequence, k: usize) -> bool {
    seq.steps[..k]
        .iter()
        .rev()
        .find(|s| {
            !matches!(
                s,
                Step::Activate { .. } | Step::Deactivate { .. } | Step::Autonumber(_)
            )
        })
        .is_some_and(|s| matches!(s, Step::Message(_)))
}

fn bar(theme: &Theme, x: f32, from: f32, to: f32, color: &Option<String>) -> Shape {
    let fill = theme.user_fill(theme.box_fill, color.as_deref());
    Shape::Rect {
        rect: RectF::new(x, from, BAR_W, to - from),
        fill,
        stroke: Some((theme.box_stroke, 1.0)),
        radius: 1.0,
    }
}

/// The name tab in a frame's top-left corner.
fn tab(shapes: &mut Vec<Shape>, theme: &Theme, r: RectF, name: &str, w: f32) {
    shapes.push(Shape::Path {
        points: vec![
            (r.x, r.y + TAB_H),
            (r.x + w - 6.0, r.y + TAB_H),
            (r.x + w, r.y + TAB_H - 6.0),
            (r.x + w, r.y),
        ],
        stroke: Some((theme.package_stroke, 1.0)),
        fill: None,
        closed: false,
        dashed: false,
    });
    let mut st = TextStyle::new(LABEL_SIZE, theme.text);
    st.bold = true;
    shapes.push(Shape::Text {
        pos: (r.x + 8.0, r.y + TAB_H / 2.0),
        text: name.to_string(),
        style: st,
    });
}

fn frame_shapes(
    under: &mut Vec<Shape>,
    over: &mut Vec<Shape>,
    theme: &Theme,
    f: &Frame,
    rect: RectF,
    tab_w: f32,
) {
    let fill = f
        .color
        .as_deref()
        .and_then(|c| theme.user_fill(theme.package_fill, Some(c)));
    under.push(Shape::Rect {
        rect,
        fill,
        stroke: Some((theme.package_stroke, 1.2)),
        radius: 3.0,
    });
    tab(over, theme, rect, &f.kind, tab_w);
    if !f.label.is_empty() {
        let st = TextStyle::new(LABEL_SIZE, theme.text);
        text_block(
            over,
            rect.x + tab_w + 8.0,
            rect.y + 3.0,
            &format!("[{}]", f.label),
            st,
            None,
        );
    }
    for (ey, label) in &f.elses {
        over.push(line(
            vec![(rect.x, *ey), (rect.right(), *ey)],
            theme.package_stroke,
            1.0,
            true,
        ));
        if !label.is_empty() {
            let st = TextStyle::new(LABEL_SIZE, theme.text);
            text_block(
                over,
                rect.x + 8.0,
                ey + 3.0,
                &format!("[{label}]"),
                st,
                Some(theme.background),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_rects(s: &Scene) -> Vec<(RectF, String)> {
        s.shapes
            .iter()
            .filter_map(|sh| match sh {
                Shape::Text { pos, text, style } => {
                    let w = text_width(text, style.size);
                    let h = style.size;
                    let x = if style.centered {
                        pos.0 - w / 2.0
                    } else {
                        pos.0
                    };
                    Some((RectF::new(x, pos.1 - h / 2.0, w, h), text.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// No text is drawn over another, and each label of a message between
    /// two participants stays between their lifelines.
    #[test]
    fn texts_do_not_overlap_and_labels_fit_between_lifelines() {
        let src = "@startuml\ntitle T\nactor User\nparticipant \"A rather long name\\nover two lines\" as web\ndatabase DB\nqueue Q\nautonumber\nUser -> web ++ : a long label that needs room between the two\nweb -> DB : SELECT * FROM a_table WHERE x = 1\nweb -> web : a self call with a long label\nDB --> web\nalt the first case\n  web -> Q : put\nelse the second, longer case label\n  web -> User : no\nend\nnote right of DB : a note on the right that is wide\nnote left of User : left\nnote over web, Q : over\nreturn done\n== A divider ==\n... later ...\nref over User, DB : something else\n@enduml\n";
        let p = umpl_text::plantuml::import(src);
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        let seq = p.doc.sequence.unwrap();
        let s = scene(&seq, p.doc.title.as_deref(), &Theme::light());
        let rects = text_rects(&s);
        for (i, (a, ta)) in rects.iter().enumerate() {
            for (b, tb) in &rects[i + 1..] {
                let shrink = |r: &RectF| RectF::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0);
                assert!(
                    !shrink(a).intersects(&shrink(b)),
                    "`{ta}` {a:?} overlaps `{tb}` {b:?}"
                );
            }
        }
        // Lifelines go left to right in the order participants are named.
        let heads: Vec<f32> = seq
            .participants
            .iter()
            .map(|p| {
                let first = p.shown.lines().next().unwrap();
                rects.iter().find(|(_, t)| t == first).unwrap().0.center().0
            })
            .collect();
        assert!(heads.windows(2).all(|w| w[0] < w[1]), "{heads:?}");
        // The label from User to web ends before web's lifeline.
        let (label, _) = rects
            .iter()
            .find(|(_, t)| t.contains("a long label that needs room"))
            .unwrap();
        assert!(
            label.x > heads[0] && label.right() < heads[1],
            "{label:?} {heads:?}"
        );
    }
}
