//! What goes inside each box, and how big the box must be.

use umpl_core::{Element, ElementKind};

use crate::fonts::ADVANCE;
use crate::GRID;

pub const NAME_SIZE: f32 = 14.0;
pub const MEMBER_SIZE: f32 = 12.0;
pub const STEREO_SIZE: f32 = 11.0;
pub const LABEL_SIZE: f32 = 11.0;
pub const NAME_LINE: f32 = 20.0;
pub const MEMBER_LINE: f32 = 16.0;
pub const STEREO_LINE: f32 = 14.0;
pub const PAD_X: f32 = 10.0;
pub const PAD_Y: f32 = 6.0;
/// Room for the kind badge on each side of a centred name.
pub const BADGE_ROOM: f32 = 22.0;
pub const NOTE_PAD: f32 = 10.0;
pub const MIN_W_UNITS: i32 = 10;
/// Width and height of an association diamond, in grid units.
pub const DIAMOND_UNITS: i32 = 3;

pub fn text_width(text: &str, size: f32) -> f32 {
    text.chars().map(cells).sum::<u32>() as f32 * ADVANCE * size
}

/// How many monospace cells a character takes: two for the wide letters
/// of Chinese, Japanese and Korean and for emoji (drawn from fallback
/// fonts at about twice the width), none for combining marks and
/// zero-width characters, four for a tab, else one.
pub fn cells(c: char) -> u32 {
    match c as u32 {
        0x09 => 4,
        0x0300..=0x036F | 0x200B..=0x200F | 0xFE00..=0xFE0F | 0x20D0..=0x20FF => 0,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x2FFFD
        | 0x30000..=0x3FFFD => 2,
        _ => 1,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemberLine {
    pub text: String,
    pub italic: bool,
    pub underline: bool,
}

/// Everything drawn inside one box.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxContent {
    pub kind: ElementKind,
    /// Shown as `«…»` above the name.
    pub stereotype: Option<String>,
    pub name: String,
    pub italic_name: bool,
    /// Non-empty compartments under the name (attributes, operations; or
    /// literals for an enum). Notes keep their text here, one compartment.
    pub compartments: Vec<Vec<MemberLine>>,
    /// The colour the user gave the box (see [`umpl_core::Element::color`]).
    pub color: Option<String>,
}

impl BoxContent {
    pub fn header_height(&self) -> f32 {
        let stereo = if self.stereotype.is_some() {
            STEREO_LINE
        } else {
            0.0
        };
        PAD_Y * 2.0 + stereo + NAME_LINE
    }

    /// Size in grid units.
    pub fn size(&self) -> (i32, i32) {
        let units = |px: f32| (px / GRID).ceil() as i32;
        if self.kind == ElementKind::Diamond {
            return (DIAMOND_UNITS, DIAMOND_UNITS);
        }
        if self.kind == ElementKind::Note {
            let lines = self.compartments.first().map_or(&[][..], |c| &c[..]);
            let w = lines
                .iter()
                .map(|l| text_width(&l.text, MEMBER_SIZE))
                .fold(0.0, f32::max);
            let h = lines.len().max(1) as f32 * MEMBER_LINE;
            return (
                units(w + 2.0 * NOTE_PAD + 8.0).max(6),
                units(h + 2.0 * NOTE_PAD).max(3),
            );
        }
        let mut w = text_width(&self.name, NAME_SIZE) + 2.0 * BADGE_ROOM + 2.0 * PAD_X;
        if let Some(s) = &self.stereotype {
            w = w.max(text_width(&format!("«{s}»"), STEREO_SIZE) + 2.0 * PAD_X);
        }
        let mut h = self.header_height();
        for c in &self.compartments {
            for l in c {
                w = w.max(text_width(&l.text, MEMBER_SIZE) + 2.0 * PAD_X);
            }
            h += c.len() as f32 * MEMBER_LINE + 2.0 * 4.0;
        }
        (units(w).max(MIN_W_UNITS), units(h).max(4))
    }
}

pub fn box_content(el: &Element) -> BoxContent {
    let line = |m: &umpl_core::Member| MemberLine {
        text: {
            let mut t = String::new();
            if let Some(v) = m.vis {
                t.push(v.symbol());
                t.push(' ');
            }
            t.push_str(&m.text);
            t.replace('\t', "    ")
        },
        italic: m.is_abstract,
        underline: m.is_static,
    };
    let compartments = match el.kind {
        ElementKind::Note => vec![el
            .members
            .iter()
            .map(|m| MemberLine {
                text: m.text.replace('\t', "    "),
                italic: false,
                underline: false,
            })
            .collect()],
        ElementKind::Enum => vec![el.members.iter().map(line).collect()],
        _ => vec![
            el.attributes().map(line).collect(),
            el.operations().map(line).collect(),
        ],
    };
    BoxContent {
        kind: el.kind,
        stereotype: el.stereotype.clone(),
        name: el.title(),
        italic_name: el.kind == ElementKind::Abstract,
        compartments: compartments
            .into_iter()
            .filter(|c: &Vec<MemberLine>| !c.is_empty())
            .collect(),
        color: el.color.clone(),
    }
}

/// Size in grid units of a label showing `text`.
pub fn label_size(text: &str) -> (i32, i32) {
    (
        ((text_width(text, LABEL_SIZE) + 4.0) / GRID).ceil() as i32,
        2,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use umpl_core::Member;

    #[test]
    fn wide_letters_and_tabs_take_more_room() {
        assert_eq!(text_width("ab", 10.0), 2.0 * ADVANCE * 10.0);
        assert_eq!(text_width("日本", 10.0), 4.0 * ADVANCE * 10.0);
        assert_eq!(text_width("a\tb", 10.0), 6.0 * ADVANCE * 10.0);
        assert_eq!(text_width("e\u{301}", 10.0), ADVANCE * 10.0);
    }

    #[test]
    fn box_size_grows_with_members() {
        let mut el = Element::new(ElementKind::Class, "Order");
        let empty = box_content(&el).size();
        el.members.push(Member::parse_line("+id: OrderId"));
        let one = box_content(&el).size();
        el.members.push(Member::parse_line(
            "+aVeryLongOperationName(argument: SomeType): Result",
        ));
        let two = box_content(&el).size();
        assert!(one.1 > empty.1);
        assert!(two.1 > one.1);
        assert!(two.0 > one.0);
        assert!(empty.0 >= MIN_W_UNITS);
    }

    #[test]
    fn abstract_members_are_italic_and_static_underlined() {
        let mut el = Element::new(ElementKind::Abstract, "P");
        el.members.push(Member::parse_line("+pay() {abstract}"));
        el.members.push(Member::parse_line("+count: int {static}"));
        let c = box_content(&el);
        assert!(c.italic_name);
        assert!(c.compartments[0][0].underline);
        assert!(c.compartments[1][0].italic);
    }
}
