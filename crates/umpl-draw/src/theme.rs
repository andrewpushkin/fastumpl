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
