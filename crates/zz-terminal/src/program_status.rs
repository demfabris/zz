use base64::{
    Engine as _, alphabet,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
};

pub(crate) const PROGRAM_STATUS_PREFIX: &[u8] = b"7501;";
const MAX_BODY_BYTES: usize = 4096 - b"\x1b]7501;".len() - b"\x1b\\".len();
pub(crate) const MAX_PROGRAM_STATUS_OSC_BYTES: usize = PROGRAM_STATUS_PREFIX.len() + MAX_BODY_BYTES;
const MAX_KEY_BYTES: usize = 16;
const MAX_MSG_ENCODED_BYTES: usize = 2732;
const MAX_MSG_BYTES: usize = 2048;
const MAX_TITLE_ENCODED_BYTES: usize = 256;
const MAX_TITLE_BYTES: usize = 192;
const MAX_APP_BYTES: usize = 32;
const MAX_ID_BYTES: usize = 128;
const MAX_ID_SEGMENT_BYTES: usize = 32;
const MAX_ID_DEPTH: usize = 8;
pub const MAX_PROGRAM_STATUS_RECORDS: usize = 256;

const TEXT: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
}

impl ProgramState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Error => "error",
        }
    }

    const fn urgency(self) -> u8 {
        match self {
            Self::Blocked => 0,
            Self::Error => 1,
            Self::Working => 2,
            Self::Done => 3,
            Self::Idle => 4,
        }
    }

    const fn outlives_program(self) -> bool {
        matches!(self, Self::Done | Self::Error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramBlockKind {
    Permission,
    Question,
    Auth,
}

impl ProgramBlockKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Permission => "permission",
            Self::Question => "question",
            Self::Auth => "auth",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramStatusRecord {
    pub id: String,
    pub state: ProgramState,
    pub kind: Option<ProgramBlockKind>,
    pub progress: Option<u8>,
    pub app: String,
    pub title: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProgramStatusReport {
    Query,
    Update(ProgramStatusRecord),
    Clear(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProgramStatus {
    records: Vec<ProgramStatusRecord>,
    reported: bool,
}

impl ProgramStatus {
    #[must_use]
    pub fn records(&self) -> &[ProgramStatusRecord] {
        &self.records
    }

    #[must_use]
    pub const fn reported(&self) -> bool {
        self.reported
    }

    #[must_use]
    pub fn headline(&self) -> Option<ProgramStatusRecord> {
        let record = self
            .records
            .iter()
            .rev()
            .min_by_key(|record| record.state.urgency())?;
        let mut headline = record.clone();
        if headline.app.is_empty() {
            headline.app = self.inherited_app(&record.id).to_owned();
        }
        Some(headline)
    }

    fn inherited_app(&self, mut id: &str) -> &str {
        while !id.is_empty() {
            id = id.rsplit_once('/').map_or("", |(parent, _)| parent);
            if let Some(record) = self
                .records
                .iter()
                .find(|record| record.id == id && !record.app.is_empty())
            {
                return &record.app;
            }
        }
        ""
    }

    pub(crate) fn apply(&mut self, report: ProgramStatusReport) -> bool {
        let first = !std::mem::replace(&mut self.reported, true);
        let changed = match report {
            ProgramStatusReport::Query => false,
            ProgramStatusReport::Clear(id) => {
                let before = self.records.len();
                self.records.retain(|record| !within(&record.id, &id));
                self.records.len() != before
            }
            ProgramStatusReport::Update(record) if self.records.last() == Some(&record) => false,
            ProgramStatusReport::Update(record) => {
                if let Some(index) = self.records.iter().position(|old| old.id == record.id) {
                    self.records.remove(index);
                } else if self.records.len() >= MAX_PROGRAM_STATUS_RECORDS {
                    self.records.remove(0);
                }
                self.records.push(record);
                true
            }
        };
        first || changed
    }

    pub(crate) fn program_left(&mut self) -> bool {
        let before = self.records.len();
        self.records
            .retain(|record| record.state.outlives_program());
        self.records.len() != before
    }

    pub(crate) fn reset(&mut self) -> bool {
        let changed = self.reported || !self.records.is_empty();
        *self = Self::default();
        changed
    }
}

fn within(id: &str, ancestor: &str) -> bool {
    ancestor.is_empty()
        || id
            .strip_prefix(ancestor)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

pub(crate) fn parse_program_status(body: &[u8]) -> Option<ProgramStatusReport> {
    if body == b"?" {
        return Some(ProgramStatusReport::Query);
    }
    if body.len() > MAX_BODY_BYTES {
        return None;
    }
    let mut state = None;
    let mut id: &[u8] = b"";
    let mut kind = None;
    let mut progress = None;
    let mut app = None;
    let mut title: &[u8] = b"";
    let mut message: &[u8] = b"";
    for pair in body.split(|byte| *byte == b':') {
        let Some(equals) = pair.iter().position(|byte| *byte == b'=') else {
            continue;
        };
        let key = pair[..equals].trim_ascii();
        if key.len() > MAX_KEY_BYTES {
            return None;
        }
        let value = pair[equals + 1..].trim_ascii();
        if key.is_empty()
            || !key.iter().all(u8::is_ascii_lowercase)
            || !value.iter().copied().all(is_value_byte)
        {
            continue;
        }
        match key {
            b"state" => state = Some(value),
            b"id" => {
                if !is_id(value) {
                    return None;
                }
                id = value;
            }
            b"kind" => kind = Some(value),
            b"progress" => progress = Some(value),
            b"app" => {
                if value.len() > MAX_APP_BYTES {
                    return None;
                }
                app = Some(value);
            }
            b"title" => {
                if value.len() > MAX_TITLE_ENCODED_BYTES {
                    return None;
                }
                title = value;
            }
            b"msg" => {
                if value.len() > MAX_MSG_ENCODED_BYTES {
                    return None;
                }
                message = value;
            }
            _ => {}
        }
    }
    let title = decode_text(title, MAX_TITLE_BYTES)?;
    let message = decode_text(message, MAX_MSG_BYTES)?;
    let id = String::from_utf8(id.to_vec()).ok()?;
    let state = match state? {
        b"idle" => ProgramState::Idle,
        b"working" => ProgramState::Working,
        b"done" => ProgramState::Done,
        b"blocked" => ProgramState::Blocked,
        b"error" => ProgramState::Error,
        b"clear" => return Some(ProgramStatusReport::Clear(id)),
        _ => return None,
    };
    let kind = match (state, kind) {
        (ProgramState::Blocked, Some(b"permission")) => Some(ProgramBlockKind::Permission),
        (ProgramState::Blocked, Some(b"question")) => Some(ProgramBlockKind::Question),
        (ProgramState::Blocked, Some(b"auth")) => Some(ProgramBlockKind::Auth),
        _ => None,
    };
    let progress = progress
        .filter(|_| matches!(state, ProgramState::Working | ProgramState::Blocked))
        .and_then(parse_progress);
    let app = app
        .filter(|app| is_name(app))
        .and_then(|app| std::str::from_utf8(app).ok())
        .unwrap_or_default()
        .to_owned();
    Some(ProgramStatusReport::Update(ProgramStatusRecord {
        id,
        state,
        kind,
        progress,
        app,
        title,
        message,
    }))
}

const fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'+' | b'-')
}

const fn is_value_byte(byte: u8) -> bool {
    is_name_byte(byte) || matches!(byte, b',' | b'/' | b'=')
}

fn is_name(value: &[u8]) -> bool {
    !value.is_empty() && value.iter().copied().all(is_name_byte)
}

fn is_id(value: &[u8]) -> bool {
    value.len() <= MAX_ID_BYTES
        && value.split(|byte| *byte == b'/').count() <= MAX_ID_DEPTH
        && value
            .split(|byte| *byte == b'/')
            .all(|segment| segment.len() <= MAX_ID_SEGMENT_BYTES && is_name(segment))
}

fn parse_progress(value: &[u8]) -> Option<u8> {
    if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut progress = 0_u8;
    for digit in value {
        progress = progress.checked_mul(10)?.checked_add(digit - b'0')?;
        if progress > 100 {
            return None;
        }
    }
    Some(progress)
}

fn decode_text(encoded: &[u8], limit: usize) -> Option<String> {
    if encoded.is_empty() {
        return Some(String::new());
    }
    let bytes = TEXT.decode(encoded).ok()?;
    if bytes.len() > limit {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    if text.chars().any(char::is_control) {
        return None;
    }
    Some(
        text.chars()
            .filter(|character| !is_hidden_format(*character))
            .collect(),
    )
}

const fn is_hidden_format(character: char) -> bool {
    matches!(
        character,
        '\u{061c}'
            | '\u{200b}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(body: &str) -> ProgramStatusRecord {
        match parse_program_status(body.as_bytes()) {
            Some(ProgramStatusReport::Update(record)) => record,
            other => panic!("{body:?} parsed as {other:?}"),
        }
    }

    fn rejected(body: &str) -> bool {
        parse_program_status(body.as_bytes()).is_none()
    }

    fn record(id: &str, state: ProgramState) -> ProgramStatusRecord {
        ProgramStatusRecord {
            id: id.to_owned(),
            state,
            kind: None,
            progress: None,
            app: String::new(),
            title: String::new(),
            message: String::new(),
        }
    }

    #[test]
    fn the_spec_examples_parse() {
        let terraform = update(
            "state=blocked:kind=permission:app=terraform:msg=QXBwbHkgMyB0byBhZGQsIDEgdG8gY2hhbmdlLCAwIHRvIGRlc3Ryb3k/",
        );
        assert_eq!(terraform.state, ProgramState::Blocked);
        assert_eq!(terraform.kind, Some(ProgramBlockKind::Permission));
        assert_eq!(terraform.app, "terraform");
        assert_eq!(
            terraform.message,
            "Apply 3 to add, 1 to change, 0 to destroy?"
        );
        assert_eq!(terraform.id, "");
        assert_eq!(terraform.progress, None);

        let region = update(
            "state=working:id=us-east:title=VVMgRWFzdA==:progress=40:msg=UHVzaGluZyBpbWFnZQ==",
        );
        assert_eq!(region.id, "us-east");
        assert_eq!(region.title, "US East");
        assert_eq!(region.progress, Some(40));
        assert_eq!(region.message, "Pushing image");

        assert_eq!(
            parse_program_status(b"state=clear:id=eu-west"),
            Some(ProgramStatusReport::Clear("eu-west".to_owned()))
        );
        assert_eq!(parse_program_status(b"?"), Some(ProgramStatusReport::Query));
    }

    #[test]
    fn malformed_pairs_and_unknown_keys_are_skipped() {
        let record = update(" state = working :future=yes:oops:=x:Bad=1:msg=a b:progress=7");
        assert_eq!(record.state, ProgramState::Working);
        assert_eq!(record.message, "");
        assert_eq!(record.progress, Some(7));
        assert_eq!(update("state=idle:state=done").state, ProgramState::Done);
    }

    #[test]
    fn a_report_without_a_known_state_is_ignored() {
        assert!(rejected("app=cargo"));
        assert!(rejected("state=sleeping"));
        assert!(rejected("state=working:state=sleeping"));
        assert!(rejected(""));
    }

    #[test]
    fn keys_only_apply_to_the_states_they_belong_to() {
        assert_eq!(update("state=working:kind=auth").kind, None);
        assert_eq!(update("state=blocked:kind=coffee").kind, None);
        assert_eq!(update("state=done:progress=50").progress, None);
        assert_eq!(update("state=blocked:progress=50").progress, Some(50));
        for progress in ["101", "-1", "4a", "", "1000"] {
            assert_eq!(
                update(&format!("state=working:progress={progress}")).progress,
                None,
                "{progress}"
            );
        }
        assert_eq!(update("state=working:progress=100").progress, Some(100));
        assert_eq!(update("state=working:app=a/b").app, "");
        assert_eq!(update("state=working:app=").app, "");
    }

    #[test]
    fn a_bad_id_discards_the_report_instead_of_hitting_the_root() {
        assert!(rejected("state=clear:id=a//b"));
        assert!(rejected("state=clear:id="));
        assert!(rejected("state=clear:id=/a"));
        assert!(rejected("state=clear:id=a,b"));
        assert!(rejected(&format!("state=idle:id={}", "a/".repeat(8) + "a")));
        assert!(rejected(&format!("state=idle:id={}", "a".repeat(33))));
        assert_eq!(
            update(&format!("state=idle:id={}", ["a"; 8].join("/")))
                .id
                .len(),
            15
        );
    }

    #[test]
    fn limits_discard_the_whole_report() {
        assert!(rejected(&format!("state=idle:{}=1", "k".repeat(17))));
        assert!(rejected(&format!("state=idle:app={}", "a".repeat(33))));
        assert!(rejected(&format!("state=idle:title={}", "A".repeat(260))));
        assert!(rejected(&format!(
            "state=idle:msg={}:msg=",
            "A".repeat(2736)
        )));
        assert!(rejected(&format!("state=idle:msg={}", "QUFB".repeat(683))));
        assert!(!rejected(&format!("state=idle:msg={}", "QUFB".repeat(682))));
        assert!(rejected(&format!(
            "state=idle:x={}",
            "a".repeat(MAX_BODY_BYTES)
        )));
    }

    #[test]
    fn text_must_decode_to_safe_utf8() {
        assert!(rejected("state=idle:msg=A"));
        assert!(rejected("state=idle:msg=a.b-"));
        assert!(!rejected("state=idle:msg=!!!!"));
        assert!(rejected("state=idle:msg=/w=="));
        assert!(rejected("state=idle:msg=G1sybQ=="));
        assert!(rejected("state=idle:title=woA="));
        assert_eq!(update("state=idle:msg=aGk").message, "hi");
        assert_eq!(update("state=idle:msg=4oCuZXZpbA==").message, "evil");
    }

    #[test]
    fn a_report_replaces_its_record_and_clear_takes_the_subtree() {
        let mut status = ProgramStatus::default();
        assert!(status.apply(ProgramStatusReport::Update(update(
            "state=working:app=deploy"
        ))));
        assert!(status.apply(ProgramStatusReport::Update(record(
            "eu",
            ProgramState::Blocked
        ))));
        assert!(status.apply(ProgramStatusReport::Update(record(
            "eu/db",
            ProgramState::Working
        ))));
        assert!(!status.apply(ProgramStatusReport::Update(record(
            "eu/db",
            ProgramState::Working
        ))));
        assert_eq!(
            status.headline().map(|record| (record.id, record.app)),
            Some(("eu".to_owned(), "deploy".to_owned()))
        );
        assert!(status.apply(ProgramStatusReport::Update(update("state=done"))));
        assert_eq!(
            status
                .records()
                .iter()
                .find(|record| record.id.is_empty())
                .map(|record| record.app.as_str()),
            Some("")
        );
        assert!(status.apply(ProgramStatusReport::Clear("eu".to_owned())));
        assert_eq!(status.records().len(), 1);
        assert!(!status.apply(ProgramStatusReport::Clear("eu".to_owned())));
        assert!(status.apply(ProgramStatusReport::Clear(String::new())));
        assert!(status.records().is_empty());
        assert!(status.reported());
        assert!(!within("eureka", "eu"));
    }

    #[test]
    fn the_least_recently_updated_record_makes_room() {
        let mut status = ProgramStatus::default();
        for index in 0..MAX_PROGRAM_STATUS_RECORDS {
            status.apply(ProgramStatusReport::Update(record(
                &index.to_string(),
                ProgramState::Idle,
            )));
        }
        status.apply(ProgramStatusReport::Update(record(
            "0",
            ProgramState::Working,
        )));
        status.apply(ProgramStatusReport::Update(record(
            "new",
            ProgramState::Idle,
        )));
        assert_eq!(status.records().len(), MAX_PROGRAM_STATUS_RECORDS);
        assert!(status.records().iter().any(|record| record.id == "0"));
        assert!(!status.records().iter().any(|record| record.id == "1"));
    }

    #[test]
    fn leaving_the_program_keeps_only_done_and_error() {
        let mut status = ProgramStatus::default();
        for (id, state) in [
            ("a", ProgramState::Idle),
            ("b", ProgramState::Working),
            ("c", ProgramState::Done),
            ("d", ProgramState::Blocked),
            ("e", ProgramState::Error),
        ] {
            status.apply(ProgramStatusReport::Update(record(id, state)));
        }
        assert_eq!(
            status.headline().map(|record| record.id),
            Some("d".to_owned())
        );
        assert!(status.program_left());
        assert!(!status.program_left());
        let ids = status
            .records()
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["c", "e"]);
        assert_eq!(
            status.headline().map(|record| record.id),
            Some("e".to_owned())
        );
        assert!(status.reset());
        assert!(!status.reported());
        assert!(!status.reset());
    }
}
