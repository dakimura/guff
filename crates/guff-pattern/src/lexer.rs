//! Pattern lexer.
//!
//! Port of `honnef.co/go/tools/pattern/lexer.go`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemType {
    Error,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    TypeName,
    Variable,
    At,
    Colon,
    Blank,
    ItemString,
    Eof,
}

#[derive(Debug, Clone)]
pub(crate) struct Item {
    pub typ: ItemType,
    pub val: String,
}

fn lex(input: &str) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    let mut chars = input.char_indices().peekable();

    while let Some((byte_pos, ch)) = chars.next() {
        match ch {
            '(' => items.push(Item {
                typ: ItemType::LeftParen,
                val: "(".into(),
            }),
            ')' => items.push(Item {
                typ: ItemType::RightParen,
                val: ")".into(),
            }),
            '[' => items.push(Item {
                typ: ItemType::LeftBracket,
                val: "[".into(),
            }),
            ']' => items.push(Item {
                typ: ItemType::RightBracket,
                val: "]".into(),
            }),
            '@' => items.push(Item {
                typ: ItemType::At,
                val: "@".into(),
            }),
            ':' => items.push(Item {
                typ: ItemType::Colon,
                val: ":".into(),
            }),
            '_' => items.push(Item {
                typ: ItemType::Blank,
                val: "_".into(),
            }),
            '"' => {
                let mut val = String::from('"');
                while let Some((_, c)) = chars.next() {
                    val.push(c);
                    if c == '"' {
                        break;
                    }
                    if c == '\\' {
                        if let Some((_, esc)) = chars.next() {
                            val.push(esc);
                        }
                    }
                }
                let inner = val
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .unwrap_or(&val)
                    .to_string();
                items.push(Item {
                    typ: ItemType::ItemString,
                    val: inner,
                });
            }
            c if c.is_whitespace() => {}
            c if c.is_ascii_uppercase() => {
                let start = byte_pos;
                let mut end = byte_pos + ch.len_utf8();
                while let Some(&(p, nc)) = chars.peek() {
                    if nc.is_ascii_alphanumeric() {
                        end = p + nc.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                items.push(Item {
                    typ: ItemType::TypeName,
                    val: input[start..end].to_string(),
                });
            }
            c if c.is_ascii_lowercase() || c == '$' => {
                let start = byte_pos;
                let mut end = byte_pos + ch.len_utf8();
                while let Some(&(p, nc)) = chars.peek() {
                    if nc.is_ascii_alphanumeric() || nc == '_' || nc == '$' {
                        end = p + nc.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                let val = &input[start..end];
                let typ = if val == "nil" {
                    ItemType::Variable
                } else {
                    ItemType::Variable
                };
                items.push(Item { typ, val: val.to_string() });
            }
            other => {
                return Err(format!("unexpected character {other:?} at byte {byte_pos}"));
            }
        }
    }
    items.push(Item {
        typ: ItemType::Eof,
        val: String::new(),
    });
    Ok(items)
}

pub(crate) struct Lexer {
    items: Vec<Item>,
    cur: usize,
    last: Option<usize>,
}

impl Lexer {
    pub fn new(input: &str) -> Result<Self, String> {
        Ok(Self {
            items: lex(input)?,
            cur: 0,
            last: None,
        })
    }

    pub fn next(&mut self) -> Item {
        if let Some(idx) = self.last.take() {
            self.cur = idx + 1;
            return self.items[idx].clone();
        }
        let item = self.items[self.cur].clone();
        if self.items[self.cur].typ != ItemType::Eof {
            self.cur += 1;
        }
        item
    }

    pub fn rewind(&mut self) {
        if self.cur > 0 {
            self.last = Some(self.cur - 1);
        }
    }

    pub fn peek(&mut self) -> Item {
        let item = self.next();
        self.rewind();
        item
    }

    pub fn accept(&mut self, typ: ItemType) -> Option<Item> {
        let item = self.next();
        if item.typ == typ {
            Some(item)
        } else {
            self.rewind();
            None
        }
    }

    pub fn unexpected(&self, valid: &str) -> String {
        let item = &self.items[self.cur.saturating_sub(1)];
        let got = match item.typ {
            ItemType::TypeName | ItemType::Variable | ItemType::ItemString => item.val.clone(),
            other => format!("'{other:?}'"),
        };
        format!("expected {valid}, found {got}")
    }
}

pub(crate) use ItemType::*;
