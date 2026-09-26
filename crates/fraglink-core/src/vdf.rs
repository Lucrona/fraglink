//! Minimal parser for Valve's text KeyValues ("VDF") format, enough for
//! `libraryfolders.vdf` and `appmanifest_*.acf`. Read-only, no allocation tricks.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Vdf {
    Str(String),
    Obj(Vec<(String, Vdf)>),
}

impl Vdf {
    /// Case-insensitive child lookup (Valve files are inconsistent about key case).
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Obj(items) => items
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            Vdf::Str(_) => None,
        }
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Vdf::as_str)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            Vdf::Obj(_) => None,
        }
    }

    pub fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Obj(items) => items,
            Vdf::Str(_) => &[],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub msg: String,
    pub offset: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VDF parse error at byte {}: {}", self.offset, self.msg)
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, PartialEq)]
enum Tok {
    Open,
    Close,
    Str(String),
}

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn err(&self, msg: &str) -> ParseError {
        ParseError {
            msg: msg.to_string(),
            offset: self.i,
        }
    }

    fn next(&mut self) -> Result<Option<Tok>, ParseError> {
        loop {
            // whitespace
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.i >= self.s.len() {
                return Ok(None);
            }
            // comments
            if self.s[self.i] == b'/' && self.s.get(self.i + 1) == Some(&b'/') {
                while self.i < self.s.len() && self.s[self.i] != b'\n' {
                    self.i += 1;
                }
                continue;
            }
            let c = self.s[self.i];
            match c {
                b'{' => {
                    self.i += 1;
                    return Ok(Some(Tok::Open));
                }
                b'}' => {
                    self.i += 1;
                    return Ok(Some(Tok::Close));
                }
                b'"' => {
                    self.i += 1;
                    let mut out: Vec<u8> = Vec::new();
                    loop {
                        let Some(&b) = self.s.get(self.i) else {
                            return Err(self.err("unterminated string"));
                        };
                        self.i += 1;
                        match b {
                            b'"' => break,
                            b'\\' => {
                                let Some(&e) = self.s.get(self.i) else {
                                    return Err(self.err("dangling escape"));
                                };
                                self.i += 1;
                                match e {
                                    b'n' => out.push(b'\n'),
                                    b't' => out.push(b'\t'),
                                    b'\\' => out.push(b'\\'),
                                    b'"' => out.push(b'"'),
                                    other => {
                                        out.push(b'\\');
                                        out.push(other);
                                    }
                                }
                            }
                            _ => out.push(b),
                        }
                    }
                    return Ok(Some(Tok::Str(String::from_utf8_lossy(&out).into_owned())));
                }
                b'[' => {
                    // conditional like [$WIN32]: skip it
                    while self.i < self.s.len() && self.s[self.i] != b']' {
                        self.i += 1;
                    }
                    self.i += 1;
                    continue;
                }
                _ => {
                    let start = self.i;
                    while self.i < self.s.len() {
                        let b = self.s[self.i];
                        if b.is_ascii_whitespace() || b == b'{' || b == b'}' || b == b'"' {
                            break;
                        }
                        self.i += 1;
                    }
                    return Ok(Some(Tok::Str(
                        String::from_utf8_lossy(&self.s[start..self.i]).into_owned(),
                    )));
                }
            }
        }
    }
}

fn parse_obj(lx: &mut Lexer<'_>, top: bool) -> Result<Vec<(String, Vdf)>, ParseError> {
    let mut items = Vec::new();
    loop {
        match lx.next()? {
            None if top => return Ok(items),
            None => return Err(lx.err("unexpected end of input (missing '}')")),
            Some(Tok::Close) if !top => return Ok(items),
            Some(Tok::Close) => return Err(lx.err("unexpected '}'")),
            Some(Tok::Open) => return Err(lx.err("expected key, found '{'")),
            Some(Tok::Str(key)) => match lx.next()? {
                Some(Tok::Open) => {
                    let child = parse_obj(lx, false)?;
                    items.push((key, Vdf::Obj(child)));
                }
                Some(Tok::Str(v)) => items.push((key, Vdf::Str(v))),
                Some(Tok::Close) | None => return Err(lx.err("key without value")),
            },
        }
    }
}

/// Parse a whole document. Returns the top-level object.
pub fn parse(text: &str) -> Result<Vdf, ParseError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lx = Lexer {
        s: text.as_bytes(),
        i: 0,
    };
    Ok(Vdf::Obj(parse_obj(&mut lx, true)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_and_escapes() {
        let doc = parse(
            r#"
            // comment
            "Root"
            {
                "path"   "C:\\Program Files (x86)\\Steam"
                "quote"  "say \"hi\""
                "child" { "a" "1" "B" "2" }
                unquoted value
            }"#,
        )
        .unwrap();
        let root = doc.get("root").unwrap();
        assert_eq!(root.get_str("path"), Some(r"C:\Program Files (x86)\Steam"));
        assert_eq!(root.get_str("quote"), Some("say \"hi\""));
        assert_eq!(root.get("child").unwrap().get_str("b"), Some("2"));
        assert_eq!(root.get_str("unquoted"), Some("value"));
    }

    #[test]
    fn rejects_unbalanced() {
        assert!(parse("\"a\" { \"b\" \"c\" ").is_err());
        assert!(parse("\"a\" }").is_err());
    }
}
