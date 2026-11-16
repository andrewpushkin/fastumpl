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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElementKind {
    Class,
    Abstract,
    Interface,
    Enum,
    Note,
}

impl ElementKind {
    pub const ALL: [ElementKind; 5] = [
        ElementKind::Class,
        ElementKind::Abstract,
        ElementKind::Interface,
        ElementKind::Enum,
        ElementKind::Note,
    ];

    /// The keyword(s) that introduce this element in a `.umpl` file.
    pub fn keyword(self) -> &'static str {
        match self {
            ElementKind::Class => "class",
            ElementKind::Abstract => "abstract class",
            ElementKind::Interface => "interface",
            ElementKind::Enum => "enum",
            ElementKind::Note => "note",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ElementKind::Class => "Class",
            ElementKind::Abstract => "Abstract class",
            ElementKind::Interface => "Interface",
            ElementKind::Enum => "Enum",
            ElementKind::Note => "Note",
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
    pub name: String,
    pub kind: ElementKind,
    pub stereotype: Option<String>,
    /// Attributes and operations (classes), literals (enums) or text lines
    /// (notes).
    pub members: Vec<Member>,
    pub pos: Option<Pos>,
    pub comments: Vec<String>,
}

impl Element {
    pub fn new(kind: ElementKind, name: impl Into<String>) -> Element {
        Element {
            name: name.into(),
            kind,
            stereotype: None,
            members: Vec::new(),
            pos: None,
            comments: Vec::new(),
        }
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
    /// Dashed line between a note and what it describes.
    NoteLink,
}

impl RelationKind {
    pub const ALL: [RelationKind; 8] = [
        RelationKind::Generalization,
        RelationKind::Realization,
        RelationKind::Association,
        RelationKind::Directed,
        RelationKind::Aggregation,
        RelationKind::Composition,
        RelationKind::Dependency,
        RelationKind::NoteLink,
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
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct End {
    pub element: ElementId,
    /// `"mult role"`: multiplicity, role name, or both.
    pub label: Option<String>,
}

impl End {
    pub fn new(element: ElementId) -> End {
        End {
            element,
            label: None,
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
        self.from.element == id || self.to.element == id
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
    pub comments: Vec<String>,
}

impl Package {
    pub fn new(name: impl Into<String>) -> Package {
        Package {
            name: name.into(),
            body: Vec::new(),
            comments: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub title: Option<String>,
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

    /// `pkg.sub.Name`, the name that identifies an element in a file.
    pub fn full_name(&self, id: ElementId) -> String {
        let Some(el) = self.elements.get(&id) else {
            return String::new();
        };
        match self.element_package(id) {
            Some(p) => {
                let mut path = self.package_path(p);
                path.push(el.name.clone());
                path.join(".")
            }
            None => el.name.clone(),
        }
    }

    pub fn full_package_name(&self, id: PackageId) -> String {
        self.package_path(id).join(".")
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
