#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

pub fn offset(text: &str, position: Position) -> Option<usize> {
    let mut start = 0;
    for _ in 0..position.line {
        start += text.get(start..)?.find('\n')? + 1;
    }
    let line = text
        .get(start..)?
        .split('\n')
        .next()?
        .trim_end_matches('\r');
    let mut units = 0;
    for (index, c) in line.char_indices() {
        if units == position.character {
            return Some(start + index);
        }
        units += c.len_utf16() as u32;
        if units > position.character {
            return None; // Never split a surrogate pair.
        }
    }
    (units == position.character).then_some(start + line.len())
}

pub fn position(text: &str, byte: usize) -> Option<Position> {
    let before = text.get(..byte)?;
    let line = before.bytes().filter(|b| *b == b'\n').count() as u32;
    let start = before.rfind('\n').map_or(0, |index| index + 1);
    Some(Position {
        line,
        character: text[start..byte].encode_utf16().count() as u32,
    })
}

pub fn line_start(text: &str, byte: usize) -> usize {
    text[..byte].rfind('\n').map_or(0, |index| index + 1)
}

pub fn line_end(text: &str, byte: usize) -> usize {
    text[byte..]
        .find('\n')
        .map_or(text.len(), |index| byte + index + 1)
}

pub fn indentation(text: &str, byte: usize) -> &str {
    let tail = &text[line_start(text, byte)..];
    &tail[..tail.len() - tail.trim_start_matches([' ', '\t']).len()]
}

pub fn newline(text: &str) -> &str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

pub fn apply(text: &str, edits: &[Edit]) -> Option<String> {
    let mut edits: Vec<_> = edits.iter().collect();
    edits.sort_by_key(|edit| (edit.start, edit.end));
    let mut previous = 0;
    let mut result = String::new();
    for edit in edits {
        if edit.start < previous || edit.start > edit.end {
            return None;
        }
        result.push_str(text.get(previous..edit.start)?);
        result.push_str(&edit.text);
        previous = edit.end;
    }
    result.push_str(text.get(previous..)?);
    Some(result)
}
