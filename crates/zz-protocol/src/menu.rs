use unicode_width::UnicodeWidthChar;

/// Cells a menu box adds around its widest row: two border columns and one
/// padding column on each side.
pub const MENU_ROW_MARGIN: u16 = 4;

/// Cells the parenthesised action-key annotation adds to a row: the separating
/// space and the two brackets.
const MENU_ANNOTATION_MARGIN: usize = 3;

/// One laid-out menu row: the drawn name and the action-key annotation the row
/// has room to show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuRowLayout {
    pub name: String,
    pub annotation: Option<String>,
}

enum Piece<'a> {
    Style(&'a str),
    Cell { text: &'a str, width: usize },
    Escaped,
}

fn pieces(value: &str) -> Vec<Piece<'_>> {
    let bytes = value.as_bytes();
    let mut pieces = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'#' {
            let character = value[index..]
                .chars()
                .next()
                .expect("index is on a character boundary");
            let end = index + character.len_utf8();
            pieces.push(Piece::Cell {
                text: &value[index..end],
                width: character.width().unwrap_or_default(),
            });
            index = end;
            continue;
        }
        let start = index;
        while bytes.get(index) == Some(&b'#') {
            index += 1;
        }
        let hashes = index - start;
        if bytes.get(index) != Some(&b'[') {
            for _ in 0..hashes / 2 {
                pieces.push(Piece::Escaped);
            }
            if hashes % 2 == 1 {
                pieces.push(Piece::Cell {
                    text: &value[index - 1..index],
                    width: 1,
                });
            }
            continue;
        }
        for _ in 0..hashes / 2 {
            pieces.push(Piece::Escaped);
        }
        if hashes % 2 == 0 {
            pieces.push(Piece::Cell {
                text: &value[index..=index],
                width: 1,
            });
            index += 1;
            continue;
        }
        let Some(relative_end) = value[index + 1..].find(']') else {
            break;
        };
        let end = index + 1 + relative_end + 1;
        pieces.push(Piece::Style(&value[index - 1..end]));
        index = end;
    }
    pieces
}

/// Visible width of a menu row, skipping `#[...]` style runs the way the pin's
/// `format_width` does.
#[must_use]
pub fn menu_row_width(value: &str) -> usize {
    pieces(value)
        .into_iter()
        .map(|piece| match piece {
            Piece::Style(_) => 0,
            Piece::Cell { width, .. } => width,
            Piece::Escaped => 1,
        })
        .sum()
}

/// Keep the last `limit` visible cells of a menu row, carrying every `#[...]`
/// style run through, the way the pin's `format_trim_right` does.
#[must_use]
pub fn trim_menu_row(value: &str, limit: usize) -> String {
    let total = menu_row_width(value);
    if total <= limit {
        return value.to_owned();
    }
    let skip = total - limit;
    let mut width = 0;
    let mut trimmed = String::with_capacity(value.len());
    for piece in pieces(value) {
        match piece {
            Piece::Style(marker) => trimmed.push_str(marker),
            Piece::Cell { text, width: cell } => {
                if width >= skip {
                    trimmed.push_str(text);
                }
                width += cell;
            }
            Piece::Escaped => {
                if width >= skip {
                    trimmed.push_str("##");
                }
                width += 1;
            }
        }
    }
    trimmed
}

/// Lay one menu row out against the cells a client has room for, following
/// `menu_add_item`: an action key is annotated when its bracketed form fits in
/// a quarter of the room or the whole name still fits beside it, an overlong
/// name is trimmed and marked with `>`, and the row keeps its action key even
/// when the annotation is dropped.
#[must_use]
pub fn layout_menu_row(name: &str, key: Option<&str>, max_width: usize) -> MenuRowLayout {
    let mut room = max_width;
    let mut annotation = None;
    if !name.starts_with('-')
        && let Some(key) = key.filter(|key| !key.is_empty())
    {
        let keylen = key.len() + MENU_ANNOTATION_MARGIN;
        if keylen <= room / 4 {
            room -= keylen;
            annotation = Some(key.to_owned());
        } else if keylen < room && name.len() < room - keylen {
            annotation = Some(key.to_owned());
        }
    }
    let mut suffix = "";
    if name.len() > room {
        room = room.saturating_sub(1);
        suffix = ">";
    }
    let mut trimmed = trim_menu_row(name, room);
    trimmed.push_str(suffix);
    MenuRowLayout {
        name: trimmed,
        annotation,
    }
}

/// Cells one laid-out row occupies, including its annotation.
#[must_use]
pub fn menu_row_cells(name: &str, annotation: Option<&str>) -> usize {
    menu_row_width(name)
        + annotation.map_or(0, |annotation| {
            menu_row_width(annotation) + MENU_ANNOTATION_MARGIN
        })
}
