use std::fmt::Display;

use anyhow::{Result, anyhow};
use logos::{Lexer, Logos};

fn parse_function_call<'a>(lex: &mut Lexer<'a, VersionToken<'a>>) -> Option<&'a str> {
    if !lex
        .next()
        .and_then(|t| t.ok())
        .map(|t| t == VersionToken::LParen)?
    {
        return None;
    } // consume and check the '(' token
    let arg1 = match lex.next()?.ok()? {
        VersionToken::Hexadecimal(s) => Some(s),
        _ => None, // if not a function call, return None
    }?;
    if !lex
        .next()
        .and_then(|t| t.ok())
        .map(|t| t == VersionToken::RParen)?
    {
        return None;
    } // consume and check the ')' token

    Some(arg1)
}

#[derive(Logos, Copy, Clone, Debug, PartialEq)]
#[logos(skip r"[ \t\n\f]+")] // ignore whitespace and newlines
enum VersionToken<'source> {
    #[token("=")]
    Eq,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token(">=")]
    GtEq,
    #[token("<=")]
    LtEq,
    #[token(">")]
    Gt,
    #[token("<")]
    Lt,
    #[token("||")]
    Or,
    #[token("&&")]
    And,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[regex(r"sha256sum", parse_function_call)]
    Sha256Sum(&'source str),
    #[regex(r"[a-fA-F0-9]+", priority = 3)]
    Hexadecimal(&'source str),
    #[regex(r"(\d+:)?[0-9][0-9A-Za-z.+\-~]*")]
    VersionNumber(&'source str),
}

impl<'source> Display for VersionToken<'source> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VersionToken::Eq => write!(f, "="),
            VersionToken::EqEq => write!(f, "=="),
            VersionToken::NotEq => write!(f, "!="),
            VersionToken::GtEq => write!(f, ">="),
            VersionToken::LtEq => write!(f, "<="),
            VersionToken::Gt => write!(f, ">"),
            VersionToken::Lt => write!(f, "<"),
            VersionToken::Or => write!(f, "||"),
            VersionToken::And => write!(f, "&&"),
            VersionToken::LParen => write!(f, "("),
            VersionToken::RParen => write!(f, ")"),
            VersionToken::Sha256Sum(hex) => write!(f, "sha256sum({})", hex),
            VersionToken::Hexadecimal(hex) => write!(f, "{}", hex),
            VersionToken::VersionNumber(version) => write!(f, "{}", version),
        }
    }
}

impl<'source> VersionToken<'source> {
    pub fn is_op(&self) -> bool {
        match self {
            VersionToken::Eq
            | VersionToken::EqEq
            | VersionToken::NotEq
            | VersionToken::GtEq
            | VersionToken::LtEq
            | VersionToken::Gt
            | VersionToken::Lt
            | VersionToken::Or
            | VersionToken::And => true,
            _ => false,
        }
    }

    pub fn is_cmp_op(&self) -> bool {
        match self {
            VersionToken::Eq
            | VersionToken::EqEq
            | VersionToken::NotEq
            | VersionToken::GtEq
            | VersionToken::LtEq
            | VersionToken::Gt
            | VersionToken::Lt => true,
            _ => false,
        }
    }

    pub fn precedence(&self) -> u8 {
        match self {
            VersionToken::Eq
            | VersionToken::GtEq
            | VersionToken::LtEq
            | VersionToken::Gt
            | VersionToken::Lt
            | VersionToken::Sha256Sum(_)
            | VersionToken::NotEq => 10,
            VersionToken::Or | VersionToken::And => 1,
            _ => 0, // invalid operator
        }
    }
}

const ZERO_STRING: &'static str = "0";
const VERSION_PLACEHOLDER: &'static str = "$VER";
const VERSION_PLACEHOLDER_TOKEN: VersionToken = VersionToken::VersionNumber(VERSION_PLACEHOLDER);

#[derive(PartialEq)]
struct DebVersion<'a> {
    epoch: u32,
    version: &'a [u8],
    release: &'a [u8],
}

impl<'a> DebVersion<'a> {
    fn parse(input: &str) -> Option<DebVersion> {
        let input_bytes = input.as_bytes();
        let mut first_colon = 0usize;
        let mut last_dash = input_bytes.len();

        for (i, byte) in input_bytes.iter().enumerate() {
            match byte {
                b':' => {
                    if first_colon < 1 {
                        first_colon = i + 1;
                    }
                }
                b'-' => {
                    last_dash = i;
                }
                _ => (),
            }
        }

        let epoch = if first_colon > 0 {
            u32::from_str_radix(
                unsafe { str::from_utf8_unchecked(&input_bytes[0..first_colon - 1]) },
                10,
            )
            .ok()?
        } else {
            0
        };
        let version = &input_bytes[first_colon..last_dash];
        let release_idx = if last_dash == input_bytes.len() {
            last_dash
        } else {
            last_dash + 1
        };
        let release = &input_bytes[release_idx..];

        Some(DebVersion {
            epoch,
            version,
            release,
        })
    }
}

fn get_version_sort_priority(c: u8) -> i16 {
    if c.is_ascii_digit() {
        return 0;
    }
    if c.is_ascii_alphabetic() {
        return c.into();
    }
    if c == b'~' {
        return -1;
    }

    (c as i16) + 0x100
}

fn version_string_cmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    let mut a_cursor = 0usize;
    let mut b_cursor = 0usize;
    let a_len = a.len();
    let b_len = b.len();

    while a_cursor <= a_len || b_cursor <= b_len {
        let mut first_diff = std::cmp::Ordering::Equal;
        while (a_cursor < a_len && !a[a_cursor].is_ascii_digit())
            || (b_cursor < b_len && !b[b_cursor].is_ascii_digit())
        {
            let ac = get_version_sort_priority(a[a_cursor]);
            let bc = get_version_sort_priority(b[b_cursor]);

            if ac != bc {
                return ac.cmp(&bc);
            }

            a_cursor += 1;
            b_cursor += 1;
        }

        while a[a_cursor] == b'0' {
            a_cursor += 1;
        }

        while b[b_cursor] == b'0' {
            b_cursor += 1;
        }

        while a[a_cursor].is_ascii_digit() && b[b_cursor].is_ascii_digit() {
            if first_diff == std::cmp::Ordering::Equal {
                first_diff = a[a_cursor].cmp(&b[b_cursor]);
            }

            a_cursor += 1;
            b_cursor += 1;
        }

        if a[a_cursor].is_ascii_digit() {
            return std::cmp::Ordering::Greater;
        }
        if b[b_cursor].is_ascii_digit() {
            return std::cmp::Ordering::Less;
        }
        if first_diff != std::cmp::Ordering::Equal {
            return first_diff;
        }
    }

    std::cmp::Ordering::Equal
}

impl PartialOrd for DebVersion<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let epoch_cmp = self.epoch.cmp(&other.epoch);
        if epoch_cmp != std::cmp::Ordering::Equal {
            return Some(epoch_cmp);
        }

        let version_cmp = version_string_cmp(self.version, other.version);
        if version_cmp != std::cmp::Ordering::Equal {
            return Some(version_cmp);
        }

        let release_cmp = version_string_cmp(self.release, other.release);
        if release_cmp != std::cmp::Ordering::Equal {
            return Some(release_cmp);
        }

        Some(std::cmp::Ordering::Equal)
    }
}

fn parse_version_expr(input: &str) -> Result<Vec<VersionToken>> {
    let mut lexer = VersionToken::lexer(input);
    let mut stack: Vec<VersionToken> = Vec::with_capacity(8);
    let mut operators: Vec<VersionToken> = Vec::with_capacity(8);
    let mut prev_is_op = false;

    // convert infix notation to RPN
    while let Some(maybe_token) = lexer.next() {
        let token = maybe_token
            .map_err(|_| anyhow!("Invalid version expression at position {:?}", lexer.span()))?;
        if token.is_cmp_op() {
            // since we use a very simplified expression format, we don't have a LHS in our "binary expression"
            // we will push a dummy VERSION_PLACEHOLDER_TOKEN to the stack, and later replace it with the actual version
            stack.push(VERSION_PLACEHOLDER_TOKEN);
        }

        match token {
            VersionToken::Eq
            | VersionToken::EqEq
            | VersionToken::NotEq
            | VersionToken::GtEq
            | VersionToken::LtEq
            | VersionToken::Gt
            | VersionToken::Lt
            | VersionToken::Or
            | VersionToken::And => {
                if let Some(last_op) = operators.last() {
                    if last_op.precedence() >= token.precedence() {
                        let last = operators.pop().unwrap();
                        stack.push(last);
                        operators.push(token);
                        prev_is_op = token.is_op();
                        continue;
                    }
                }
                operators.push(token);
            }
            VersionToken::LParen => operators.push(token),
            VersionToken::RParen => {
                // drain all operators and push them back to the output stack
                while let Some(op) = operators.pop() {
                    if op == VersionToken::LParen {
                        break;
                    }
                    stack.push(op);
                }
            }
            VersionToken::Hexadecimal(_) => {
                return Err(anyhow!(
                    "Invalid version expression at position {:?}",
                    lexer.span()
                ));
            }
            VersionToken::Sha256Sum(_) | VersionToken::VersionNumber(_) => {
                if !prev_is_op {
                    return Err(anyhow!(
                        "Unexpected string '{}' at position {:?}",
                        token,
                        lexer.span()
                    ));
                }
                stack.push(token);
            }
        }

        prev_is_op = token.is_op();
    }

    // drain all remaining operators and add them to the output stack
    while let Some(op) = operators.pop() {
        if op == VersionToken::LParen {
            return Err(anyhow!("Unmatched '(' at position {:?}", lexer.span()));
        }
        stack.push(op);
    }

    Ok(stack)
}

pub fn check_version_compatibility(
    required_version_expr: &str,
    version_to_check: &str,
) -> Result<bool> {
    todo!()
}

#[test]
fn test_lexer() {
    let input = "1.2.3+4-5";
    let mut lexer = VersionToken::lexer(input);
    let token = lexer.next().unwrap();
    assert_eq!(token, Ok(VersionToken::VersionNumber(input)));
    assert_eq!(lexer.slice(), "1.2.3+4-5");

    let input = "sha256sum(012345abc)";
    let lexer = VersionToken::lexer(input);
    let token = lexer.map(|t| t.unwrap()).collect::<Vec<_>>();
    assert_eq!(token, vec![VersionToken::Sha256Sum("012345abc")]);
}

#[test]
fn test_parser_simple() {
    let input_expr = "(=1.2.3 || =4.5.6) && <7.8.9 && sha256sum(012345abc)";
    let tokens = parse_version_expr(input_expr).unwrap();
    assert_eq!(
        tokens,
        vec![
            VERSION_PLACEHOLDER_TOKEN,
            VersionToken::VersionNumber("1.2.3"),
            VersionToken::Eq,
            VERSION_PLACEHOLDER_TOKEN,
            VersionToken::VersionNumber("4.5.6"),
            VersionToken::Eq,
            VersionToken::Or,
            VERSION_PLACEHOLDER_TOKEN,
            VersionToken::VersionNumber("7.8.9"),
            VersionToken::Lt,
            VersionToken::Sha256Sum("012345abc"),
            VersionToken::And,
            VersionToken::And,
        ]
    );
}

#[test]
fn test_deb_parsing() {
    let input = "1:1.2.3+4-5";
    let deb_version = DebVersion::parse(input).unwrap();
    assert_eq!(deb_version.epoch, 1);
    assert_eq!(deb_version.version, b"1.2.3+4");
    assert_eq!(deb_version.release, b"5");

    let input = "2:1.2.3-4";
    let deb_version = DebVersion::parse(input).unwrap();
    assert_eq!(deb_version.epoch, 2);
    assert_eq!(deb_version.version, b"1.2.3");
    assert_eq!(deb_version.release, b"4");

    let input = "1:1.2.3";
    let deb_version = DebVersion::parse(input).unwrap();
    assert_eq!(deb_version.epoch, 1);
    assert_eq!(deb_version.version, b"1.2.3");
    assert_eq!(deb_version.release, b"");

    let input = "1";
    let deb_version = DebVersion::parse(input).unwrap();
    assert_eq!(deb_version.epoch, 0);
    assert_eq!(deb_version.version, b"1");
    assert_eq!(deb_version.release, b"");
}


#[test]
fn test_version_cmp() {
    let a = DebVersion::parse("1.2.3-4").unwrap();
    let b = DebVersion::parse("1.2.3+4").unwrap();
    assert!(a < b);

    // let a = "1.2.3+4";
    // let b = "1.2.3-4";
    // assert!(version_cmp(a, b) == std::cmp::Ordering::Greater);

    // let a = "1.2.3-4";
    // let b = "1.2.3-4";
    // assert!(version_cmp(a, b) == std::cmp::Ordering::Equal);

    // let a = "1.2.3-4";
    // let b = "1.2.3";
    // assert!(version_cmp(a, b) == std::cmp::Ordering::Less);
}