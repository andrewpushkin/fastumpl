//! Small scanning helpers over one line of text.

use umpl_core::RelationKind;

/// A cursor over one line.
#[derive(Clone)]
pub struct Cursor<'a> {
    pub s: &'a str,
    pub pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(s: &'a str) -> Self {
        Cursor { s, pos: 0 }
    }

    pub fn rest(&self) -> &'a str {
        &self.s[self.pos..]
    }

    pub fn at_end(&self) -> bool {
        self.rest().trim().is_empty()
    }

    pub fn skip_ws(&mut self) {
        let r = self.rest();
        self.pos += r.len() - r.trim_start().len();
    }

    pub fn eat(&mut self, token: &str) -> bool {
        self.skip_ws();
        if self.rest().starts_with(token) {
            self.pos += token.len();
            true
        } else {
            false
        }
    }

    /// Eats a keyword followed by whitespace or end of line.
    pub fn eat_word(&mut self, word: &str) -> bool {
        self.skip_ws();
        let r = self.rest();
        if let Some(after) = r.strip_prefix(word) {
            if after.is_empty() || after.starts_with(char::is_whitespace) || after.starts_with('{')
            {
                self.pos += word.len();
                return true;
            }
        }
        false
    }

    /// One name: a plain name, or any text in double quotes.
    pub fn segment(&mut self) -> Option<String> {
        self.skip_ws();
        let r = self.rest();
        if r.starts_with('"') {
            let save = self.pos;
            let s = self.string()?;
            if s.is_empty() {
                self.pos = save;
                return None;
            }
            return Some(s);
        }
        let first = r.chars().next()?;
        if !(first.is_alphabetic() || first == '_') {
            return None;
        }
        let len = r
            .char_indices()
            .find(|(_, c)| !(c.is_alphanumeric() || *c == '_'))
            .map(|(i, _)| i)
            .unwrap_or(r.len());
        self.pos += len;
        Some(r[..len].to_string())
    }

    /// A dotted path of names, `billing."Order line"`, in the form
    /// [`umpl_core::Document::full_name`] gives: plain names unquoted.
    pub fn path(&mut self) -> Option<String> {
        let save = self.pos;
        let mut parts = vec![self.segment()?];
        loop {
            let r = self.rest();
            let joins = r.strip_prefix('.').is_some_and(|after| {
                after.starts_with('"') || after.starts_with(|c: char| c.is_alphabetic() || c == '_')
            });
            if !joins {
                break;
            }
            self.pos += 1;
            match self.segment() {
                Some(p) => parts.push(p),
                None => {
                    self.pos = save;
                    return None;
                }
            }
        }
        let parts: Vec<String> = parts
            .iter()
            .map(|p| umpl_core::name_in_path(p).into_owned())
            .collect();
        Some(parts.join("."))
    }

    /// A double-quoted string with `\"`, `\\` and `\n` escapes.
    pub fn string(&mut self) -> Option<String> {
        self.skip_ws();
        let r = self.rest();
        if !r.starts_with('"') {
            return None;
        }
        let mut out = String::new();
        let mut chars = r.char_indices().skip(1);
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => {
                    self.pos += i + 1;
                    return Some(out);
                }
                '\\' => match chars.next() {
                    Some((_, 'n')) => out.push('\n'),
                    Some((_, c)) => out.push(c),
                    None => return None,
                },
                c => out.push(c),
            }
        }
        None
    }

    pub fn int(&mut self) -> Option<i32> {
        self.skip_ws();
        let r = self.rest();
        let len = r
            .char_indices()
            .find(|(i, c)| !(c.is_ascii_digit() || (*i == 0 && *c == '-')))
            .map(|(i, _)| i)
            .unwrap_or(r.len());
        let v = r[..len].parse().ok()?;
        self.pos += len;
        Some(v)
    }

    /// A relation arrow. Returns the kind and whether it was written
    /// reversed (`<|--` instead of `--|>`).
    pub fn arrow(&mut self) -> Option<(RelationKind, bool)> {
        use RelationKind::*;
        // Longest first, so `--|>` wins over `--`.
        const ARROWS: [(&str, RelationKind, bool); 16] = [
            ("--|>", Generalization, false),
            ("<|--", Generalization, true),
            ("..|>", Realization, false),
            ("<|..", Realization, true),
            ("-->", Directed, false),
            ("<--", Directed, true),
            ("..>", Dependency, false),
            ("<..", Dependency, true),
            ("*--", Composition, false),
            ("--*", Composition, true),
            ("o--", Aggregation, false),
            ("--o", Aggregation, true),
            ("+--", Nesting, false),
            ("--+", Nesting, true),
            ("--", Association, false),
            ("..", NoteLink, false),
        ];
        self.skip_ws();
        let r = self.rest();
        for (text, kind, rev) in ARROWS {
            if let Some(after) = r.strip_prefix(text) {
                // `--o` must not swallow the first letter of a name.
                if text.ends_with('o')
                    && after.starts_with(|c: char| c.is_alphanumeric() || c == '_')
                {
                    continue;
                }
                // `o--` must be a separate word.
                if text.starts_with('o') && self.pos > 0 {
                    let before = self.s[..self.pos].chars().last();
                    if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                }
                self.pos += text.len();
                return Some((kind, rev));
            }
        }
        None
    }

    /// Type parameters after a name: `<T>`, `<K, V>`, `<T extends Map<K, V>>`
    /// (the text between the outer angle brackets). Not a `<<stereotype>>`.
    pub fn type_params(&mut self) -> Option<String> {
        let r = self.rest();
        if !r.starts_with('<') || r.starts_with("<<") {
            return None;
        }
        // The `>` that closes the brackets, or a later one if more text
        // follows it directly: `<a>b>` holds `a>b`.
        let mut depth = 0;
        for (i, c) in r.char_indices() {
            match c {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth <= 0 && ends_token(&r[i + 1..]) {
                        let inner = r[1..i].trim();
                        if inner.is_empty() {
                            return None;
                        }
                        self.pos += i + 1;
                        return Some(inner.to_string());
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// A colour: `#lightblue`, `#a1b2c3`, `#transparent` (without the `#`,
    /// in lower case).
    pub fn color(&mut self) -> Option<String> {
        self.skip_ws();
        let r = self.rest();
        let after = r.strip_prefix('#')?;
        let len = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        if len == 0 {
            return None;
        }
        self.pos += 1 + len;
        Some(after[..len].to_ascii_lowercase())
    }

    /// `<<stereo>>` or `«stereo»`.
    pub fn stereotype(&mut self) -> Option<String> {
        self.skip_ws();
        let r = self.rest();
        for (open, close) in [("<<", ">>"), ("«", "»")] {
            if let Some(after) = r.strip_prefix(open) {
                // The first close that ends the token: `<<a>>b>>` is `a>>b`.
                let end = after
                    .match_indices(close)
                    .map(|(i, _)| i)
                    .find(|&i| ends_token(&after[i + close.len()..]))
                    .or_else(|| after.find(close));
                if let Some(end) = end {
                    self.pos += open.len() + end + close.len();
                    return Some(after[..end].trim().to_string());
                }
            }
        }
        None
    }
}

/// Does `rest`, the text after a closing `>` or `>>`, leave it closing?
/// Not when a name goes on directly (`<a>b>`, `<<a>>b>>`).
fn ends_token(rest: &str) -> bool {
    !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_')
}

/// Quotes a string for writing, escaping `\`, `"` and newlines.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// A valid `.umpl` name made from any text: `Long name` → `Long_name`.
pub fn clean(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().chars() {
        if c.is_alphanumeric() || c == '_' {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    match out.chars().next() {
        None => "Unnamed".to_string(),
        Some(c) if !c.is_alphabetic() => format!("_{out}"),
        _ => out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_relation_parts() {
        let mut c = Cursor::new(r#"Order "1" *-- "1..*" OrderLine : contains"#);
        assert_eq!(c.path().as_deref(), Some("Order"));
        assert_eq!(c.string().as_deref(), Some("1"));
        assert_eq!(c.arrow(), Some((RelationKind::Composition, false)));
        assert_eq!(c.string().as_deref(), Some("1..*"));
        assert_eq!(c.path().as_deref(), Some("OrderLine"));
        assert!(c.eat(":"));
    }

    #[test]
    fn paths_of_plain_and_quoted_names() {
        let mut c = Cursor::new(r#"billing."Order line" --> "Plain".x.y"#);
        assert_eq!(c.path().as_deref(), Some(r#"billing."Order line""#));
        c.arrow();
        // A quoted plain name is the plain name.
        assert_eq!(c.path().as_deref(), Some("Plain.x.y"));
        let mut c = Cursor::new(r#""a.b" end."#);
        assert_eq!(c.segment().as_deref(), Some("a.b"));
        assert_eq!(c.path().as_deref(), Some("end"));
        assert_eq!(c.rest(), ".");
        assert_eq!(Cursor::new("\"\" x").segment(), None);
        assert_eq!(Cursor::new("\" \" x").segment().as_deref(), Some(" "));
    }

    #[test]
    fn aggregation_arrow_does_not_eat_names() {
        let mut c = Cursor::new("A -- order");
        c.path();
        assert_eq!(c.arrow(), Some((RelationKind::Association, false)));
        assert_eq!(c.path().as_deref(), Some("order"));
        let mut c = Cursor::new("A --o B");
        c.path();
        assert_eq!(c.arrow(), Some((RelationKind::Aggregation, true)));
    }

    #[test]
    fn quote_round_trip() {
        let s = "a \"b\"\\c\nd";
        assert_eq!(Cursor::new(&quote(s)).string().as_deref(), Some(s));
    }
}
