use crate::core::packets::{Direction, Packet};

#[derive(Debug, Clone)]
pub enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Comparison(String, Op, Value),
    Bare(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
}

#[derive(Debug, Clone)]
pub enum Value {
    Number(u64),
    Str(String),
    Hex(Vec<u8>),
}

#[derive(Debug)]
pub struct FilterError {
    pub message: String,
}

fn tokenize(input: &str) -> Result<Vec<String>, FilterError> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        if c == '(' || c == ')' {
            tokens.push(c.to_string());
            chars.next();
            continue;
        }
        if c == '&' {
            chars.next();
            if chars.peek() == Some(&'&') {
                chars.next();
                tokens.push("&&".into());
            } else {
                return Err(FilterError {
                    message: "expected '&&'".into(),
                });
            }
            continue;
        }
        if c == '|' {
            chars.next();
            if chars.peek() == Some(&'|') {
                chars.next();
                tokens.push("||".into());
            } else {
                return Err(FilterError {
                    message: "expected '||'".into(),
                });
            }
            continue;
        }
        if c == '!' {
            chars.next();
            if chars.peek() == Some(&'=') {
                chars.next();
                tokens.push("!=".into());
            } else {
                tokens.push("!".into());
            }
            continue;
        }
        if c == '=' {
            chars.next();
            if chars.peek() == Some(&'=') {
                chars.next();
            }
            tokens.push("==".into());
            continue;
        }
        if c == '<' {
            chars.next();
            if chars.peek() == Some(&'=') {
                chars.next();
                tokens.push("<=".into());
            } else {
                tokens.push("<".into());
            }
            continue;
        }
        if c == '>' {
            chars.next();
            if chars.peek() == Some(&'=') {
                chars.next();
                tokens.push(">=".into());
            } else {
                tokens.push(">".into());
            }
            continue;
        }
        if c == '"' || c == '\'' {
            let quote = c;
            chars.next();
            let mut s = String::new();
            while let Some(&cc) = chars.peek() {
                if cc == quote {
                    chars.next();
                    break;
                }
                if cc == '\\' {
                    chars.next();
                    if let Some(&esc) = chars.peek() {
                        s.push(esc);
                        chars.next();
                    }
                    continue;
                }
                s.push(cc);
                chars.next();
            }
            tokens.push(format!("\x00{}", s));
            continue;
        }
        let mut word = String::new();
        while let Some(&cc) = chars.peek() {
            if cc.is_whitespace() || "()&|!=<>".contains(cc) {
                break;
            }
            word.push(cc);
            chars.next();
        }
        if word.is_empty() {
            return Err(FilterError {
                message: format!("unexpected character '{}'", c),
            });
        }
        tokens.push(word);
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<String>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<String>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.pos).map(|s| s.as_str())
    }

    fn next(&mut self) -> Option<String> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn parse_or(&mut self) -> Result<Expr, FilterError> {
        let mut left = self.parse_and()?;
        while self.peek() == Some("||") {
            self.next();
            let right = self.parse_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, FilterError> {
        let mut left = self.parse_unary()?;
        while self.peek() == Some("&&") {
            self.next();
            let right = self.parse_unary()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, FilterError> {
        if self.peek() == Some("!") {
            self.next();
            let inner = self.parse_unary()?;
            return Ok(Expr::Not(Box::new(inner)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, FilterError> {
        if self.peek() == Some("(") {
            self.next();
            let inner = self.parse_or()?;
            if self.peek() != Some(")") {
                return Err(FilterError {
                    message: "expected ')'".into(),
                });
            }
            self.next();
            return Ok(inner);
        }

        let token = self.next().ok_or_else(|| FilterError {
            message: "unexpected end of filter".into(),
        })?;

        if let Some(stripped) = token.strip_prefix('\x00') {
            return Ok(Expr::Bare(stripped.to_string()));
        }

        let known_fields = [
            "dir", "direction", "len", "size", "op", "opcode", "hex", "text", "id", "pid",
        ];

        if known_fields.contains(&token.as_str()) {
            let op_token = self.next().ok_or_else(|| FilterError {
                message: format!("expected operator after '{}'", token),
            })?;

            let op = match op_token.as_str() {
                "==" | "=" => Op::Eq,
                "!=" => Op::Ne,
                "<" => Op::Lt,
                "<=" => Op::Le,
                ">" => Op::Gt,
                ">=" => Op::Ge,
                "contains" => Op::Contains,
                other => {
                    return Err(FilterError {
                        message: format!("unknown operator '{}'", other),
                    })
                }
            };

            let value_token = self.next().ok_or_else(|| FilterError {
                message: "expected value after operator".into(),
            })?;

            let value = parse_value(&token, &value_token)?;
            return Ok(Expr::Comparison(token, op, value));
        }

        Ok(Expr::Bare(token))
    }
}

fn parse_value(field: &str, token: &str) -> Result<Value, FilterError> {
    if let Some(stripped) = token.strip_prefix('\x00') {
        return Ok(Value::Str(stripped.to_string()));
    }

    match field {
        "dir" | "direction" => Ok(Value::Str(token.to_string())),
        "len" | "size" | "id" | "pid" => token
            .parse::<u64>()
            .map(Value::Number)
            .map_err(|_| FilterError {
                message: format!("'{}' expects a number, got '{}'", field, token),
            }),
        "op" | "opcode" => {
            let cleaned = token.trim_start_matches("0x").trim_start_matches("0X");
            u64::from_str_radix(cleaned, 16)
                .map(Value::Number)
                .map_err(|_| FilterError {
                    message: format!("'{}' expects a hex opcode, got '{}'", field, token),
                })
        }
        "hex" => {
            let bytes = parse_hex_filter(token);
            if bytes.is_empty() {
                return Err(FilterError {
                    message: format!("'{}' expects hex bytes, got '{}'", field, token),
                });
            }
            Ok(Value::Hex(bytes))
        }
        "text" => Ok(Value::Str(token.to_string())),
        _ => Ok(Value::Str(token.to_string())),
    }
}

pub fn parse_filter(input: &str) -> Result<Option<Expr>, FilterError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let tokens = tokenize(trimmed)?;
    if tokens.is_empty() {
        return Ok(None);
    }
    let mut parser = Parser::new(tokens);
    let expr = parser.parse_or()?;
    if parser.pos != parser.tokens.len() {
        return Err(FilterError {
            message: format!(
                "unexpected trailing token '{}'",
                parser.tokens[parser.pos]
            ),
        });
    }
    Ok(Some(expr))
}

pub fn eval_expr(expr: &Expr, p: &Packet) -> bool {
    match expr {
        Expr::And(a, b) => eval_expr(a, p) && eval_expr(b, p),
        Expr::Or(a, b) => eval_expr(a, p) || eval_expr(b, p),
        Expr::Not(a) => !eval_expr(a, p),
        Expr::Bare(token) => {
            let lower = token.to_lowercase();
            let hex = p
                .data
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();
            if hex.contains(&lower) {
                return true;
            }
            if let Some(op) = p.opcode {
                if format!("{:04x}", op).contains(&lower) {
                    return true;
                }
            }
            let text: String = p
                .data
                .iter()
                .map(|b| {
                    if b.is_ascii_graphic() || *b == b' ' {
                        *b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            if text.to_lowercase().contains(&lower) {
                return true;
            }
            let bytes = parse_hex_filter(token);
            if !bytes.is_empty() && contains_subsequence(&p.data, &bytes) {
                return true;
            }
            false
        }
        Expr::Comparison(field, op, value) => {
            let field = field.as_str();
            match field {
                "dir" | "direction" => {
                    let dir_str = match p.direction {
                        Direction::Sent => "sent",
                        Direction::Received => "received",
                    };
                    let v = match value {
                        Value::Str(s) => s.to_lowercase(),
                        _ => return false,
                    };
                    match op {
                        Op::Eq => dir_str == v,
                        Op::Ne => dir_str != v,
                        _ => false,
                    }
                }
                "len" | "size" => {
                    let n = match value {
                        Value::Number(n) => *n,
                        _ => return false,
                    };
                    cmp_num(p.size as u64, n, *op)
                }
                "id" => {
                    let n = match value {
                        Value::Number(n) => *n,
                        _ => return false,
                    };
                    cmp_num(p.id as u64, n, *op)
                }
                "pid" => {
                    let n = match value {
                        Value::Number(n) => *n,
                        _ => return false,
                    };
                    cmp_num(p.source_pid as u64, n, *op)
                }
                "op" | "opcode" => {
                    let n = match value {
                        Value::Number(n) => *n,
                        _ => return false,
                    };
                    let actual = p.opcode.unwrap_or(0) as u64;
                    cmp_num(actual, n, *op)
                }
                "hex" => {
                    let bytes = match value {
                        Value::Hex(b) => b,
                        _ => return false,
                    };
                    match op {
                        Op::Contains | Op::Eq => contains_subsequence(&p.data, bytes),
                        Op::Ne => !contains_subsequence(&p.data, bytes),
                        _ => false,
                    }
                }
                "text" => {
                    let s = match value {
                        Value::Str(s) => s.to_lowercase(),
                        _ => return false,
                    };
                    let text: String = p
                        .data
                        .iter()
                        .map(|b| {
                            if b.is_ascii_graphic() || *b == b' ' {
                                *b as char
                            } else {
                                '.'
                            }
                        })
                        .collect();
                    let text_lower = text.to_lowercase();
                    match op {
                        Op::Eq => text_lower == s,
                        Op::Ne => text_lower != s,
                        Op::Contains => text_lower.contains(&s),
                        _ => false,
                    }
                }
                _ => false,
            }
        }
    }
}

fn cmp_num(actual: u64, expected: u64, op: Op) -> bool {
    match op {
        Op::Eq => actual == expected,
        Op::Ne => actual != expected,
        Op::Lt => actual < expected,
        Op::Le => actual <= expected,
        Op::Gt => actual > expected,
        Op::Ge => actual >= expected,
        Op::Contains => false,
    }
}

pub fn parse_hex_filter(s: &str) -> Vec<u8> {
    let cleaned: String = s
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':')
        .collect();
    if cleaned.is_empty() || cleaned.len() % 2 != 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let bytes = cleaned.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16);
        let lo = (bytes[i + 1] as char).to_digit(16);
        match (hi, lo) {
            (Some(h), Some(l)) => out.push(((h << 4) | l) as u8),
            _ => return Vec::new(),
        }
        i += 2;
    }
    out
}

pub fn contains_subsequence(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}