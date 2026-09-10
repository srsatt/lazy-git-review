use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PositionEncoding {
    Utf8,
    Utf16,
    Utf32,
}

impl PositionEncoding {
    pub fn from_lsp(value: &str) -> Self {
        match value {
            "utf-8" => Self::Utf8,
            "utf-32" => Self::Utf32,
            _ => Self::Utf16,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TextRange {
    pub start: Position,
    pub end: Position,
}

pub fn byte_offset(text: &str, position: Position, encoding: PositionEncoding) -> Result<usize> {
    let line_start = if position.line == 0 {
        0
    } else {
        text.match_indices('\n')
            .nth(position.line as usize - 1)
            .map(|(index, _)| index + 1)
            .ok_or_else(|| invalid_position(position, "line is outside document"))?
    };
    let raw_end = text[line_start..]
        .find('\n')
        .map(|i| line_start + i)
        .unwrap_or(text.len());
    let line_end = if raw_end > line_start && text.as_bytes()[raw_end - 1] == b'\r' {
        raw_end - 1
    } else {
        raw_end
    };
    let line = &text[line_start..line_end];
    let target = position.character as usize;
    let relative = match encoding {
        PositionEncoding::Utf8 => {
            if target <= line.len() && line.is_char_boundary(target) {
                Some(target)
            } else {
                None
            }
        }
        PositionEncoding::Utf16 => encoded_offset(line, target, |c| c.len_utf16()),
        PositionEncoding::Utf32 => encoded_offset(line, target, |_| 1),
    }
    .ok_or_else(|| invalid_position(position, "character is outside line or splits a character"))?;
    Ok(line_start + relative)
}

fn encoded_offset(text: &str, target: usize, width: impl Fn(char) -> usize) -> Option<usize> {
    let mut units = 0;
    if target == 0 {
        return Some(0);
    }
    for (offset, character) in text.char_indices() {
        units += width(character);
        if units == target {
            return Some(offset + character.len_utf8());
        }
        if units > target {
            return None;
        }
    }
    (units == target).then_some(text.len())
}

fn invalid_position(position: Position, reason: &str) -> AppError {
    AppError::InvalidInput {
        code: "invalid_lsp_position",
        message: format!("{}:{}: {reason}", position.line, position.character),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_utf8_utf16_utf32_and_crlf() {
        let text = "a😀b\r\nnext";
        assert_eq!(
            byte_offset(
                text,
                Position {
                    line: 0,
                    character: 5
                },
                PositionEncoding::Utf8
            )
            .unwrap(),
            5
        );
        assert_eq!(
            byte_offset(
                text,
                Position {
                    line: 0,
                    character: 3
                },
                PositionEncoding::Utf16
            )
            .unwrap(),
            5
        );
        assert_eq!(
            byte_offset(
                text,
                Position {
                    line: 0,
                    character: 2
                },
                PositionEncoding::Utf32
            )
            .unwrap(),
            5
        );
        assert_eq!(
            byte_offset(
                text,
                Position {
                    line: 1,
                    character: 0
                },
                PositionEncoding::Utf16
            )
            .unwrap(),
            8
        );
    }

    #[test]
    fn rejects_split_surrogate_and_accepts_empty_range() {
        let text = "😀";
        assert!(
            byte_offset(
                text,
                Position {
                    line: 0,
                    character: 1
                },
                PositionEncoding::Utf16
            )
            .is_err()
        );
        let at_end = byte_offset(
            text,
            Position {
                line: 0,
                character: 2,
            },
            PositionEncoding::Utf16,
        )
        .unwrap();
        assert_eq!(at_end, text.len());
    }
}
