const ERROR_CONTEXT: usize = 8;
const PARSE_DEPTH_MAX: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Json<'a> {
    String(&'a str),
    Number(i64),
    Boolean(bool),
    Object(Vec<(&'a str, Json<'a>)>),
    Array(Vec<Json<'a>>),
}

impl<'a> Json<'a> {
    pub(super) fn find(&self, key: &str) -> Option<&Json<'a>> {
        let Self::Object(fields) = self else {
            return None;
        };
        fields
            .iter()
            .find_map(|(name, value)| (*name == key).then_some(value))
    }

    fn field(&self, key: &str, expected: &str) -> Result<&Json<'a>, String> {
        let value = self
            .find(key)
            .ok_or_else(|| format!("key \"{key}\" not found"))?;
        if value.kind() == expected {
            Ok(value)
        } else {
            Err(format!("key \"{key}\" expected {expected}"))
        }
    }

    const fn kind(&self) -> &'static str {
        match self {
            Self::String(_) => "a string",
            Self::Number(_) => "a number",
            Self::Boolean(_) => "a boolean",
            Self::Object(_) => "an object",
            Self::Array(_) => "an array",
        }
    }

    pub(super) fn string(&self, key: &str) -> Result<&'a str, String> {
        match self.field(key, "a string")? {
            Self::String(value) => Ok(value),
            _ => unreachable!(),
        }
    }

    pub(super) fn number(&self, key: &str) -> Result<i64, String> {
        match self.field(key, "a number")? {
            Self::Number(value) => Ok(*value),
            _ => unreachable!(),
        }
    }

    pub(super) fn boolean(&self, key: &str) -> Result<bool, String> {
        match self.field(key, "a boolean")? {
            Self::Boolean(value) => Ok(*value),
            _ => unreachable!(),
        }
    }

    pub(super) fn object(&self, key: &str) -> Result<&Json<'a>, String> {
        self.field(key, "an object")
    }

    pub(super) fn array(&self, key: &str) -> Result<&[Json<'a>], String> {
        match self.field(key, "an array")? {
            Self::Array(members) => Ok(members),
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    OpenObject,
    CloseObject,
    OpenArray,
    CloseArray,
    Comma,
    Colon,
    Quote,
    Value,
    Eof,
}

#[derive(Clone, Copy, Debug)]
struct Token {
    kind: Kind,
    offset: usize,
    len: usize,
}

pub(super) fn parse(input: &str) -> Result<Json<'_>, String> {
    parse_within(input, PARSE_DEPTH_MAX)
}

pub(super) fn parse_within(input: &str, depth_max: usize) -> Result<Json<'_>, String> {
    let input = input.split('\0').next().unwrap_or_default();
    if input.is_empty() {
        return Err("empty input".to_owned());
    }
    let tokens = tokenize(input)?;
    let mut parser = Parser {
        input,
        tokens: &tokens,
        cursor: 0,
        depth: 0,
        depth_max,
    };
    if parser.peek().kind != Kind::OpenObject {
        return Err(parser.error("expected object", parser.peek().offset));
    }
    let root = parser.object()?;
    if parser.peek().kind != Kind::Eof {
        return Err(parser.error("unexpected trailing data", parser.peek().offset));
    }
    Ok(root)
}

fn error(input: &str, reason: &str, offset: usize) -> String {
    let rest = &input.as_bytes()[offset.min(input.len())..];
    if rest.is_empty() {
        return reason.to_owned();
    }
    let shown = &rest[..rest.len().min(ERROR_CONTEXT)];
    let ellipsis = if rest.len() > ERROR_CONTEXT {
        "..."
    } else {
        ""
    };
    format!("{reason}: {}{ellipsis}", String::from_utf8_lossy(shown))
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let bytes = input.as_bytes();
    let mut tokens: Vec<Token> = Vec::new();
    let mut in_string = false;
    let mut cursor = 0;
    let mut location = 0;
    while cursor < bytes.len() {
        location = cursor;
        let kind = if in_string && bytes[cursor] != b'"' {
            Kind::Value
        } else {
            match bytes[cursor] {
                b' ' | b'\t' | b'\n' | b'\r' => {
                    cursor += 1;
                    continue;
                }
                b'{' => Kind::OpenObject,
                b'}' => Kind::CloseObject,
                b'[' => Kind::OpenArray,
                b']' => Kind::CloseArray,
                b'"' => Kind::Quote,
                b':' => Kind::Colon,
                b',' => Kind::Comma,
                _ => Kind::Value,
            }
        };
        let mut len = 1;
        if kind == Kind::Value {
            len = tokenize_value(tokens.last().map(|token| token.kind), &bytes[location..])
                .ok_or_else(|| error(input, "tokenization error", location))?;
            cursor += len - 1;
        }
        tokens.push(Token {
            kind,
            offset: location,
            len,
        });
        if kind == Kind::Quote {
            in_string = !in_string;
        }
        cursor += 1;
    }
    tokens.push(Token {
        kind: Kind::Eof,
        offset: location,
        len: 0,
    });
    Ok(tokens)
}

fn tokenize_value(previous: Option<Kind>, value: &[u8]) -> Option<usize> {
    let at = |index: usize| value.get(index).copied().unwrap_or(0);
    let mut scan = 0;
    match previous? {
        Kind::Quote => {
            while at(scan) != b'"' {
                if at(scan) < 0x20 {
                    return None;
                }
                if at(scan) != b'\\' {
                    scan += 1;
                    continue;
                }
                scan += 1;
                match at(scan) {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => scan += 1,
                    b'u' => {
                        if !(1..=4).all(|offset| at(scan + offset).is_ascii_hexdigit()) {
                            return None;
                        }
                        scan += 5;
                    }
                    _ => return None,
                }
            }
        }
        Kind::Colon => loop {
            if at(scan) == 0 {
                return None;
            }
            scan += 1;
            if matches!(
                at(scan),
                b']' | b'}' | b',' | b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'
            ) {
                break;
            }
        },
        _ => return None,
    }
    Some(scan)
}

struct Parser<'a, 't> {
    input: &'a str,
    tokens: &'t [Token],
    cursor: usize,
    depth: usize,
    depth_max: usize,
}

impl<'a> Parser<'a, '_> {
    fn peek(&self) -> Token {
        self.tokens[self.cursor.min(self.tokens.len() - 1)]
    }

    fn kind_at(&self, offset: usize) -> Kind {
        self.tokens
            .get(self.cursor + offset)
            .map_or(Kind::Eof, |token| token.kind)
    }

    fn text(&self, token: Token) -> &'a str {
        &self.input[token.offset..token.offset + token.len]
    }

    fn error(&self, reason: &str, offset: usize) -> String {
        error(self.input, reason, offset)
    }

    fn key(&mut self) -> Result<&'a str, String> {
        let start = self.peek().offset;
        let fail = |parser: &Self| parser.error("invalid key", start);
        if self.peek().kind != Kind::Quote {
            return Err(fail(self));
        }
        self.cursor += 1;
        let value = self.peek();
        if value.kind != Kind::Value {
            return Err(fail(self));
        }
        self.cursor += 1;
        if self.peek().kind != Kind::Quote {
            return Err(fail(self));
        }
        self.cursor += 1;
        Ok(self.text(value))
    }

    fn object(&mut self) -> Result<Json<'a>, String> {
        self.depth += 1;
        if self.depth > self.depth_max {
            return Err(self.error("parse depth exceeded", self.peek().offset));
        }
        self.cursor += 1;
        let mut fields: Vec<(&'a str, Json<'a>)> = Vec::new();
        while self.peek().kind != Kind::CloseObject {
            let key = self.key()?;
            if fields.iter().any(|(name, _)| *name == key) {
                return Err(self.error("duplicate key", self.peek().offset));
            }
            if self.peek().kind != Kind::Colon {
                return Err(self.error("missing colon", self.peek().offset));
            }
            self.cursor += 1;
            let value = match self.peek().kind {
                Kind::Quote => self.string()?,
                Kind::Value => {
                    let text = self.text(self.peek()).as_bytes();
                    let digit = |index: usize| text.get(index).is_some_and(u8::is_ascii_digit);
                    if digit(0) || (text.first() == Some(&b'-') && digit(1)) {
                        self.number()?
                    } else {
                        self.boolean()?
                    }
                }
                Kind::OpenObject => self.object()?,
                Kind::OpenArray => self.array()?,
                _ => {
                    return Err(
                        self.error("unexpected value when parsing object", self.peek().offset)
                    );
                }
            };
            fields.push((key, value));
            if self.peek().kind == Kind::Comma {
                if self.kind_at(1) == Kind::CloseObject {
                    return Err(self.error("invalid object", self.peek().offset));
                }
                self.cursor += 1;
            } else if self.peek().kind != Kind::CloseObject {
                return Err(self.error("invalid object", self.peek().offset));
            }
        }
        self.cursor += 1;
        self.depth -= 1;
        Ok(Json::Object(fields))
    }

    fn array(&mut self) -> Result<Json<'a>, String> {
        self.cursor += 1;
        let mut members = Vec::new();
        while self.peek().kind != Kind::CloseArray {
            if self.peek().kind != Kind::OpenObject {
                return Err(self.error("invalid array member", self.peek().offset));
            }
            members.push(self.object()?);
            if self.peek().kind == Kind::Comma {
                if self.kind_at(1) == Kind::CloseArray {
                    return Err(self.error("invalid array", self.peek().offset));
                }
                self.cursor += 1;
            } else if self.peek().kind != Kind::CloseArray {
                return Err(self.error("invalid array", self.peek().offset));
            }
        }
        self.cursor += 1;
        Ok(Json::Array(members))
    }

    fn string(&mut self) -> Result<Json<'a>, String> {
        let start = self.peek().offset;
        self.cursor += 1;
        let value = self.peek();
        if value.kind != Kind::Value {
            return Err(self.error("invalid string", start));
        }
        self.cursor += 1;
        if self.peek().kind != Kind::Quote {
            return Err(self.error("invalid string", start));
        }
        self.cursor += 1;
        Ok(Json::String(self.text(value)))
    }

    fn number(&mut self) -> Result<Json<'a>, String> {
        let token = self.peek();
        let text = self.text(token);
        let digits = text.strip_prefix('-').unwrap_or(text);
        let leading_zero = digits.starts_with('0') && digits.len() != 1;
        let parsed = (!leading_zero && digits.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| text.parse::<i64>().ok())
            .flatten();
        let Some(value) = parsed else {
            return Err(self.error("invalid number", token.offset));
        };
        self.cursor += 1;
        Ok(Json::Number(value))
    }

    fn boolean(&mut self) -> Result<Json<'a>, String> {
        let token = self.peek();
        let value = match self.text(token) {
            "true" => true,
            "false" => false,
            _ => return Err(self.error("invalid boolean", token.offset)),
        };
        self.cursor += 1;
        Ok(Json::Boolean(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_quote_eight_bytes_of_context_like_json_c() {
        let cases = [
            ("{", "invalid key: {"),
            ("{}", ""),
            ("{\"V\":2,\"V\":2}", "duplicate key: :2}"),
            ("{\"V\":02}", "invalid number: 02}"),
            ("{\"V\":2x}", "invalid number: 2x}"),
            ("{\"V\":tru}", "invalid boolean: tru}"),
            ("{\"V\" 2}", "tokenization error: 2}"),
            ("{V:2}", "tokenization error: V:2}"),
            ("{\"V\":2,}", "invalid object: ,}"),
            ("{\"V\":[1]}", "tokenization error: 1]}"),
            ("{\"V\":[{},]}", "invalid array: ,]}"),
            ("{\"V\":\"\"}", "invalid string: \"\"}"),
            ("{\"V\":\"a\\qb\"}", "tokenization error: a\\qb\"}"),
            ("{\"V\":{}}}", "unexpected trailing data: }"),
            ("{\"V\":1} x", "tokenization error: x"),
            ("{\"V\":1,\"L\":{\"t\":1}", "invalid object: }"),
            ("{\"abcdefghij\" 1}", "tokenization error: 1}"),
            ("{\"V\":12345678901}", ""),
            (
                "{\"V\":99999999999999999999}",
                "invalid number: 99999999...",
            ),
            ("{\"V\":-0}", ""),
            ("{\"V\":-01}", "invalid number: -01}"),
            ("{\"V\":true,\"W\":false}", ""),
            ("{\"V\":[]}", ""),
            ("{\"V\":[{}]", "invalid object: ]"),
        ];
        for (input, expected) in cases {
            let result = parse(input).err().unwrap_or_default();
            assert_eq!(result, expected, "{input}");
        }
    }

    #[test]
    fn depth_is_counted_in_objects_only() {
        let nested = |depth: usize| {
            let mut input = String::new();
            for _ in 1..depth {
                input.push_str("{\"a\":");
            }
            input.push_str("{}");
            for _ in 1..depth {
                input.push('}');
            }
            input
        };
        assert!(parse(&nested(200)).is_ok());
        assert_eq!(
            parse(&nested(201)).unwrap_err(),
            "parse depth exceeded: {}}}}}}}..."
        );
    }

    #[test]
    fn lookups_report_missing_and_mistyped_keys() {
        let root = parse("{\"V\":\"2\",\"L\":[],\"a\":1}").unwrap();
        assert_eq!(root.number("V").unwrap_err(), "key \"V\" expected a number");
        assert_eq!(
            root.object("L").unwrap_err(),
            "key \"L\" expected an object"
        );
        assert_eq!(
            root.boolean("a").unwrap_err(),
            "key \"a\" expected a boolean"
        );
        assert_eq!(root.string("t").unwrap_err(), "key \"t\" not found");
        assert_eq!(root.array("L").unwrap(), &[]);
        assert_eq!(root.string("V").unwrap(), "2");
    }
}
