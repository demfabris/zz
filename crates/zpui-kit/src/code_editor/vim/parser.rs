use super::{
    VimMode,
    motion::{FindChar, Motion},
    text_object::TextObject,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operator {
    Delete,
    Change,
    Yank,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OperatorTarget {
    Motion(Motion),
    Object(TextObject),
    Line,
    Selection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InsertAt {
    Cursor,
    After,
    LineStart,
    LineEnd,
    OpenBelow,
    OpenAbove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VisualKind {
    Char,
    Line,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScrollAlign {
    Center,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Verb {
    Motion(Motion),
    Operate {
        operator: Operator,
        target: OperatorTarget,
    },
    SelectObject(TextObject),
    Insert(InsertAt),
    DeleteChar {
        before: bool,
    },
    DeleteToLineEnd,
    ChangeToLineEnd,
    YankLine,
    SubstituteChar,
    SubstituteLine,
    Join,
    Replace(char),
    ToggleCase,
    Indent {
        outdent: bool,
    },
    Paste {
        before: bool,
    },
    Undo,
    Redo,
    EnterVisual(VisualKind),
    SwapVisualEnds,
    EnterNormal,
    Scroll(ScrollAlign),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Command {
    pub count: Option<usize>,
    pub verb: Verb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    Char(char),
    Ctrl(char),
    Escape,
    Enter,
    Backspace,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Pending {
    count: Option<u32>,
    operator: Option<Operator>,
    operator_count: Option<u32>,
    stage: Stage,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Stage {
    #[default]
    Start,
    Find {
        till: bool,
        backward: bool,
    },
    Replace,
    Object {
        around: bool,
    },
    Prefix,
    Scroll,
    Indent {
        outdent: bool,
    },
}

impl Pending {
    pub(super) fn is_empty(self) -> bool {
        self == Self::default()
    }

    fn has_count(self) -> bool {
        if self.operator.is_some() {
            self.operator_count.is_some()
        } else {
            self.count.is_some()
        }
    }

    fn push_digit(&mut self, digit: u32) {
        let slot = if self.operator.is_some() {
            &mut self.operator_count
        } else {
            &mut self.count
        };
        *slot = Some(
            slot.unwrap_or(0)
                .saturating_mul(10)
                .saturating_add(digit)
                .max(1),
        );
    }

    fn count(self) -> Option<usize> {
        match (self.count, self.operator_count) {
            (None, None) => None,
            (first, second) => Some(first.unwrap_or(1) as usize * second.unwrap_or(1) as usize),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Pending,
    Cancel,
    Command(Command),
    PassThrough,
}

pub(super) fn step(pending: &mut Pending, mode: VimMode, key: Key) -> Step {
    let step = advance(pending, mode, key);
    if !matches!(step, Step::Pending) {
        *pending = Pending::default();
    }
    step
}

fn advance(pending: &mut Pending, mode: VimMode, key: Key) -> Step {
    match pending.stage {
        Stage::Start => start(pending, mode, key),
        Stage::Find { till, backward } => match key {
            Key::Char(target) => motion(
                *pending,
                Motion::Find(FindChar {
                    target,
                    till,
                    backward,
                }),
            ),
            _ => Step::Cancel,
        },
        Stage::Replace => match key {
            Key::Char(character) => command(*pending, Verb::Replace(character)),
            _ => Step::Cancel,
        },
        Stage::Object { around } => match key {
            Key::Char(character) => TextObject::from_key(character, around)
                .map_or(Step::Cancel, |object| {
                    object_command(*pending, mode, object)
                }),
            _ => Step::Cancel,
        },
        Stage::Prefix => match key {
            Key::Char('g') => motion(*pending, Motion::FirstLine),
            Key::Char('e') => motion(*pending, Motion::WordEndBackward { big: false }),
            Key::Char('E') => motion(*pending, Motion::WordEndBackward { big: true }),
            _ => Step::Cancel,
        },
        Stage::Scroll => match key {
            Key::Char('z') => command(*pending, Verb::Scroll(ScrollAlign::Center)),
            Key::Char('t') => command(*pending, Verb::Scroll(ScrollAlign::Top)),
            Key::Char('b') => command(*pending, Verb::Scroll(ScrollAlign::Bottom)),
            _ => Step::Cancel,
        },
        Stage::Indent { outdent } => match key {
            Key::Char('>') if !outdent => command(*pending, Verb::Indent { outdent }),
            Key::Char('<') if outdent => command(*pending, Verb::Indent { outdent }),
            _ => Step::Cancel,
        },
    }
}

fn command(pending: Pending, verb: Verb) -> Step {
    Step::Command(Command {
        count: pending.count(),
        verb,
    })
}

fn simple(pending: Pending, verb: Verb) -> Step {
    if pending.operator.is_some() {
        return Step::Cancel;
    }
    command(pending, verb)
}

fn motion(pending: Pending, motion: Motion) -> Step {
    match pending.operator {
        Some(operator) => command(
            pending,
            Verb::Operate {
                operator,
                target: OperatorTarget::Motion(motion),
            },
        ),
        None => command(pending, Verb::Motion(motion)),
    }
}

fn object_command(pending: Pending, mode: VimMode, object: TextObject) -> Step {
    match pending.operator {
        Some(operator) => command(
            pending,
            Verb::Operate {
                operator,
                target: OperatorTarget::Object(object),
            },
        ),
        None if mode.is_visual() => command(pending, Verb::SelectObject(object)),
        None => Step::Cancel,
    }
}

fn operator(pending: &mut Pending, mode: VimMode, operator: Operator) -> Step {
    if mode.is_visual() {
        return command(
            *pending,
            Verb::Operate {
                operator,
                target: OperatorTarget::Selection,
            },
        );
    }
    match pending.operator {
        Some(existing) if existing == operator => command(
            *pending,
            Verb::Operate {
                operator,
                target: OperatorTarget::Line,
            },
        ),
        Some(_) => Step::Cancel,
        None => {
            pending.operator = Some(operator);
            Step::Pending
        }
    }
}

fn await_key(pending: &mut Pending, stage: Stage) -> Step {
    pending.stage = stage;
    Step::Pending
}

fn start(pending: &mut Pending, mode: VimMode, key: Key) -> Step {
    match key {
        Key::Escape => {
            if mode.is_visual() {
                command(*pending, Verb::EnterNormal)
            } else {
                Step::Cancel
            }
        }
        Key::Char(digit @ '0'..='9') if digit != '0' || pending.has_count() => {
            pending.push_digit(digit.to_digit(10).unwrap_or(0));
            Step::Pending
        }
        Key::Char('0') | Key::Home => motion(*pending, Motion::LineStart),
        Key::Char('^') => motion(*pending, Motion::FirstNonBlank),
        Key::Char('$') | Key::End => motion(*pending, Motion::LineEnd),
        Key::Char('h') | Key::Left | Key::Backspace => motion(*pending, Motion::Left),
        Key::Char('l') | Key::Right => motion(*pending, Motion::Right),
        Key::Char('k') | Key::Up => motion(*pending, Motion::Up),
        Key::Char('j') | Key::Down | Key::Enter => motion(*pending, Motion::Down),
        Key::Char('w') => motion(*pending, Motion::WordForward { big: false }),
        Key::Char('W') => motion(*pending, Motion::WordForward { big: true }),
        Key::Char('b') => motion(*pending, Motion::WordBackward { big: false }),
        Key::Char('B') => motion(*pending, Motion::WordBackward { big: true }),
        Key::Char('e') => motion(*pending, Motion::WordEnd { big: false }),
        Key::Char('E') => motion(*pending, Motion::WordEnd { big: true }),
        Key::Char('G') => motion(*pending, Motion::LastLine),
        Key::Char('{') => motion(*pending, Motion::ParagraphBackward),
        Key::Char('}') => motion(*pending, Motion::ParagraphForward),
        Key::Char(';') => motion(*pending, Motion::RepeatFind { reverse: false }),
        Key::Char(',') => motion(*pending, Motion::RepeatFind { reverse: true }),
        Key::Char('f') => await_key(
            pending,
            Stage::Find {
                till: false,
                backward: false,
            },
        ),
        Key::Char('F') => await_key(
            pending,
            Stage::Find {
                till: false,
                backward: true,
            },
        ),
        Key::Char('t') => await_key(
            pending,
            Stage::Find {
                till: true,
                backward: false,
            },
        ),
        Key::Char('T') => await_key(
            pending,
            Stage::Find {
                till: true,
                backward: true,
            },
        ),
        Key::Char('g') => await_key(pending, Stage::Prefix),
        Key::Char('d') => operator(pending, mode, Operator::Delete),
        Key::Char('c') => operator(pending, mode, Operator::Change),
        Key::Char('y') => operator(pending, mode, Operator::Yank),
        Key::Char('i') => {
            if pending.operator.is_some() || mode.is_visual() {
                await_key(pending, Stage::Object { around: false })
            } else {
                command(*pending, Verb::Insert(InsertAt::Cursor))
            }
        }
        Key::Char('a') => {
            if pending.operator.is_some() || mode.is_visual() {
                await_key(pending, Stage::Object { around: true })
            } else {
                command(*pending, Verb::Insert(InsertAt::After))
            }
        }
        Key::Char('I') => simple(*pending, Verb::Insert(InsertAt::LineStart)),
        Key::Char('A') => simple(*pending, Verb::Insert(InsertAt::LineEnd)),
        Key::Char('o') if mode.is_visual() => simple(*pending, Verb::SwapVisualEnds),
        Key::Char('o') => simple(*pending, Verb::Insert(InsertAt::OpenBelow)),
        Key::Char('O') => simple(*pending, Verb::Insert(InsertAt::OpenAbove)),
        Key::Char('x') if mode.is_visual() => command(
            *pending,
            Verb::Operate {
                operator: Operator::Delete,
                target: OperatorTarget::Selection,
            },
        ),
        Key::Char('x') => simple(*pending, Verb::DeleteChar { before: false }),
        Key::Char('X') => simple(*pending, Verb::DeleteChar { before: true }),
        Key::Char('D') => simple(*pending, Verb::DeleteToLineEnd),
        Key::Char('C') => simple(*pending, Verb::ChangeToLineEnd),
        Key::Char('Y') => simple(*pending, Verb::YankLine),
        Key::Char('s') => simple(*pending, Verb::SubstituteChar),
        Key::Char('S') => simple(*pending, Verb::SubstituteLine),
        Key::Char('J') => simple(*pending, Verb::Join),
        Key::Char('r') => await_key(pending, Stage::Replace),
        Key::Char('~') => simple(*pending, Verb::ToggleCase),
        Key::Char('>') if mode.is_visual() => simple(*pending, Verb::Indent { outdent: false }),
        Key::Char('<') if mode.is_visual() => simple(*pending, Verb::Indent { outdent: true }),
        Key::Char('>') => await_key(pending, Stage::Indent { outdent: false }),
        Key::Char('<') => await_key(pending, Stage::Indent { outdent: true }),
        Key::Char('p') => simple(*pending, Verb::Paste { before: false }),
        Key::Char('P') => simple(*pending, Verb::Paste { before: true }),
        Key::Char('u') => simple(*pending, Verb::Undo),
        Key::Char('v') => simple(*pending, Verb::EnterVisual(VisualKind::Char)),
        Key::Char('V') => simple(*pending, Verb::EnterVisual(VisualKind::Line)),
        Key::Char('z') if pending.operator.is_none() => await_key(pending, Stage::Scroll),
        Key::Ctrl('d') => motion(*pending, Motion::HalfPage { down: true }),
        Key::Ctrl('u') => motion(*pending, Motion::HalfPage { down: false }),
        Key::Ctrl('f') | Key::PageDown => motion(*pending, Motion::Page { down: true }),
        Key::Ctrl('b') | Key::PageUp => motion(*pending, Motion::Page { down: false }),
        Key::Ctrl('r') => simple(*pending, Verb::Redo),
        Key::Ctrl(_) => Step::PassThrough,
        Key::Char(_) => Step::Cancel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(mode: VimMode, keys: &str) -> Step {
        let mut pending = Pending::default();
        let mut last = Step::Cancel;
        for character in keys.chars() {
            last = step(&mut pending, mode, Key::Char(character));
        }
        last
    }

    fn verb(mode: VimMode, keys: &str) -> Verb {
        match run(mode, keys) {
            Step::Command(command) => command.verb,
            other => panic!("expected a command from {keys:?}, got {other:?}"),
        }
    }

    fn parsed(mode: VimMode, keys: &str) -> Command {
        match run(mode, keys) {
            Step::Command(command) => command,
            other => panic!("expected a command from {keys:?}, got {other:?}"),
        }
    }

    #[test]
    fn bare_motions_carry_their_count() {
        assert_eq!(
            parsed(VimMode::Normal, "w"),
            Command {
                count: None,
                verb: Verb::Motion(Motion::WordForward { big: false })
            }
        );
        assert_eq!(
            parsed(VimMode::Normal, "3w"),
            Command {
                count: Some(3),
                verb: Verb::Motion(Motion::WordForward { big: false })
            }
        );
        assert_eq!(
            parsed(VimMode::Normal, "12j"),
            Command {
                count: Some(12),
                verb: Verb::Motion(Motion::Down)
            }
        );
    }

    #[test]
    fn zero_is_a_motion_until_a_count_is_running() {
        assert_eq!(verb(VimMode::Normal, "0"), Verb::Motion(Motion::LineStart));
        assert_eq!(
            parsed(VimMode::Normal, "10j"),
            Command {
                count: Some(10),
                verb: Verb::Motion(Motion::Down)
            }
        );
    }

    #[test]
    fn operator_counts_multiply() {
        assert_eq!(
            parsed(VimMode::Normal, "2d3w"),
            Command {
                count: Some(6),
                verb: Verb::Operate {
                    operator: Operator::Delete,
                    target: OperatorTarget::Motion(Motion::WordForward { big: false })
                }
            }
        );
        assert_eq!(parsed(VimMode::Normal, "d2w").count, Some(2));
        assert_eq!(parsed(VimMode::Normal, "2dw").count, Some(2));
    }

    #[test]
    fn doubled_operators_are_linewise() {
        for (keys, operator) in [
            ("dd", Operator::Delete),
            ("cc", Operator::Change),
            ("yy", Operator::Yank),
        ] {
            assert_eq!(
                verb(VimMode::Normal, keys),
                Verb::Operate {
                    operator,
                    target: OperatorTarget::Line
                }
            );
        }
        assert_eq!(parsed(VimMode::Normal, "3dd").count, Some(3));
    }

    #[test]
    fn mismatched_operators_cancel() {
        assert_eq!(run(VimMode::Normal, "dy"), Step::Cancel);
        assert_eq!(run(VimMode::Normal, "dp"), Step::Cancel);
    }

    #[test]
    fn operators_take_text_objects() {
        assert_eq!(
            verb(VimMode::Normal, "ci\""),
            Verb::Operate {
                operator: Operator::Change,
                target: OperatorTarget::Object(TextObject::from_key('"', false).unwrap())
            }
        );
        assert_eq!(
            verb(VimMode::Normal, "da("),
            Verb::Operate {
                operator: Operator::Delete,
                target: OperatorTarget::Object(TextObject::from_key('(', true).unwrap())
            }
        );
        assert_eq!(run(VimMode::Normal, "diz"), Step::Cancel);
    }

    #[test]
    fn insert_entries_only_read_as_objects_after_an_operator() {
        assert_eq!(
            verb(VimMode::Normal, "i"),
            Verb::Insert(InsertAt::Cursor),
            "a bare i inserts"
        );
        assert_eq!(verb(VimMode::Normal, "a"), Verb::Insert(InsertAt::After));
        assert_eq!(
            verb(VimMode::Visual, "iw"),
            Verb::SelectObject(TextObject::from_key('w', false).unwrap()),
            "visual mode grows the selection instead"
        );
    }

    #[test]
    fn pending_character_keys_wait_for_their_argument() {
        let mut pending = Pending::default();
        assert_eq!(
            step(&mut pending, VimMode::Normal, Key::Char('f')),
            Step::Pending
        );
        assert_eq!(
            step(&mut pending, VimMode::Normal, Key::Char('x')),
            Step::Command(Command {
                count: None,
                verb: Verb::Motion(Motion::Find(FindChar {
                    target: 'x',
                    till: false,
                    backward: false
                }))
            })
        );
        assert_eq!(pending, Pending::default(), "the pending state is cleared");

        assert_eq!(
            verb(VimMode::Normal, "rz"),
            Verb::Replace('z'),
            "r takes any character"
        );
        assert_eq!(
            verb(VimMode::Normal, "r3"),
            Verb::Replace('3'),
            "even a digit"
        );
    }

    #[test]
    fn escape_cancels_a_partial_command() {
        let mut pending = Pending::default();
        step(&mut pending, VimMode::Normal, Key::Char('2'));
        step(&mut pending, VimMode::Normal, Key::Char('d'));
        assert_ne!(pending, Pending::default());
        assert_eq!(
            step(&mut pending, VimMode::Normal, Key::Escape),
            Step::Cancel
        );
        assert_eq!(pending, Pending::default());
    }

    #[test]
    fn escape_leaves_visual_mode() {
        let mut pending = Pending::default();
        assert_eq!(
            step(&mut pending, VimMode::Visual, Key::Escape),
            Step::Command(Command {
                count: None,
                verb: Verb::EnterNormal
            })
        );
    }

    #[test]
    fn a_non_character_key_abandons_a_pending_argument() {
        let mut pending = Pending::default();
        step(&mut pending, VimMode::Normal, Key::Char('f'));
        assert_eq!(step(&mut pending, VimMode::Normal, Key::Down), Step::Cancel);
        assert_eq!(pending, Pending::default());
    }

    #[test]
    fn garbage_keys_are_swallowed_rather_than_typed() {
        assert_eq!(run(VimMode::Normal, "q"), Step::Cancel);
        assert_eq!(run(VimMode::Normal, "@"), Step::Cancel);
        assert_eq!(run(VimMode::Normal, "2q"), Step::Cancel);
    }

    #[test]
    fn unclaimed_control_chords_pass_through() {
        let mut pending = Pending::default();
        assert_eq!(
            step(&mut pending, VimMode::Normal, Key::Ctrl('z')),
            Step::PassThrough
        );
        assert_eq!(
            step(&mut pending, VimMode::Normal, Key::Ctrl('d')),
            Step::Command(Command {
                count: None,
                verb: Verb::Motion(Motion::HalfPage { down: true })
            })
        );
    }

    #[test]
    fn prefix_keys_resolve_their_second_key() {
        assert_eq!(verb(VimMode::Normal, "gg"), Verb::Motion(Motion::FirstLine));
        assert_eq!(
            verb(VimMode::Normal, "ge"),
            Verb::Motion(Motion::WordEndBackward { big: false })
        );
        assert_eq!(
            verb(VimMode::Normal, "dgg"),
            Verb::Operate {
                operator: Operator::Delete,
                target: OperatorTarget::Motion(Motion::FirstLine)
            }
        );
        assert_eq!(run(VimMode::Normal, "gq"), Step::Cancel);
        assert_eq!(
            verb(VimMode::Normal, "zz"),
            Verb::Scroll(ScrollAlign::Center)
        );
        assert_eq!(verb(VimMode::Normal, "zt"), Verb::Scroll(ScrollAlign::Top));
        assert_eq!(run(VimMode::Normal, "zq"), Step::Cancel);
    }

    #[test]
    fn indent_needs_doubling_in_normal_mode_but_not_in_visual() {
        assert_eq!(verb(VimMode::Normal, ">>"), Verb::Indent { outdent: false });
        assert_eq!(verb(VimMode::Normal, "<<"), Verb::Indent { outdent: true });
        assert_eq!(parsed(VimMode::Normal, "3>>").count, Some(3));
        assert_eq!(run(VimMode::Normal, "><"), Step::Cancel);
        assert_eq!(verb(VimMode::Visual, ">"), Verb::Indent { outdent: false });
    }

    #[test]
    fn visual_mode_operators_act_on_the_selection() {
        for (keys, operator) in [
            ("d", Operator::Delete),
            ("c", Operator::Change),
            ("y", Operator::Yank),
            ("x", Operator::Delete),
        ] {
            assert_eq!(
                verb(VimMode::Visual, keys),
                Verb::Operate {
                    operator,
                    target: OperatorTarget::Selection
                }
            );
        }
        assert_eq!(verb(VimMode::VisualLine, "o"), Verb::SwapVisualEnds);
        assert_eq!(
            verb(VimMode::Normal, "o"),
            Verb::Insert(InsertAt::OpenBelow)
        );
    }

    #[test]
    fn named_keys_move_like_their_letters() {
        let mut pending = Pending::default();
        for (key, expected) in [
            (Key::Left, Motion::Left),
            (Key::Right, Motion::Right),
            (Key::Up, Motion::Up),
            (Key::Down, Motion::Down),
            (Key::Enter, Motion::Down),
            (Key::Backspace, Motion::Left),
            (Key::Home, Motion::LineStart),
            (Key::End, Motion::LineEnd),
            (Key::PageDown, Motion::Page { down: true }),
            (Key::PageUp, Motion::Page { down: false }),
        ] {
            assert_eq!(
                step(&mut pending, VimMode::Normal, key),
                Step::Command(Command {
                    count: None,
                    verb: Verb::Motion(expected)
                })
            );
        }
    }
}
