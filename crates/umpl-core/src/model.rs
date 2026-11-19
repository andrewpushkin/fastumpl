use std::collections::BTreeMap;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u32);
    };
}

id_type!(ElementId);
id_type!(RelationId);
id_type!(PackageId);

/// Top-left corner of an element, in grid units (1 unit = 8 px).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
}

impl Pos {
    pub fn new(x: i32, y: i32) -> Self {
        Pos { x, y }
    }
}

/// Positions in a file lie within this many grid units of the origin;
/// further ones are ignored as typos (layout cost grows with the drawing's
/// extent).
pub const MAX_COORD: i32 = 1_000_000;
/// The largest width or height, in grid units, a box or frame may be given.
pub const MAX_SIZE: i32 = 10_000;

/// Splits enum literals written on one line at `,` (and at any other of
/// `seps`), except inside brackets or quotes: `RED(255, 0, 0), GREEN` is
/// two literals. Parts are trimmed; empty ones are dropped.
pub fn split_literals<'a>(text: &'a str, seps: &[char]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut quoted = false;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '(' | '[' | '{' | '<' if !quoted => depth += 1,
            ')' | ']' | '}' | '>' if !quoted => depth = depth.saturating_sub(1),
            c if depth == 0 && !quoted && seps.contains(&c) => {
                out.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out.into_iter()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Is `s` a plain name: a letter or `_`, then letters, digits and `_`? Plain
/// names are written as they are in a file; others are quoted.
pub fn is_plain_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// `name` as one part of a dotted path in a file: as it is if plain,
/// otherwise in double quotes with `"`, `\` and line breaks escaped.
pub fn name_in_path(name: &str) -> std::borrow::Cow<'_, str> {
    if is_plain_name(name) {
        return name.into();
    }
    let mut out = String::with_capacity(name.len() + 2);
    out.push('"');
    for c in name.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out.into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElementKind {
    Class,
    Abstract,
    Interface,
    Enum,
    Note,
    /// The diamond of an association between more than two classes, or
    /// where an association class meets its association. No name is shown.
    Diamond,
}

impl ElementKind {
    pub const ALL: [ElementKind; 6] = [
        ElementKind::Class,
        ElementKind::Abstract,
        ElementKind::Interface,
        ElementKind::Enum,
        ElementKind::Note,
        ElementKind::Diamond,
    ];

    /// The keyword(s) that introduce this element in a `.umpl` file.
    pub fn keyword(self) -> &'static str {
        match self {
            ElementKind::Class => "class",
            ElementKind::Abstract => "abstract class",
            ElementKind::Interface => "interface",
            ElementKind::Enum => "enum",
            ElementKind::Note => "note",
            ElementKind::Diamond => "diamond",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ElementKind::Class => "Class",
            ElementKind::Abstract => "Abstract class",
            ElementKind::Interface => "Interface",
            ElementKind::Enum => "Enum",
            ElementKind::Note => "Note",
            ElementKind::Diamond => "Association diamond",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Visibility {
    Public,
    Private,
    Protected,
    Package,
}

impl Visibility {
    pub fn symbol(self) -> char {
        match self {
            Visibility::Public => '+',
            Visibility::Private => '-',
            Visibility::Protected => '#',
            Visibility::Package => '~',
        }
    }

    pub fn from_char(c: char) -> Option<Visibility> {
        match c {
            '+' => Some(Visibility::Public),
            '-' => Some(Visibility::Private),
            '#' => Some(Visibility::Protected),
            '~' => Some(Visibility::Package),
            _ => None,
        }
    }
}

/// One attribute, operation, enum literal or note line.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Member {
    pub vis: Option<Visibility>,
    /// Everything after the visibility and markers, e.g. `total(): Money`.
    pub text: String,
    pub is_static: bool,
    pub is_abstract: bool,
}

impl Member {
    /// A member with no visibility or markers (enum literal, note line).
    pub fn plain(text: impl Into<String>) -> Member {
        Member {
            vis: None,
            text: text.into(),
            is_static: false,
            is_abstract: false,
        }
    }

    pub fn is_operation(&self) -> bool {
        self.text.contains('(')
    }

    /// Parses a member line as written in a class body, e.g.
    /// `+pay(): Receipt {abstract}` or `-{static} count: int`.
    pub fn parse_line(line: &str) -> Member {
        let mut s = line.trim();
        let mut vis = None;
        if let Some(c) = s.chars().next() {
            if let Some(v) = Visibility::from_char(c) {
                vis = Some(v);
                s = s[c.len_utf8()..].trim_start();
            }
        }
        let mut is_static = false;
        let mut is_abstract = false;
        loop {
            if let Some(r) = s.strip_prefix("{static}") {
                is_static = true;
                s = r.trim_start();
            } else if let Some(r) = s.strip_prefix("{abstract}") {
                is_abstract = true;
                s = r.trim_start();
            } else if let Some(r) = s.strip_suffix("{static}") {
                is_static = true;
                s = r.trim_end();
            } else if let Some(r) = s.strip_suffix("{abstract}") {
                is_abstract = true;
                s = r.trim_end();
            } else {
                break;
            }
        }
        Member {
            vis,
            text: s.to_string(),
            is_static,
            is_abstract,
        }
    }

    /// The canonical line form: visibility, text, then markers.
    pub fn to_line(&self) -> String {
        let mut out = String::new();
        if let Some(v) = self.vis {
            out.push(v.symbol());
        }
        out.push_str(&self.text);
        if self.is_static {
            out.push_str(" {static}");
        }
        if self.is_abstract {
            out.push_str(" {abstract}");
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// What the file and lines refer to it by; unique in its package.
    pub name: String,
    /// What the box shows instead of the name, if it differs: PlantUML's
    /// `class "Purchase order" as PO`. Several elements may show the same.
    pub label: Option<String>,
    pub kind: ElementKind,
    /// Type parameters of a generic class, as written between `<` and `>`:
    /// `T`, `K, V`, `T extends Entity`.
    pub type_params: Option<String>,
    pub stereotype: Option<String>,
    /// Attributes and operations (classes), literals (enums) or text lines
    /// (notes).
    pub members: Vec<Member>,
    pub pos: Option<Pos>,
    /// Width and height in grid units the box was resized to: it is at
    /// least that big, and bigger if its text needs it. Layout, like `pos`.
    pub size: Option<(i32, i32)>,
    /// Fill colour: `#rrggbb`, a web colour name (`lightblue`) or
    /// `transparent`; the kind's colour if none.
    pub color: Option<String>,
    pub comments: Vec<String>,
}

impl Element {
    pub fn new(kind: ElementKind, name: impl Into<String>) -> Element {
        Element {
            name: name.into(),
            label: None,
            kind,
            type_params: None,
            stereotype: None,
            members: Vec::new(),
            pos: None,
            size: None,
            color: None,
            comments: Vec::new(),
        }
    }

    /// What the box shows: the label (else the name), with any type
    /// parameters: `Repository<T>`.
    pub fn title(&self) -> String {
        let shown = self.shown();
        match &self.type_params {
            Some(p) => format!("{shown}<{p}>"),
            None => shown.to_string(),
        }
    }

    /// The label, or else the name.
    pub fn shown(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.name)
    }

    pub fn attributes(&self) -> impl Iterator<Item = &Member> {
        self.members.iter().filter(|m| !m.is_operation())
    }

    pub fn operations(&self) -> impl Iterator<Item = &Member> {
        self.members.iter().filter(|m| m.is_operation())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RelationKind {
    /// `from` extends `to`.
    Generalization,
    /// `from` implements `to`.
    Realization,
    Association,
    /// Association navigable towards `to`.
    Directed,
    /// `from` is the whole, `to` the part.
    Aggregation,
    /// `from` is the whole, `to` the part.
    Composition,
    /// `from` depends on `to`.
    Dependency,
    /// Dashed line between a note and what it describes, or between an
    /// association class and its association's diamond.
    NoteLink,
    /// `to` is declared inside `from` (an inner class).
    Nesting,
}

impl RelationKind {
    pub const ALL: [RelationKind; 9] = [
        RelationKind::Generalization,
        RelationKind::Realization,
        RelationKind::Association,
        RelationKind::Directed,
        RelationKind::Aggregation,
        RelationKind::Composition,
        RelationKind::Dependency,
        RelationKind::NoteLink,
        RelationKind::Nesting,
    ];

    /// The forward arrow written in a `.umpl` file.
    pub fn arrow(self) -> &'static str {
        match self {
            RelationKind::Generalization => "--|>",
            RelationKind::Realization => "..|>",
            RelationKind::Association => "--",
            RelationKind::Directed => "-->",
            RelationKind::Aggregation => "o--",
            RelationKind::Composition => "*--",
            RelationKind::Dependency => "..>",
            RelationKind::NoteLink => "..",
            RelationKind::Nesting => "+--",
        }
    }

    pub fn is_dashed(self) -> bool {
        matches!(
            self,
            RelationKind::Realization | RelationKind::Dependency | RelationKind::NoteLink
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            RelationKind::Generalization => "Generalization",
            RelationKind::Realization => "Realization",
            RelationKind::Association => "Association",
            RelationKind::Directed => "Directed association",
            RelationKind::Aggregation => "Aggregation",
            RelationKind::Composition => "Composition",
            RelationKind::Dependency => "Dependency",
            RelationKind::NoteLink => "Note link",
            RelationKind::Nesting => "Nesting",
        }
    }
}

/// What one end of a relation is attached to: an element, or a package
/// (package diagrams draw lines between packages).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Endpoint {
    Element(ElementId),
    Package(PackageId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct End {
    pub target: Endpoint,
    /// `"mult role"`: multiplicity, role name, or both.
    pub label: Option<String>,
}

impl End {
    pub fn new(element: ElementId) -> End {
        End::at(Endpoint::Element(element))
    }

    pub fn at(target: Endpoint) -> End {
        End {
            target,
            label: None,
        }
    }

    /// The element this end is attached to, if it is not a package.
    pub fn element(&self) -> Option<ElementId> {
        match self.target {
            Endpoint::Element(e) => Some(e),
            Endpoint::Package(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Relation {
    pub kind: RelationKind,
    pub from: End,
    pub to: End,
    pub name: Option<String>,
    pub comments: Vec<String>,
}

impl Relation {
    pub fn new(kind: RelationKind, from: ElementId, to: ElementId) -> Relation {
        Relation {
            kind,
            from: End::new(from),
            to: End::new(to),
            name: None,
            comments: Vec::new(),
        }
    }

    pub fn touches(&self, id: ElementId) -> bool {
        self.touches_endpoint(Endpoint::Element(id))
    }

    pub fn touches_endpoint(&self, target: Endpoint) -> bool {
        self.from.target == target || self.to.target == target
    }
}

/// One entry in a declaration list (the file root or a package body), in
/// file order.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Decl {
    Element(ElementId),
    Relation(RelationId),
    Package(PackageId),
    /// A line the parser did not understand, written back unchanged.
    Raw(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Package {
    pub name: String,
    pub body: Vec<Decl>,
    /// Fill colour, as for [`Element::color`]: `transparent` draws only
    /// the frame.
    pub color: Option<String>,
    pub comments: Vec<String>,
    /// The area the user gave the package's frame (top-left corner and
    /// size, in grid units): the frame covers at least this and always its
    /// content. Like positions, it is layout.
    pub frame: Option<(Pos, (i32, i32))>,
}

impl Package {
    pub fn new(name: impl Into<String>) -> Package {
        Package {
            name: name.into(),
            body: Vec::new(),
            color: None,
            comments: Vec::new(),
            frame: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub title: Option<String>,
    /// Laid out left to right (PlantUML's `left to right direction`),
    /// not top to bottom.
    pub left_to_right: bool,
    /// A sequence diagram instead of boxes and lines (it then has no
    /// elements, relations or packages).
    pub sequence: Option<crate::Sequence>,
    pub elements: BTreeMap<ElementId, Element>,
    pub relations: BTreeMap<RelationId, Relation>,
    pub packages: BTreeMap<PackageId, Package>,
    /// Top-level declarations in file order.
    pub root: Vec<Decl>,
    /// Comments after the last declaration.
    pub trailing_comments: Vec<String>,
    pub(crate) next_id: u32,
}

impl Document {
    pub fn new() -> Document {
        Document::default()
    }

    fn next(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub(crate) fn reserve(&mut self, id: u32) {
        self.next_id = self.next_id.max(id + 1);
    }

    pub fn new_element_id(&mut self) -> ElementId {
        ElementId(self.next())
    }

    pub fn new_relation_id(&mut self) -> RelationId {
        RelationId(self.next())
    }

    pub fn new_package_id(&mut self) -> PackageId {
        PackageId(self.next())
    }

    /// The package whose body directly contains `decl`, or `None` for root.
    pub fn container_of(&self, decl: &Decl) -> Option<PackageId> {
        self.packages
            .iter()
            .find(|(_, p)| p.body.contains(decl))
            .map(|(id, _)| *id)
    }

    pub fn element_package(&self, id: ElementId) -> Option<PackageId> {
        self.container_of(&Decl::Element(id))
    }

    /// Package names from the outermost to `pkg` itself.
    pub fn package_path(&self, pkg: PackageId) -> Vec<String> {
        let mut path = Vec::new();
        let mut cur = Some(pkg);
        // The depth bound guards against a malformed cyclic tree.
        while let (Some(p), true) = (cur, path.len() <= self.packages.len()) {
            if let Some(package) = self.packages.get(&p) {
                path.push(package.name.clone());
            }
            cur = self.container_of(&Decl::Package(p));
        }
        path.reverse();
        path
    }

    /// `pkg.sub.Name`, the name that identifies an element in a file: the
    /// names of its packages and its own, joined by `.`, each quoted if it
    /// is not a plain name (`billing."Order line"`).
    pub fn full_name(&self, id: ElementId) -> String {
        let Some(el) = self.elements.get(&id) else {
            return String::new();
        };
        match self.element_package(id) {
            Some(p) => format!("{}.{}", self.full_package_name(p), name_in_path(&el.name)),
            None => name_in_path(&el.name).into_owned(),
        }
    }

    /// Like [`Document::full_name`], for a package.
    pub fn full_package_name(&self, id: PackageId) -> String {
        let path: Vec<String> = self
            .package_path(id)
            .iter()
            .map(|n| name_in_path(n).into_owned())
            .collect();
        path.join(".")
    }

    /// The full name of an element or package.
    pub fn endpoint_name(&self, target: Endpoint) -> String {
        match target {
            Endpoint::Element(e) => self.full_name(e),
            Endpoint::Package(p) => self.full_package_name(p),
        }
    }

    /// The package with this full name.
    pub fn find_package(&self, full_name: &str) -> Option<PackageId> {
        self.packages
            .keys()
            .copied()
            .find(|id| self.full_package_name(*id) == full_name)
    }

    /// The element with this full name, or else the package.
    pub fn find_endpoint(&self, full_name: &str) -> Option<Endpoint> {
        self.find(full_name)
            .map(Endpoint::Element)
            .or_else(|| self.find_package(full_name).map(Endpoint::Package))
    }

    pub fn find(&self, full_name: &str) -> Option<ElementId> {
        self.elements
            .keys()
            .copied()
            .find(|id| self.full_name(*id) == full_name)
    }

    pub fn body(&self, pkg: Option<PackageId>) -> &[Decl] {
        match pkg {
            Some(p) => self.packages.get(&p).map(|p| &p.body[..]).unwrap_or(&[]),
            None => &self.root,
        }
    }

    /// Every element inside `pkg`, including nested packages.
    pub fn elements_in(&self, pkg: PackageId) -> Vec<ElementId> {
        let mut out = Vec::new();
        let mut stack = vec![pkg];
        let mut seen = 0;
        while let Some(p) = stack.pop() {
            seen += 1;
            if seen > self.packages.len() {
                break;
            }
            for d in self.body(Some(p)) {
                match d {
                    Decl::Element(e) => out.push(*e),
                    Decl::Package(q) => stack.push(*q),
                    _ => {}
                }
            }
        }
        out
    }

    /// Is `name` free for a new element in `pkg`?
    pub fn name_is_free(&self, name: &str, pkg: Option<PackageId>) -> bool {
        !self.body(pkg).iter().any(|d| match d {
            Decl::Element(e) => self.elements.get(e).is_some_and(|el| el.name == name),
            Decl::Package(p) => self.packages.get(p).is_some_and(|pk| pk.name == name),
            _ => false,
        })
    }

    /// `base`, or `base2`, `base3`... whichever is free in `pkg`.
    pub fn unique_name(&self, base: &str, pkg: Option<PackageId>) -> String {
        if self.name_is_free(base, pkg) {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base}{n}"))
            .find(|n| self.name_is_free(n, pkg))
            .unwrap()
    }

    /// Element ids in the order they are declared in the file.
    pub fn elements_in_order(&self) -> Vec<ElementId> {
        let mut out = Vec::new();
        self.walk(None, &mut |d| {
            if let Decl::Element(e) = d {
                out.push(*e)
            }
        });
        out
    }

    /// Relation ids in the order they are declared in the file.
    pub fn relations_in_order(&self) -> Vec<RelationId> {
        let mut out = Vec::new();
        self.walk(None, &mut |d| {
            if let Decl::Relation(r) = d {
                out.push(*r)
            }
        });
        out
    }

    /// Package ids, parents before children.
    pub fn packages_in_order(&self) -> Vec<PackageId> {
        let mut out = Vec::new();
        self.walk(None, &mut |d| {
            if let Decl::Package(p) = d {
                out.push(*p)
            }
        });
        out
    }

    fn walk(&self, pkg: Option<PackageId>, f: &mut dyn FnMut(&Decl)) {
        self.walk_depth(pkg, f, 0)
    }

    fn walk_depth(&self, pkg: Option<PackageId>, f: &mut dyn FnMut(&Decl), depth: usize) {
        if depth > self.packages.len() {
            return;
        }
        for d in self.body(pkg) {
            f(d);
            if let Decl::Package(p) = d {
                self.walk_depth(Some(*p), f, depth + 1);
            }
        }
    }
}
