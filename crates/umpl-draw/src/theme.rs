use umpl_core::ElementKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color(r, g, b, 255)
    }

    pub const fn hex(v: u32) -> Color {
        Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    pub fn css(&self) -> String {
        if self.3 == 255 {
            format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
        } else {
            format!(
                "rgba({},{},{},{:.3})",
                self.0,
                self.1,
                self.2,
                self.3 as f32 / 255.0
            )
        }
    }

    pub fn with_alpha(self, a: u8) -> Color {
        Color(self.0, self.1, self.2, a)
    }

    /// This colour with `amount` (0..1) of `over` painted on top: the
    /// opaque colour a half-transparent `over` would show.
    pub fn blend(self, over: Color, amount: f32) -> Color {
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * amount).round() as u8;
        Color(
            mix(self.0, over.0),
            mix(self.1, over.1),
            mix(self.2, over.2),
            self.3,
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    pub background: Color,
    pub grid: Color,
    pub box_fill: Color,
    pub box_stroke: Color,
    pub text: Color,
    pub muted: Color,
    pub line: Color,
    pub accent: Color,
    pub package_fill: Color,
    pub package_stroke: Color,
    pub note_fill: Color,
    pub note_stroke: Color,
    pub mark_class: Color,
    pub mark_abstract: Color,
    pub mark_interface: Color,
    pub mark_enum: Color,
    pub mark_text: Color,
    /// How strongly a box is tinted with the colour of its kind.
    pub tint: f32,
}

impl Theme {
    pub fn light() -> Theme {
        Theme {
            dark: false,
            background: Color::hex(0xfbfbfa),
            grid: Color::hex(0xeceef1),
            box_fill: Color::hex(0xffffff),
            box_stroke: Color::hex(0x3b4452),
            text: Color::hex(0x1d2430),
            muted: Color::hex(0x5c6675),
            line: Color::hex(0x4b5563),
            accent: Color::hex(0x2563c9),
            package_fill: Color::hex(0xf2f4f7),
            package_stroke: Color::hex(0xa9b3c1),
            note_fill: Color::hex(0xfff8d9),
            note_stroke: Color::hex(0xc4ae57),
            mark_class: Color::hex(0x4f8ad9),
            mark_abstract: Color::hex(0x8a6fd1),
            mark_interface: Color::hex(0x2c9c74),
            mark_enum: Color::hex(0xd0832a),
            mark_text: Color::hex(0xffffff),
            tint: 0.12,
        }
    }

    pub fn dark() -> Theme {
        Theme {
            dark: true,
            background: Color::hex(0x12151b),
            grid: Color::hex(0x1b1f27),
            box_fill: Color::hex(0x1d222b),
            box_stroke: Color::hex(0x8d99ab),
            text: Color::hex(0xe6e9ef),
            muted: Color::hex(0x9aa3b2),
            line: Color::hex(0x9ba7b9),
            accent: Color::hex(0x6ea2ff),
            package_fill: Color::hex(0x171b22),
            package_stroke: Color::hex(0x3d4655),
            note_fill: Color::hex(0x332f1d),
            note_stroke: Color::hex(0x9c8a45),
            mark_class: Color::hex(0x5b95e5),
            mark_abstract: Color::hex(0x9b82e0),
            mark_interface: Color::hex(0x3cb38a),
            mark_enum: Color::hex(0xe0963f),
            mark_text: Color::hex(0x0f1218),
            tint: 0.18,
        }
    }

    /// Fill of a box of `kind`: the box colour lightly tinted with the
    /// colour of its kind. Opaque, so the canvas grid does not show
    /// through behind the text.
    pub fn fill(&self, kind: ElementKind) -> Color {
        self.box_fill.blend(self.mark(kind), self.tint)
    }

    pub fn mark(&self, kind: ElementKind) -> Color {
        match kind {
            ElementKind::Class | ElementKind::Note | ElementKind::Diamond => self.mark_class,
            ElementKind::Abstract => self.mark_abstract,
            ElementKind::Interface => self.mark_interface,
            ElementKind::Enum => self.mark_enum,
        }
    }
}

/// A fill the user chose for a box or package: a colour, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    Color(Color),
    Transparent,
}

/// The CSS colour names, as PlantUML also takes them.
const NAMED: [(&str, u32); 148] = [
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

/// `#rrggbb`, `#rgb` (the `#` is optional), a CSS colour name in any
/// case, or `transparent`. `None` for anything else.
pub fn parse_paint(s: &str) -> Option<Paint> {
    let s = s.trim().trim_start_matches('#').to_ascii_lowercase();
    if s == "transparent" {
        return Some(Paint::Transparent);
    }
    if let Some(&(_, v)) = NAMED.iter().find(|(n, _)| *n == s) {
        return Some(Paint::Color(Color::hex(v)));
    }
    let hex = |t: &str| u32::from_str_radix(t, 16).ok();
    match s.len() {
        6 if s.chars().all(|c| c.is_ascii_hexdigit()) => {
            hex(&s).map(|v| Paint::Color(Color::hex(v)))
        }
        3 if s.chars().all(|c| c.is_ascii_hexdigit()) => {
            let v = hex(&s)?;
            let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
            Some(Paint::Color(Color::rgb(
                (r * 17) as u8,
                (g * 17) as u8,
                (b * 17) as u8,
            )))
        }
        _ => None,
    }
}

impl Color {
    /// Perceived brightness, 0 (black) to 1 (white).
    pub fn luminance(self) -> f32 {
        (0.2126 * self.0 as f32 + 0.7152 * self.1 as f32 + 0.0722 * self.2 as f32) / 255.0
    }
}

impl Theme {
    /// The fill of a box or package coloured `color` (`None` for
    /// transparent): the colour itself in the light theme, half of it over
    /// `usual` in the dark one; `usual` when there is no colour or it is
    /// not one fastumpl knows.
    pub fn user_fill(&self, usual: Color, color: Option<&str>) -> Option<Color> {
        match color.and_then(parse_paint) {
            None => Some(usual),
            Some(Paint::Transparent) => None,
            Some(Paint::Color(c)) if self.dark => Some(usual.blend(c, 0.45)),
            Some(Paint::Color(c)) => Some(c),
        }
    }

    /// Text (or, `muted`, secondary text) readable on a box filled `fill`
    /// for colour `color`: the theme's own unless the user coloured it.
    pub fn text_on(&self, fill: Option<Color>, color: Option<&str>, muted: bool) -> Color {
        let usual = if muted { self.muted } else { self.text };
        let Some(fill) =
            fill.filter(|_| matches!(color.and_then(parse_paint), Some(Paint::Color(_))))
        else {
            return usual;
        };
        match (fill.luminance() > 0.55, muted) {
            (true, false) => Color::hex(0x1f2328),
            (true, true) => Color::hex(0x57606a),
            (false, false) => Color::hex(0xf0f3f6),
            (false, true) => Color::hex(0xb7bdc8),
        }
    }
}

/// Colours of groups that have none of their own, in turn: blue, red,
/// purple, green, amber, teal, pink, slate.
const GROUP_HUES: [u32; 8] = [
    0x4a86d8, 0xd2615f, 0x9a6fd8, 0x4ea56d, 0xd39b3a, 0x3aa6a8, 0xc864a0, 0x7f8a9c,
];

/// How a group (package) frame is painted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupPaint {
    pub border: Color,
    /// `None`: transparent, only the frame.
    pub fill: Option<Color>,
    pub title: Color,
}

impl Theme {
    /// The paint of the `index`-th group, coloured `color` by the user or
    /// else in turn from a palette, so groups tell apart: a border in the
    /// colour, a light tint of it inside, the title in it.
    pub fn group_paint(&self, index: usize, color: Option<&str>) -> GroupPaint {
        let paint = color.and_then(parse_paint);
        let auto = Color::hex(GROUP_HUES[index % GROUP_HUES.len()]);
        let hue = match paint {
            Some(Paint::Color(c)) => c,
            _ => auto,
        };
        let white = Color::rgb(255, 255, 255);
        let black = Color::rgb(0, 0, 0);
        let fill = match paint {
            Some(Paint::Transparent) => None,
            // A colour the user chose fills the group as chosen.
            Some(Paint::Color(_)) => self.user_fill(self.package_fill, color),
            None => Some(
                self.background
                    .blend(hue, if self.dark { 0.13 } else { 0.08 }),
            ),
        };
        let border = if self.dark {
            self.background.blend(hue, 0.8)
        } else {
            hue.blend(black, 0.15)
        };
        let title = match (paint, fill) {
            (Some(Paint::Color(_)), f) => self.text_on(f, color, false),
            _ if self.dark => hue.blend(white, 0.35),
            _ => hue.blend(black, 0.35),
        };
        GroupPaint {
            border,
            fill,
            title,
        }
    }
}
