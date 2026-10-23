use zz_gpui::{Autocapitalize, TextInputAction, TextInputConfiguration, TextInputMode};

const TYPE_CLASS_TEXT: i32 = 1;
const TYPE_CLASS_NUMBER: i32 = 2;
const TYPE_CLASS_PHONE: i32 = 3;
const TYPE_TEXT_FLAG_CAP_CHARACTERS: i32 = 0x1000;
const TYPE_TEXT_FLAG_CAP_WORDS: i32 = 0x2000;
const TYPE_TEXT_FLAG_CAP_SENTENCES: i32 = 0x4000;
const TYPE_TEXT_FLAG_AUTO_CORRECT: i32 = 0x8000;
const TYPE_TEXT_FLAG_MULTI_LINE: i32 = 0x20000;
const TYPE_TEXT_FLAG_NO_SUGGESTIONS: i32 = 0x80000;
const TYPE_TEXT_VARIATION_URI: i32 = 0x10;
const TYPE_TEXT_VARIATION_EMAIL_ADDRESS: i32 = 0x20;
const TYPE_TEXT_VARIATION_VISIBLE_PASSWORD: i32 = 0x90;
const TYPE_NUMBER_FLAG_DECIMAL: i32 = 0x2000;
const IME_ACTION_UNSPECIFIED: i32 = 0;
const IME_ACTION_NONE: i32 = 1;
const IME_ACTION_GO: i32 = 2;
const IME_ACTION_SEARCH: i32 = 3;
const IME_ACTION_SEND: i32 = 4;
const IME_ACTION_NEXT: i32 = 5;
pub const IME_ACTION_DONE: i32 = 6;
const IME_ACTION_PREVIOUS: i32 = 7;
const IME_FLAG_NO_FULLSCREEN: i32 = 0x2000000;
const IME_FLAG_NO_EXTRACT_UI: i32 = 0x10000000;

/// A terminal asks for a Return that inserts a line break; every other field
/// confirms with its action key.
pub fn is_terminal(configuration: &TextInputConfiguration) -> bool {
    configuration.input_action == TextInputAction::Enter
}

pub fn editor_flags(configuration: &TextInputConfiguration) -> (i32, i32) {
    let action = match configuration.input_action {
        TextInputAction::Enter => IME_ACTION_NONE,
        TextInputAction::Go => IME_ACTION_GO,
        TextInputAction::Next => IME_ACTION_NEXT,
        TextInputAction::Previous => IME_ACTION_PREVIOUS,
        TextInputAction::Search => IME_ACTION_SEARCH,
        TextInputAction::Send => IME_ACTION_SEND,
        TextInputAction::Done => IME_ACTION_DONE,
        TextInputAction::Unspecified => IME_ACTION_UNSPECIFIED,
    };
    let text = |variation: i32| {
        let mut kind = TYPE_CLASS_TEXT | variation;
        kind |= match configuration.autocapitalize {
            Autocapitalize::None => 0,
            Autocapitalize::Words => TYPE_TEXT_FLAG_CAP_WORDS,
            Autocapitalize::Sentences => TYPE_TEXT_FLAG_CAP_SENTENCES,
            Autocapitalize::Characters => TYPE_TEXT_FLAG_CAP_CHARACTERS,
        };
        if configuration.autocorrect {
            kind |= TYPE_TEXT_FLAG_AUTO_CORRECT;
        }
        if !configuration.suggestions {
            kind |= TYPE_TEXT_FLAG_NO_SUGGESTIONS;
        }
        if is_terminal(configuration) {
            kind |= TYPE_TEXT_FLAG_MULTI_LINE;
            if !configuration.suggestions && variation == 0 {
                kind |= TYPE_TEXT_VARIATION_VISIBLE_PASSWORD;
            }
        }
        kind
    };
    let kind = match configuration.input_mode {
        TextInputMode::Text => text(0),
        TextInputMode::Search => text(0),
        TextInputMode::Url => text(TYPE_TEXT_VARIATION_URI),
        TextInputMode::Email => text(TYPE_TEXT_VARIATION_EMAIL_ADDRESS),
        TextInputMode::Numeric => TYPE_CLASS_NUMBER,
        TextInputMode::Decimal => TYPE_CLASS_NUMBER | TYPE_NUMBER_FLAG_DECIMAL,
        TextInputMode::Tel => TYPE_CLASS_PHONE,
        TextInputMode::None => 0,
    };
    let action = match (configuration.input_mode, action) {
        (TextInputMode::Search, IME_ACTION_UNSPECIFIED) => IME_ACTION_SEARCH,
        (
            TextInputMode::Numeric | TextInputMode::Decimal | TextInputMode::Tel,
            IME_ACTION_NONE | IME_ACTION_UNSPECIFIED,
        ) => IME_ACTION_DONE,
        (_, action) => action,
    };
    (
        kind,
        action | IME_FLAG_NO_FULLSCREEN | IME_FLAG_NO_EXTRACT_UI,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminals_get_raw_multiline_input() {
        let terminal = TextInputConfiguration {
            input_action: TextInputAction::Enter,
            ..TextInputConfiguration::default()
        };
        let (kind, options) = editor_flags(&terminal);
        assert_eq!(kind & 0xf, TYPE_CLASS_TEXT);
        assert_ne!(kind & TYPE_TEXT_FLAG_MULTI_LINE, 0);
        assert_eq!(options & 0xff, IME_ACTION_NONE);
        assert_ne!(options & IME_FLAG_NO_EXTRACT_UI, 0);
    }

    #[test]
    fn number_pads_confirm_with_done() {
        let decimal = TextInputConfiguration {
            input_mode: TextInputMode::Decimal,
            ..TextInputConfiguration::default()
        };
        let (kind, options) = editor_flags(&decimal);
        assert_eq!(kind, TYPE_CLASS_NUMBER | TYPE_NUMBER_FLAG_DECIMAL);
        assert_eq!(options & 0xff, IME_ACTION_DONE);
    }
}
