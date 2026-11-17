//! Splits Umple source into tokens.
//!
//! Umple is free-form: statements end with `;` or a `{ … }` block, not at
//! line ends, and class bodies may hold methods written in Java, Python and
//! other languages. The lexer therefore only needs to be exact about what
//! Umple itself uses (names, numbers, multiplicities, association arrows,
//! brackets) and safe about the rest: strings and comments are single
//! tokens, so braces inside them never unbalance a method body.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A name, possibly dotted: `Student`, `a.b.c`, `position.association`.
    Ident,
    Number,
    /// A quoted string or character literal, quotes included.
    Str,
    Punct,
    /// A `//` or `/* */` comment; `text` is the comment without its markers.
    Comment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: Kind,
    pub text: &'a str,
    /// Byte range of the whole token in the source.
    pub start: usize,
    pub end: usize,
    /// 1-based line and column (in characters).
    pub line: usize,
    pub col: usize,
}

/// Splits `src` into tokens. Never fails: a character that starts no token
/// becomes a one-character punctuation token.
pub fn lex(src: &str) -> Vec<Token<'_>> {
    let mut lx = Lexer {
        src,
        pos: 0,
        line: 1,
        line_start: 0,
        out: Vec::new(),
    };
    lx.run();
    lx.out
}

/// Multi-character punctuation, longest first.
const PUNCT: [&str; 8] = ["<@>-", "-<@>", "...", "--", "->", "<-", "><", ".."];

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    line: usize,
    line_start: usize,
    out: Vec<Token<'a>>,
}

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

impl<'a> Lexer<'a> {
    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    /// Adds a token covering `start..self.pos`, whose meaningful text is
    /// `text`. `line` and `col` are those of `start`.
    fn push(&mut self, kind: Kind, text: &'a str, start: usize, line: usize, line_start: usize) {
        let col = self.src[line_start..start].chars().count() + 1;
        self.out.push(Token {
            kind,
            text,
            start,
            end: self.pos,
            line,
            col,
        });
    }

    /// Moves to byte `to`, counting the newlines passed.
    fn advance_to(&mut self, to: usize) {
        for (i, c) in self.src[self.pos..to].char_indices() {
            if c == '\n' {
                self.line += 1;
                self.line_start = self.pos + i + 1;
            }
        }
        self.pos = to;
    }

    fn run(&mut self) {
        while let Some(c) = self.rest().chars().next() {
            let (start, line, line_start) = (self.pos, self.line, self.line_start);
            let r = self.rest();
            if c.is_whitespace() {
                self.advance_to(start + c.len_utf8());
            } else if let Some(body) = r.strip_prefix("//") {
                let len = body.find('\n').unwrap_or(body.len());
                self.advance_to(start + 2 + len);
                self.push(Kind::Comment, &body[..len], start, line, line_start);
            } else if let Some(body) = r.strip_prefix("/*") {
                let (len, close) = match body.find("*/") {
                    Some(i) => (i, 2),
                    None => (body.len(), 0),
                };
                self.advance_to(start + 2 + len + close);
                self.push(Kind::Comment, &body[..len], start, line, line_start);
            } else if is_name_start(c) {
                let end = start + name_len(r);
                self.advance_to(end);
                self.push(Kind::Ident, &self.src[start..end], start, line, line_start);
            } else if c.is_ascii_digit() {
                let end = start + number_len(r);
                self.advance_to(end);
                self.push(Kind::Number, &self.src[start..end], start, line, line_start);
            } else if let Some(len) = (c == '"' || c == '\'').then(|| quoted_len(r)).flatten() {
                self.advance_to(start + len);
                self.push(
                    Kind::Str,
                    &self.src[start..start + len],
                    start,
                    line,
                    line_start,
                );
            } else {
                let len = PUNCT
                    .iter()
                    .find(|p| r.starts_with(*p))
                    .map(|p| p.len())
                    .unwrap_or(c.len_utf8());
                self.advance_to(start + len);
                self.push(
                    Kind::Punct,
                    &self.src[start..start + len],
                    start,
                    line,
                    line_start,
                );
            }
        }
    }
}

/// Length of a name with dotted parts: `a.b.c`, but not the dot in `a.*`.
fn name_len(r: &str) -> usize {
    let mut len = 0;
    loop {
        len += r[len..]
            .char_indices()
            .find(|(_, c)| !is_name_char(*c))
            .map(|(i, _)| i)
            .unwrap_or(r.len() - len);
        let after = &r[len..];
        match after.strip_prefix('.').and_then(|a| a.chars().next()) {
            Some(c) if is_name_start(c) => len += 1,
            _ => return len,
        }
    }
}

/// Length of `12`, `1.5`, `3L` or `2.0f`; `0..1` stops before the dots.
fn number_len(r: &str) -> usize {
    let digits = |s: &str| {
        s.char_indices()
            .find(|(_, c)| !c.is_ascii_digit())
            .map(|(i, _)| i)
            .unwrap_or(s.len())
    };
    let mut len = digits(r);
    if r[len..].starts_with('.') && r[len + 1..].starts_with(|c: char| c.is_ascii_digit()) {
        len += 1 + digits(&r[len + 1..]);
    }
    len + r[len..]
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphanumeric())
        .map(|(i, _)| i)
        .unwrap_or(r.len() - len)
}

/// Length of a string or character literal that closes on the same line,
/// with backslash escapes; `None` when it does not close.
fn quoted_len(r: &str) -> Option<usize> {
    let quote = r.chars().next()?;
    let mut escaped = false;
    for (i, c) in r.char_indices().skip(1) {
        match c {
            '\n' => return None,
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            c if c == quote => return Some(i + 1),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str) -> Vec<&str> {
        lex(src)
            .into_iter()
            .filter(|t| t.kind != Kind::Comment)
            .map(|t| t.text)
            .collect()
    }

    #[test]
    fn association_arrows_and_multiplicities() {
        assert_eq!(
            texts("0..1 -- * B; 1 <@>- 1..* C; * -<@> 0..1 D; 1 -> * E r; * <- 1 F; 1 >< 1 G;"),
            [
                "0", "..", "1", "--", "*", "B", ";", "1", "<@>-", "1", "..", "*", "C", ";", "*",
                "-<@>", "0", "..", "1", "D", ";", "1", "->", "*", "E", "r", ";", "*", "<-", "1",
                "F", ";", "1", "><", "1", "G", ";"
            ]
        );
    }

    #[test]
    fn dotted_names_numbers_and_generics() {
        assert_eq!(
            texts("namespace a.b_c; position.association X 1,2; Double d = 1.5; List<String> l;"),
            [
                "namespace",
                "a.b_c",
                ";",
                "position.association",
                "X",
                "1",
                ",",
                "2",
                ";",
                "Double",
                "d",
                "=",
                "1.5",
                ";",
                "List",
                "<",
                "String",
                ">",
                "l",
                ";"
            ]
        );
        // A trailing dot is not part of the name.
        assert_eq!(
            texts("depend java.util.*;"),
            ["depend", "java.util", ".", "*", ";"]
        );
    }

    #[test]
    fn strings_and_char_literals_hide_braces() {
        let toks = lex(r#"s = "a } \" {"; c = '{'; d = "unclosed"#);
        let kinds: Vec<(Kind, &str)> = toks.iter().map(|t| (t.kind, t.text)).collect();
        assert_eq!(
            kinds,
            [
                (Kind::Ident, "s"),
                (Kind::Punct, "="),
                (Kind::Str, r#""a } \" {""#),
                (Kind::Punct, ";"),
                (Kind::Ident, "c"),
                (Kind::Punct, "="),
                (Kind::Str, "'{'"),
                (Kind::Punct, ";"),
                (Kind::Ident, "d"),
                (Kind::Punct, "="),
                // No closing quote on the line: the quote is punctuation.
                (Kind::Punct, "\""),
                (Kind::Ident, "unclosed"),
            ]
        );
    }

    #[test]
    fn comments_with_positions() {
        let src = "// first\nclass A { /* two\n  lines */ x; }";
        let toks = lex(src);
        let c: Vec<&Token> = toks.iter().filter(|t| t.kind == Kind::Comment).collect();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].text, " first");
        assert_eq!((c[0].line, c[0].col), (1, 1));
        assert_eq!(c[1].text, " two\n  lines ");
        assert_eq!((c[1].line, c[1].col), (2, 11));
        let x = toks.iter().find(|t| t.text == "x").unwrap();
        assert_eq!((x.line, x.col), (3, 12));
        assert_eq!(&src[x.start..x.end], "x");
        // An unclosed block comment runs to the end.
        let t = lex("a /* never closed");
        assert_eq!(t.len(), 2);
        assert_eq!(t[1].text, " never closed");
    }

    #[test]
    fn columns_count_characters() {
        let toks = lex("«é» x");
        let x = toks.iter().find(|t| t.text == "x").unwrap();
        assert_eq!((x.line, x.col), (1, 5));
    }
}
