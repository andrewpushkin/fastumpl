//! PlantUML's preprocessor, the parts class diagrams use: `!include` of
//! files, macros (`!define`, `!definelong`), variables (`!$x = …`),
//! procedures and functions (`!procedure`, `!function`, `!return`), and
//! conditions (`!if`, `!ifdef`, `!ifndef`, `!elseif`, `!else`, `!endif`).
//!
//! The result is the diagram's lines with every directive done. Each line
//! remembers where it came from: its line in the main file, and for lines
//! from an included file, that file and line, so problems can point at both.
//!
//! Included files are found relative to the file that includes them.
//! Includes from the web (`!includeurl`, `!include https://…`) and from
//! PlantUML's standard library (`!include <C4/C4_Container>`) are not read;
//! each is reported once.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::parser::Problem;

/// Reads an included file; `None` if it cannot be read.
pub type Reader<'a> = &'a dyn Fn(&Path) -> Option<String>;

/// A line of the diagram after preprocessing.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    /// The line in the main file; for included lines, the `!include` line.
    pub line: usize,
    /// For lines from an included file: its name and the line there.
    pub from: Option<(String, usize)>,
}

/// Includes nested deeper than this are not followed (a file including
/// itself through others).
const MAX_DEPTH: usize = 16;
/// Macro expansions in one line, at most: a macro that expands to itself.
const MAX_EXPANSIONS: usize = 64;

#[derive(Clone, Debug)]
struct Macro {
    /// Parameter names; `None` for a macro without parentheses.
    params: Option<Vec<Param>>,
    body: Vec<String>,
    /// A `!function`: a call is replaced by what its `!return` gives.
    function: bool,
}

#[derive(Clone, Debug)]
struct Param {
    name: String,
    default: Option<String>,
}

/// A condition being read: is its current branch taken, and was any.
#[derive(Clone, Copy)]
struct Cond {
    active: bool,
    taken: bool,
    /// The enclosing condition was inactive: every branch is.
    outer_off: bool,
}

/// A source of lines: the main file or an included one.
struct Source<'s> {
    lines: Vec<(usize, &'s str)>,
    /// Folder of the file, for relative includes.
    dir: Option<PathBuf>,
    /// File name shown in problems; `None` for the main file.
    name: Option<String>,
}

pub struct Pre<'r> {
    read: Reader<'r>,
    macros: HashMap<String, Macro>,
    /// Names of macros, longest first, so `AB` is tried before `A`.
    order: Vec<String>,
    vars: HashMap<String, String>,
    conds: Vec<Cond>,
    included: HashSet<PathBuf>,
    reported: HashSet<String>,
    pub problems: Vec<Problem>,
    pub out: Vec<Line>,
    /// Libraries included that fastumpl knows without reading them.
    pub libraries: Vec<String>,
}

/// Preprocesses `lines` (line number and text) of the main file in `dir`.
pub fn preprocess(
    lines: &[(usize, &str)],
    dir: Option<&Path>,
    read: Reader,
) -> (Vec<Line>, Vec<Problem>, Vec<String>) {
    let mut p = Pre {
        read,
        macros: HashMap::new(),
        order: Vec::new(),
        vars: HashMap::new(),
        conds: Vec::new(),
        included: HashSet::new(),
        reported: HashSet::new(),
        problems: Vec::new(),
        out: Vec::new(),
        libraries: Vec::new(),
    };
    let src = Source {
        lines: lines.to_vec(),
        dir: dir.map(Path::to_path_buf),
        name: None,
    };
    p.run(&src, None, 0);
    if !p.conds.is_empty() {
        let line = lines.last().map_or(1, |l| l.0);
        p.problem(line, None, "missing `!endif` at end of file".into());
    }
    (p.out, p.problems, p.libraries)
}

/// The library an include names, if it is one fastumpl knows without
/// reading it: PlantUML's standard library (`<C4/C4_Container>`, shipped
/// inside PlantUML) and copies of its libraries on the web. Their
/// procedures draw boxes, lines and frames, which the importer reads from
/// the calls; only icons and styles are left out.
pub fn known_library(target: &str) -> Option<String> {
    if let Some(inner) = target.strip_prefix('<') {
        let name = inner.trim_end_matches('>').split('/').next().unwrap_or("");
        return (!name.is_empty()).then(|| name.to_string());
    }
    let lower = target.to_ascii_lowercase();
    if !lower.contains("://") {
        return None;
    }
    const KNOWN: [(&str, &str); 7] = [
        ("c4-plantuml", "C4"),
        ("azure-plantuml", "azure"),
        ("aws-icons-for-plantuml", "awslib"),
        ("plantuml-icon-font-sprites", "tupadr3"),
        ("archimate-plantuml", "archimate"),
        ("opensecurityarchitecture", "osa"),
        ("plantuml-stdlib", "stdlib"),
    ];
    KNOWN
        .iter()
        .find(|(part, _)| lower.contains(part))
        .map(|(_, lib)| lib.to_string())
}

/// Is `c` part of a macro or variable name?
fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// The directive word of a `!` line, lower case: `!include x` → `include`.
fn directive(t: &str) -> Option<(String, &str)> {
    let rest = t.strip_prefix('!')?;
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .unwrap_or(rest.len());
    // Directives are any case (`!IfDef`); variables keep theirs (`!$VIEW`).
    let word = if rest.starts_with('$') {
        rest[..end].to_string()
    } else {
        rest[..end].to_ascii_lowercase()
    };
    Some((word, rest[end..].trim()))
}

/// Splits `a, "b, c", f(d, e)` at top-level commas.
fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut quote = false;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '"' => {
                quote = !quote;
                cur.push(c);
            }
            '(' if !quote => {
                depth += 1;
                cur.push(c);
            }
            ')' if !quote => {
                depth -= 1;
                cur.push(c);
            }
            ',' if !quote && depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() || !out.is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// The text inside balanced parentheses starting at `s[0] == '('`, and
/// the byte length up to and including the closing one.
fn balanced(s: &str) -> Option<(&str, usize)> {
    let mut depth = 0;
    let mut quote = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => quote = !quote,
            '(' if !quote => depth += 1,
            ')' if !quote => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[1..i], i + 1));
                }
            }
            _ => {}
        }
    }
    None
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .or_else(|| s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
        .unwrap_or(s)
}

/// Replaces whole-word `name` in `text` with `value`.
fn replace_word(text: &str, name: &str, value: &str) -> String {
    if name.is_empty() || !text.contains(name) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut prev: Option<char> = None;
    while let Some(i) = rest.find(name) {
        let before = rest[..i].chars().next_back().or(prev);
        let after = rest[i + name.len()..].chars().next();
        let starts_word = before.is_none_or(|c| !word_char(c)) || name.starts_with('$');
        // A `$` starts the next variable: `$a$b`.
        let ends_word = after.is_none_or(|c| !word_char(c) || c == '$');
        out.push_str(&rest[..i]);
        if starts_word && ends_word {
            out.push_str(value);
        } else {
            out.push_str(name);
        }
        prev = name.chars().next_back();
        rest = &rest[i + name.len()..];
    }
    out.push_str(rest);
    out
}

/// `##` joins tokens in a macro body: `a ## b` → `ab`.
fn paste(s: &str) -> String {
    if !s.contains("##") {
        return s.to_string();
    }
    s.split("##").map(str::trim).collect::<Vec<_>>().join("")
}

impl Pre<'_> {
    fn problem(&mut self, line: usize, from: Option<&(String, usize)>, message: String) {
        let message = match from {
            Some((file, l)) => format!("in `{file}` line {l}: {message}"),
            None => message,
        };
        self.problems.push(Problem {
            line,
            col: 1,
            message,
        });
    }

    /// Reports `message` only the first time (one per include target).
    fn problem_once(
        &mut self,
        key: String,
        line: usize,
        from: Option<&(String, usize)>,
        message: String,
    ) {
        if self.reported.insert(key) {
            self.problem(line, from, message);
        }
    }

    fn active(&self) -> bool {
        self.conds.last().is_none_or(|c| c.active)
    }

    fn define(&mut self, name: String, m: Macro) {
        if !self.macros.contains_key(&name) {
            let key = |n: &String| (std::cmp::Reverse(n.len()), n.clone());
            let at = self
                .order
                .binary_search_by(|n| key(n).cmp(&key(&name)))
                .unwrap_or_else(|i| i);
            self.order.insert(at, name.clone());
        }
        self.macros.insert(name, m);
    }

    fn undefine(&mut self, name: &str) {
        if self.macros.remove(name).is_some() {
            self.order.retain(|n| n != name);
        }
        self.vars.remove(name);
    }

    fn is_defined(&self, name: &str) -> bool {
        self.macros.contains_key(name) || self.vars.contains_key(name)
    }

    /// Reads `src`'s lines. `main_line` is the main file's line for lines
    /// of an included file.
    fn run(&mut self, src: &Source, main_line: Option<usize>, depth: usize) {
        let mut i = 0;
        while i < src.lines.len() {
            let (n, raw) = src.lines[i];
            i += 1;
            let line = main_line.unwrap_or(n);
            let from = src.name.as_ref().map(|f| (f.clone(), n));
            let t = raw.trim();
            if let Some((word, rest)) = directive(t) {
                // Blocks: their lines up to the closing directive.
                let end = match word.as_str() {
                    "definelong" => Some("enddefinelong"),
                    "procedure" => Some("endprocedure"),
                    "function" => Some("endfunction"),
                    "unquoted" if rest.starts_with("function") => Some("endfunction"),
                    "unquoted" => Some("endprocedure"),
                    _ => None,
                };
                if let Some(end) = end {
                    let start = i;
                    while i < src.lines.len()
                        && directive(src.lines[i].1.trim()).is_none_or(|(w, _)| w != end)
                    {
                        i += 1;
                    }
                    let body: Vec<String> = src.lines[start..i.min(src.lines.len())]
                        .iter()
                        .map(|(_, l)| l.to_string())
                        .collect();
                    if i >= src.lines.len() {
                        self.problem(line, from.as_ref(), format!("missing `!{end}`"));
                    }
                    i += 1;
                    if self.active() {
                        self.block(&word, rest, body);
                    }
                    continue;
                }
                self.directive(&word, rest, src, line, from.as_ref(), depth);
                continue;
            }
            if !self.active() {
                continue;
            }
            for text in self.expand(raw, 0) {
                self.out.push(Line {
                    text,
                    line,
                    from: from.clone(),
                });
            }
        }
    }

    /// `!definelong`, `!procedure` and `!function` with their bodies.
    fn block(&mut self, word: &str, rest: &str, body: Vec<String>) {
        let rest = match word {
            // `!unquoted procedure $x()` / `!unquoted function`.
            "unquoted" => rest
                .trim_start_matches("procedure")
                .trim_start_matches("function")
                .trim(),
            _ => rest,
        };
        let Some((name, params)) = signature(rest) else {
            return;
        };
        let function = word == "function"
            || (word == "unquoted" && body.iter().any(|l| l.trim().starts_with("!return")));
        self.define(
            name,
            Macro {
                params: Some(params),
                body,
                function,
            },
        );
    }

    fn directive(
        &mut self,
        word: &str,
        rest: &str,
        src: &Source,
        line: usize,
        from: Option<&(String, usize)>,
        depth: usize,
    ) {
        // Conditions are followed even inside inactive branches.
        match word {
            "ifdef" | "ifndef" | "if" => {
                let outer_off = !self.active();
                let yes = match word {
                    "ifdef" => self.is_defined(rest.trim()),
                    "ifndef" => !self.is_defined(rest.trim()),
                    _ => self.condition(rest),
                };
                let active = !outer_off && yes;
                self.conds.push(Cond {
                    active,
                    taken: active,
                    outer_off,
                });
                return;
            }
            "elseif" | "else" => {
                let yes = word == "else" || self.condition(rest);
                if let Some(c) = self.conds.last_mut() {
                    c.active = !c.outer_off && !c.taken && yes;
                    c.taken |= c.active;
                } else {
                    self.problem(line, from, format!("`!{word}` without `!if`"));
                }
                return;
            }
            "endif" => {
                if self.conds.pop().is_none() {
                    self.problem(line, from, "`!endif` without `!if`".into());
                }
                return;
            }
            _ => {}
        }
        if !self.active() {
            return;
        }
        match word {
            "define" => {
                if let Some((name, m)) = define_line(rest) {
                    self.define(name, m);
                }
            }
            "undef" => self.undefine(rest.trim()),
            "include" | "include_many" | "include_once" | "includesub" | "includeurl"
            | "import" => self.include(word, rest, src, line, from, depth),
            "startsub" | "endsub" | "pragma" | "assert" | "log" | "dump_memory" | "option"
            | "return" => {}
            w if w.starts_with('$') || w == "global" || w == "local" => self.assign(word, rest),
            _ => {
                // Not ours (`!theme` and the like): the importer reports it.
                self.out.push(Line {
                    text: format!("!{word} {rest}").trim().to_string(),
                    line,
                    from: from.cloned(),
                });
            }
        }
    }

    /// `!$x = value`, `!$x ?= value`, `!global $x = value`.
    fn assign(&mut self, word: &str, rest: &str) {
        let (name, rest) = if word.starts_with('$') {
            (format!("${}", word.trim_start_matches('$')), rest)
        } else {
            let mut parts = rest.splitn(2, |c: char| c.is_whitespace() || c == '=' || c == '?');
            let n = parts.next().unwrap_or("").to_string();
            let r = rest[n.len()..].trim_start();
            (n, r)
        };
        let (only_if_new, value) = if let Some(v) = rest.strip_prefix("?=") {
            (true, v)
        } else if let Some(v) = rest.strip_prefix('=') {
            (false, v)
        } else {
            return;
        };
        if only_if_new && self.vars.contains_key(&name) {
            return;
        }
        let value = self.expand_inline(value.trim());
        let value = value
            .split(" + ")
            .map(|p| unquote(p).to_string())
            .collect::<String>();
        self.vars.insert(name, value);
    }

    /// The truth of an `!if` condition. Understood: numbers, `%true()`,
    /// `%false()`, `%defined(X)`, `%variable_exists(X)`, `==`, `!=`, `!`,
    /// `&&` and `||`. Anything else counts as true.
    fn condition(&mut self, expr: &str) -> bool {
        let e = self.expand_inline(expr.trim());
        eval(&e, &|name| self.is_defined(name))
    }

    fn include(
        &mut self,
        word: &str,
        rest: &str,
        src: &Source,
        line: usize,
        from: Option<&(String, usize)>,
        depth: usize,
    ) {
        let target = self.expand_inline(unquote(rest));
        // Windows paths work on every system: `..\lib\a.iuml`.
        let target = unquote(&target).trim().replace('\\', "/");
        let skipped = |what: &str| format!("{what} `{target}` is not read; line skipped");
        if let Some(lib) = known_library(&target) {
            if !self.libraries.contains(&lib) {
                self.libraries.push(lib);
            }
            return;
        }
        if word == "includeurl" || target.contains("://") {
            return self.problem_once(target.clone(), line, from, skipped("include from the web"));
        }
        if word == "import" {
            return self.problem_once(target.clone(), line, from, skipped("`!import` of"));
        }
        // file!3, file!ID or (includesub) file!PART.
        let (file, part) = match target.rsplit_once('!') {
            Some((f, p)) if !f.is_empty() => (f.trim(), Some(p.trim())),
            _ => (target.as_str(), None),
        };
        let Some(dir) = &src.dir else {
            return self.problem_once(
                target.clone(),
                line,
                from,
                skipped("include (no folder to look in)"),
            );
        };
        let path = dir.join(file);
        let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if depth >= MAX_DEPTH {
            return self.problem(
                line,
                from,
                format!("`{target}` includes itself; line skipped"),
            );
        }
        if word == "include_once" && self.included.contains(&key) {
            return;
        }
        if word == "include" && part.is_none() && self.included.contains(&key) {
            // PlantUML includes a file once unless asked with `!include_many`.
            return;
        }
        let Some(text) = (self.read)(&path) else {
            return self.problem_once(
                key.display().to_string(),
                line,
                from,
                format!("cannot read included file `{target}`; line skipped"),
            );
        };
        self.included.insert(key);
        let text = super::one_line_end(&text).into_owned();
        let all: Vec<(usize, &str)> = text.lines().enumerate().map(|(k, l)| (k + 1, l)).collect();
        let lines = if word == "includesub" {
            let Some(part) = part else {
                return self.problem(
                    line,
                    from,
                    format!("`!includesub` needs `file!PART`: `{target}`"),
                );
            };
            sub_lines(&all, part)
        } else {
            diagram_lines(&all, part)
        };
        let name = Path::new(file)
            .file_name()
            .map_or(file.to_string(), |n| n.to_string_lossy().into_owned());
        let inner = Source {
            lines,
            dir: path.parent().map(Path::to_path_buf),
            name: Some(name),
        };
        self.run(&inner, Some(line), depth + 1);
    }

    /// Expands a line for use inside a directive (one line of text).
    fn expand_inline(&mut self, text: &str) -> String {
        self.expand(text, 0).join("\n")
    }

    /// Expands variables and macros in `text`. Multi-line macros and
    /// procedures give several lines.
    fn expand(&mut self, text: &str, level: usize) -> Vec<String> {
        let mut text = text.to_string();
        for (k, v) in &self.vars {
            if text.contains(k.as_str()) {
                text = replace_word(&text, k, v);
            }
        }
        if self.order.is_empty() || level > 8 {
            return vec![text];
        }
        let mut done = 0;
        while done < MAX_EXPANSIONS {
            // The first macro used in the text, longest names first.
            let found = self.order.iter().find_map(|name| {
                let m = &self.macros[name];
                find_call(&text, name, m.params.is_some())
                    .map(|(at, args, len)| (name.clone(), at, args, len))
            });
            let Some((name, at, args, len)) = found else {
                break;
            };
            done += 1;
            let m = self.macros[&name].clone();
            // A macro is not expanded inside its own expansion:
            // `!define PK <b>PK</b>` gives `<b>PK</b>`, once.
            let shield = shield(&name);
            let body: Vec<String> = self
                .instantiate(&m, &args)
                .iter()
                .map(|l| replace_word(l, &name, &shield))
                .collect();
            let (before, tail) = (text[..at].to_string(), text[at + len..].to_string());
            if m.function {
                let value = self.call_function(&body);
                text = format!("{before}{value}{tail}");
                continue;
            }
            if body.len() == 1 {
                text = format!("{before}{}{tail}", body[0]);
                continue;
            }
            // A multi-line body: the text around the call goes on the first
            // and last lines; each line is expanded again.
            let mut lines = Vec::new();
            let last = body.len() - 1;
            for (k, b) in body.into_iter().enumerate() {
                let mut l = b;
                if k == 0 {
                    l = format!("{before}{l}");
                }
                if k == last {
                    l = format!("{l}{tail}");
                }
                lines.extend(self.body_line(&l, level));
            }
            return lines.iter().map(|l| unshield(l)).collect();
        }
        vec![unshield(&text)]
    }

    /// A line of a procedure body: directives inside run (variables,
    /// conditions), other lines are expanded again.
    fn body_line(&mut self, l: &str, level: usize) -> Vec<String> {
        let t = l.trim();
        if let Some((word, rest)) = directive(t) {
            match word.as_str() {
                "if" | "ifdef" | "ifndef" | "elseif" | "else" | "endif" => {
                    let src = Source {
                        lines: Vec::new(),
                        dir: None,
                        name: None,
                    };
                    self.directive(&word, rest, &src, 0, None, MAX_DEPTH);
                    return Vec::new();
                }
                w if w.starts_with('$') || w == "local" || w == "global" => {
                    if self.active() {
                        self.assign(&word, rest);
                    }
                    return Vec::new();
                }
                _ => return Vec::new(),
            }
        }
        if !self.active() {
            return Vec::new();
        }
        self.expand(l, level + 1)
    }

    /// The body of `m` with `args` put in for its parameters.
    fn instantiate(&self, m: &Macro, args: &[String]) -> Vec<String> {
        let Some(params) = &m.params else {
            return m.body.clone();
        };
        let mut values: Vec<(String, String)> = Vec::new();
        for (k, p) in params.iter().enumerate() {
            // Named arguments: `$x="v"`.
            let named = args.iter().find_map(|a| {
                let (n, v) = a.split_once('=')?;
                (n.trim() == p.name).then(|| v.trim().to_string())
            });
            let v = named
                .or_else(|| {
                    args.get(k)
                        .filter(|a| !a.contains('=') || a.starts_with('"'))
                        .cloned()
                })
                .or_else(|| p.default.clone())
                .unwrap_or_default();
            // A quoted argument goes in without its quotes.
            values.push((p.name.clone(), unquote(&v).to_string()));
        }
        // Longest names first, so `$ab` is not replaced as `$a` + `b`.
        values.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        m.body
            .iter()
            .map(|line| {
                let mut l = line.clone();
                for (n, v) in &values {
                    l = replace_word(&l, n, v);
                }
                paste(&l)
            })
            .collect()
    }

    /// Runs a function's body; returns what its `!return` gives.
    fn call_function(&mut self, body: &[String]) -> String {
        let depth = self.conds.len();
        let mut result = String::new();
        for l in body {
            let t = l.trim();
            if let Some(("return", rest)) = directive(t).as_ref().map(|(w, r)| (w.as_str(), *r)) {
                if self.active() {
                    let v = self.expand_inline(rest);
                    result = v.split(" + ").map(|p| unquote(p).to_string()).collect();
                    break;
                }
                continue;
            }
            self.body_line(l, 1);
        }
        self.conds.truncate(depth);
        result
    }
}

/// `name` hidden from macro lookup until [`unshield`]: each character
/// moved into a private-use plane, so the text no longer holds the name.
fn shield(name: &str) -> String {
    name.chars()
        .map(|c| char::from_u32(SHIELD + c as u32).unwrap_or(c))
        .collect()
}

/// Start of the plane [`shield`] moves characters into.
const SHIELD: u32 = 0xF0000;

fn unshield(text: &str) -> String {
    text.chars()
        .map(|c| match c as u32 {
            u if (SHIELD..SHIELD + 0x10000).contains(&u) => char::from_u32(u - SHIELD).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// Finds whole-word `name` in `text`, from byte `from` on.
fn find_word(text: &str, name: &str, mut from: usize) -> Option<usize> {
    while let Some(i) = text[from..].find(name) {
        let at = from + i;
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        let starts = before.is_none_or(|c| !word_char(c) || name.starts_with('$'));
        if starts && after.is_none_or(|c| !word_char(c) || c == '$') {
            return Some(at);
        }
        from = at + name.len();
    }
    None
}

/// Where macro `name` is used in `text`: its position, its arguments and
/// the length of the use (name and parentheses). A macro with parameters
/// is only used where parentheses follow its name.
fn find_call(text: &str, name: &str, with_args: bool) -> Option<(usize, Vec<String>, usize)> {
    let mut from = 0;
    while let Some(at) = find_word(text, name, from) {
        let after = &text[at + name.len()..];
        if !with_args {
            return Some((at, Vec::new(), name.len()));
        }
        let skip = after.len() - after.trim_start().len();
        if after[skip..].starts_with('(') {
            if let Some((inside, l)) = balanced(&after[skip..]) {
                return Some((at, split_args(inside), name.len() + skip + l));
            }
        }
        from = at + name.len();
    }
    None
}

/// `NAME(a, b="x")` or `$name($a, $b)`: the name and its parameters.
fn signature(s: &str) -> Option<(String, Vec<Param>)> {
    let s = s.trim();
    let open = s.find('(')?;
    let name = s[..open].trim().to_string();
    if name.is_empty() {
        return None;
    }
    let (inside, _) = balanced(&s[open..])?;
    let params = split_args(inside)
        .into_iter()
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((n, d)) => Param {
                name: n.trim().to_string(),
                default: Some(d.trim().to_string()),
            },
            None => Param {
                name: p.trim().to_string(),
                default: None,
            },
        })
        .collect();
    Some((name, params))
}

/// `!define NAME value` or `!define NAME(a, b) value`.
fn define_line(rest: &str) -> Option<(String, Macro)> {
    let rest = rest.trim();
    let end = rest.find(|c: char| !word_char(c)).unwrap_or(rest.len());
    let name = &rest[..end];
    if name.is_empty() {
        return None;
    }
    let after = &rest[end..];
    if after.starts_with('(') {
        let (_, len) = balanced(after)?;
        let (_, params) = signature(&rest[..end + len])?;
        let body = after[len..].trim().to_string();
        return Some((
            name.to_string(),
            Macro {
                params: Some(params),
                body: vec![body],
                function: false,
            },
        ));
    }
    Some((
        name.to_string(),
        Macro {
            params: None,
            body: vec![after.trim().to_string()],
            function: false,
        },
    ))
}

/// The lines of an included file to use: the `part`-th diagram (`file!2`)
/// or the one with that id (`@startuml(id=ID)`), else the first diagram;
/// the whole file if it has no `@start` line.
fn diagram_lines<'s>(all: &[(usize, &'s str)], part: Option<&str>) -> Vec<(usize, &'s str)> {
    let starts: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, (_, l))| l.trim_start().to_ascii_lowercase().starts_with("@start"))
        .map(|(k, _)| k)
        .collect();
    if starts.is_empty() {
        return all.to_vec();
    }
    let pick = match part {
        Some(p) => match p.parse::<usize>() {
            Ok(k) => starts.get(k).copied(),
            Err(_) => starts
                .iter()
                .copied()
                .find(|&k| all[k].1.replace(' ', "").contains(&format!("id={p}"))),
        },
        None => None,
    }
    .unwrap_or(starts[0]);
    all[pick + 1..]
        .iter()
        .take_while(|(_, l)| !l.trim_start().to_ascii_lowercase().starts_with("@end"))
        .copied()
        .collect()
}

/// The lines between `!startsub part` and `!endsub`.
fn sub_lines<'s>(all: &[(usize, &'s str)], part: &str) -> Vec<(usize, &'s str)> {
    let mut out = Vec::new();
    let mut inside = false;
    for &(n, l) in all {
        match directive(l.trim()) {
            Some((w, rest)) if w == "startsub" && rest.trim() == part => inside = true,
            Some((w, _)) if w == "endsub" => inside = false,
            _ if inside => out.push((n, l)),
            _ => {}
        }
    }
    out
}

/// Evaluates an `!if` condition; unknown parts count as true.
fn eval(e: &str, defined: &dyn Fn(&str) -> bool) -> bool {
    let e = e.trim();
    if let Some(inner) = e.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        if balanced(e).is_some_and(|(_, l)| l == e.len()) {
            return eval(inner, defined);
        }
    }
    for op in ["||", "&&"] {
        if let Some((a, b)) = split_top(e, op) {
            let (a, b) = (eval(a, defined), eval(b, defined));
            return if op == "||" { a || b } else { a && b };
        }
    }
    for op in ["==", "!="] {
        if let Some((a, b)) = split_top(e, op) {
            let same = unquote(a) == unquote(b);
            return if op == "==" { same } else { !same };
        }
    }
    if let Some(x) = e.strip_prefix('!') {
        return !eval(x, defined);
    }
    let lower = e.to_ascii_lowercase();
    if lower == "%true()" || lower == "true" {
        return true;
    }
    if lower == "%false()" || lower == "false" || lower.is_empty() {
        return false;
    }
    for f in ["%defined(", "%variable_exists(", "%function_exists("] {
        if let Some(rest) = lower.strip_prefix(f) {
            let name = unquote(e[f.len()..e.len() - 1].trim());
            let _ = rest;
            return defined(name);
        }
    }
    if let Ok(n) = e.parse::<i64>() {
        return n != 0;
    }
    if e.starts_with('"') {
        return !unquote(e).is_empty();
    }
    true
}

/// Splits at the first `op` outside quotes and parentheses.
fn split_top<'e>(e: &'e str, op: &str) -> Option<(&'e str, &'e str)> {
    let mut depth = 0;
    let mut quote = false;
    for (i, c) in e.char_indices() {
        match c {
            '"' => quote = !quote,
            '(' if !quote => depth += 1,
            ')' if !quote => depth -= 1,
            _ if !quote && depth == 0 && e[i..].starts_with(op) => {
                return Some((&e[..i], &e[i + op.len()..]));
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str, files: &[(&str, &str)]) -> (Vec<String>, Vec<Problem>) {
        let files: HashMap<PathBuf, String> = files
            .iter()
            .map(|(n, t)| (Path::new("/d").join(n), t.to_string()))
            .collect();
        let read = |p: &Path| files.get(p).cloned();
        let lines: Vec<(usize, &str)> = src.lines().enumerate().map(|(k, l)| (k + 1, l)).collect();
        let (out, problems, _) = preprocess(&lines, Some(Path::new("/d")), &read);
        (out.into_iter().map(|l| l.text).collect(), problems)
    }

    #[test]
    fn includes_files_relative_to_the_includer() {
        let files = [
            ("common.iuml", "class Base\n!include sub/more.iuml\n"),
            (
                "sub/more.iuml",
                "@startuml\nclass More\n@enduml\n@startuml\nclass Second\n@enduml\n",
            ),
            (
                "parts.iuml",
                "!startsub A\nclass A1\n!endsub\nclass Outside\n!startsub B\nclass B1\n!endsub\n",
            ),
        ];
        let (out, problems) = run(
            "!include common.iuml\n!include_once common.iuml\n!include sub/more.iuml!1\n!includesub parts.iuml!B\n!include missing.iuml\n!include <C4/C4>\n!includeurl http://x/y.puml\nclass Main\n",
            &files,
        );
        assert_eq!(
            out,
            [
                "class Base",
                "class More",
                "class Second",
                "class B1",
                "class Main"
            ]
        );
        // The standard library is known without reading it; a web include
        // of an unknown file is not.
        let m: Vec<&str> = problems.iter().map(|p| p.message.as_str()).collect();
        assert_eq!(m.len(), 2, "{m:?}");
        assert!(m[0].contains("cannot read") && problems[0].line == 5);
        assert!(m[1].contains("from the web"));
        assert_eq!(known_library("<C4/C4_Container>").as_deref(), Some("C4"));
        assert_eq!(
            known_library("https://raw.githubusercontent.com/plantuml-stdlib/C4-PlantUML/master/C4_Container.puml")
                .as_deref(),
            Some("C4")
        );
        assert_eq!(known_library("https://example.org/theme.puml"), None);
        assert_eq!(known_library("lib/common.iuml"), None);
    }

    #[test]
    fn included_lines_know_where_they_came_from() {
        let read = |_: &Path| Some("class A\n???\n".to_string());
        let lines = [(1, "' x"), (2, "!include a.iuml")];
        let (out, _, _) = preprocess(&lines, Some(Path::new("/d")), &read);
        assert_eq!(out[0].from, None);
        assert_eq!(out[2].text, "???");
        assert_eq!(out[2].line, 2);
        assert_eq!(out[2].from, Some(("a.iuml".to_string(), 2)));
    }

    #[test]
    fn variables_keep_their_case() {
        let src = "!$VIEW = \"all\"\n!if ($VIEW == \"all\" || $VIEW == \"core\")\nclass A\n!endif\n!if ($VIEW != \"all\")\nclass B\n!endif\n!$view = \"core\"\n!IF ($VIEW == \"all\")\nclass C\n!ENDIF\n";
        let (out, problems) = run(src, &[]);
        assert_eq!(problems, vec![]);
        assert_eq!(out, ["class A", "class C"]);
    }

    #[test]
    fn a_file_including_itself_stops() {
        let files = [("a.iuml", "class A\n!include_many a.iuml\n")];
        let (out, problems) = run("!include_many a.iuml\n", &files);
        assert_eq!(out.len(), MAX_DEPTH);
        assert!(problems[0].message.contains("includes itself"));
    }

    #[test]
    fn macros_and_variables() {
        let src = r#"!define Table(name,desc) class name as "desc" << (T,#FFAAAA) >>
!define primary_key(x) <b>x</b>
!define TYPE int
!$prefix = "app_"
!definelong Pair(a, b)
class a
class b
a --> b
!enddefinelong
Table(user, "Users") {
  primary_key(id) TYPE
  $prefix
}
Pair(X, Y)
"#;
        let (out, problems) = run(src, &[]);
        assert_eq!(problems, vec![]);
        assert_eq!(
            out,
            [
                r#"class user as "Users" << (T,#FFAAAA) >> {"#,
                "  <b>id</b> int",
                "  app_",
                "}",
                "class X",
                "class Y",
                "X --> Y",
            ]
        );
    }

    #[test]
    fn a_macro_is_not_expanded_inside_itself() {
        let src = "!define PK <b>PK</b>\n!define NN <i>NN</i> PK\nPK id\nNN name\n";
        let (out, problems) = run(src, &[]);
        assert_eq!(problems, vec![]);
        assert_eq!(out, ["<b>PK</b> id", "<i>NN</i> <b>PK</b> name"]);
    }

    #[test]
    fn procedures_functions_and_conditions() {
        let src = r#"!$mode = "full"
!procedure $entity($name, $kind="class")
$kind $name
!endprocedure
!function $suffix($x)
!return $x + "Impl"
!endfunction
!if $mode == "full"
$entity("Order")
$entity(Shape, "interface")
!else
class Short
!endif
!ifdef NOT_DEFINED
class Never
!elseif %true()
class $suffix("Repo")
!endif
!ifndef NOT_DEFINED
class Yes
!endif
"#;
        let (out, problems) = run(src, &[]);
        assert_eq!(problems, vec![]);
        assert_eq!(
            out,
            [
                "class Order",
                "interface Shape",
                "class RepoImpl",
                "class Yes"
            ]
        );
    }

    #[test]
    fn unclosed_blocks_are_reported() {
        let (out, problems) = run("!if 1\nclass A\n", &[]);
        assert_eq!(out, ["class A"]);
        assert!(problems[0].message.contains("!endif"));
        let (_, problems) = run("!procedure $p()\nclass A\n", &[]);
        assert!(problems[0].message.contains("!endprocedure"));
    }

    #[test]
    fn words_are_replaced_whole() {
        assert_eq!(replace_word("A AB A_ (A)", "A", "x"), "x AB A_ (x)");
        assert_eq!(replace_word("$a$a", "$a", "1"), "11");
        assert_eq!(
            split_args(r#"a, "b, c", f(d, e)"#),
            ["a", "\"b, c\"", "f(d, e)"]
        );
        assert!(eval("1 && !0", &|_| false));
        assert!(!eval(r#""a" == "b" || %false()"#, &|_| false));
        assert!(eval("%defined(\"X\")", &|n| n == "X"));
    }
}
