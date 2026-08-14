//! Agent attention: one transition detector per pane, two outputs.
//!
//! [`AgentAttentionTracker`] is the single place that watches per-pane agent
//! status change. From the same edges it emits (a) a chime and (b) the
//! finished-but-unseen badge state the workspace chrome renders; a desktop
//! banner would ride the same edges when it lands.
//!
//! Chime rules: a `Request` on any transition into needs-input, a `Done` only
//! on the exact working -> idle edge, both silent while the user is already
//! watching that pane. A pane's first observation only seeds the baseline, so
//! restoring a session never rings.
//!
//! Playback synthesizes two short PCM WAVs in code, materializes each into the
//! temp dir once, and hands the file to the platform's own player (`afplay`,
//! or the first of `paplay`/`pw-play`/`aplay`) on a background thread with a
//! kill deadline. No audio dependency, no bundled assets. `ZZ_AGENT_SOUND=0`
//! disables it.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use zz_protocol::PaneId;

const SOUND_ENV: &str = "ZZ_AGENT_SOUND";
const SAMPLE_RATE: u32 = 44_100;
const BITS_PER_SAMPLE: u16 = 16;
const CHANNELS: u16 = 1;
const WAV_HEADER_LEN: usize = 44;
const PLAYER_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(
    dead_code,
    reason = "the live states are only constructed by the agent-pane build; the chrome reads them in every build"
)]
pub(crate) enum AgentPaneStatus {
    #[default]
    Idle,
    Working,
    NeedsInput,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentBadge {
    NeedsInput,
    Failed,
    Working,
    Finished,
}

impl AgentBadge {
    const fn rank(self) -> u8 {
        match self {
            Self::NeedsInput => 0,
            Self::Failed => 1,
            Self::Working => 2,
            Self::Finished => 3,
        }
    }

    pub(crate) fn merge(self, other: Self) -> Self {
        if other.rank() < self.rank() {
            other
        } else {
            self
        }
    }

    pub(crate) fn merge_into(slot: &mut Option<Self>, badge: Self) {
        *slot = Some(slot.map_or(badge, |current| current.merge(badge)));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Chime {
    Request,
    Done,
}

const fn chime_for(previous: AgentPaneStatus, next: AgentPaneStatus) -> Option<Chime> {
    match (previous, next) {
        (AgentPaneStatus::NeedsInput, AgentPaneStatus::NeedsInput) => None,
        (_, AgentPaneStatus::NeedsInput) => Some(Chime::Request),
        (AgentPaneStatus::Working, AgentPaneStatus::Idle) => Some(Chime::Done),
        _ => None,
    }
}

#[derive(Debug, Default)]
pub(crate) struct AgentAttentionTracker {
    previous: BTreeMap<PaneId, AgentPaneStatus>,
    unseen: BTreeSet<PaneId>,
}

impl AgentAttentionTracker {
    /// `watched` is the pane the user is demonstrably looking at (focused pane
    /// of an active window), the single gate for both suppressing chimes and
    /// clearing the finished-but-unseen badge. At most one chime per batch:
    /// several panes settling together must not stack, and a question outranks
    /// a completion.
    pub(crate) fn observe(
        &mut self,
        statuses: &BTreeMap<PaneId, AgentPaneStatus>,
        watched: Option<PaneId>,
    ) -> Option<Chime> {
        self.previous.retain(|pane, _| statuses.contains_key(pane));
        self.unseen.retain(|pane| statuses.contains_key(pane));

        let mut chime = None;
        for (&pane, &status) in statuses {
            let Some(previous) = self.previous.insert(pane, status) else {
                continue;
            };
            let Some(edge) = chime_for(previous, status) else {
                continue;
            };
            if watched == Some(pane) {
                continue;
            }
            if edge == Chime::Done {
                self.unseen.insert(pane);
            }
            chime = Some(match chime {
                Some(Chime::Request) => Chime::Request,
                _ => edge,
            });
        }

        if let Some(pane) = watched {
            self.unseen.remove(&pane);
        }
        chime
    }

    pub(crate) fn badge(&self, pane: PaneId, status: AgentPaneStatus) -> Option<AgentBadge> {
        match status {
            AgentPaneStatus::NeedsInput => Some(AgentBadge::NeedsInput),
            AgentPaneStatus::Failed => Some(AgentBadge::Failed),
            AgentPaneStatus::Working => Some(AgentBadge::Working),
            AgentPaneStatus::Idle => self.unseen.contains(&pane).then_some(AgentBadge::Finished),
        }
    }
}

pub(crate) fn play(chime: Chime) {
    if !enabled(std::env::var_os(SOUND_ENV).as_deref()) {
        return;
    }
    std::thread::spawn(move || {
        if let Err(error) = play_blocking(chime) {
            log::debug!(target: "zz::agent::sound", "chime playback failed: {error}");
        }
    });
}

fn enabled(value: Option<&OsStr>) -> bool {
    value.is_none_or(|value| value != "0")
}

fn play_blocking(chime: Chime) -> Result<(), String> {
    let path = materialize(chime)?;
    let mut child = spawn_player(path).ok_or_else(|| "no audio player available".to_owned())?;
    let deadline = std::time::Instant::now() + PLAYER_DEADLINE;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("player exited with {status}")),
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("player timed out".to_owned());
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(25)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        }
    }
}

fn materialize(chime: Chime) -> Result<&'static Path, String> {
    static FILES: OnceLock<[Result<PathBuf, String>; 2]> = OnceLock::new();
    let files = FILES.get_or_init(|| {
        [Chime::Request, Chime::Done].map(|chime| {
            let path = std::env::temp_dir().join(format!(
                "zz-agent-{}-{}.wav",
                match chime {
                    Chime::Request => "request",
                    Chime::Done => "done",
                },
                std::process::id()
            ));
            std::fs::write(&path, wav(chime)).map_err(|error| error.to_string())?;
            Ok(path)
        })
    });
    let index = match chime {
        Chime::Request => 0,
        Chime::Done => 1,
    };
    files[index]
        .as_deref()
        .map_err(|error: &String| error.clone())
}

#[cfg(target_os = "macos")]
fn spawn_player(path: &Path) -> Option<std::process::Child> {
    player_command("afplay", &[], path)
}

#[cfg(target_os = "linux")]
fn spawn_player(path: &Path) -> Option<std::process::Child> {
    [("paplay", &[][..]), ("pw-play", &[]), ("aplay", &["-q"])]
        .into_iter()
        .find_map(|(program, args)| player_command(program, args, path))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn spawn_player(_path: &Path) -> Option<std::process::Child> {
    None
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn player_command(program: &str, args: &[&str], path: &Path) -> Option<std::process::Child> {
    std::process::Command::new(program)
        .args(args)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
}

/// `(hz, milliseconds)`; a zero frequency is silence.
const fn segments(chime: Chime) -> &'static [(f32, u32)] {
    match chime {
        Chime::Request => &[(1318.0, 70), (0.0, 60), (1318.0, 70)],
        Chime::Done => &[(880.0, 110), (1174.0, 190)],
    }
}

fn wav(chime: Chime) -> Vec<u8> {
    let mut samples = Vec::new();
    for &(hz, ms) in segments(chime) {
        let count = (SAMPLE_RATE as f32 * ms as f32 / 1000.0) as usize;
        let fade = (count / 4).clamp(1, SAMPLE_RATE as usize / 125);
        for index in 0..count {
            if hz == 0.0 {
                samples.push(0);
                continue;
            }
            let phase = std::f32::consts::TAU * hz * index as f32 / SAMPLE_RATE as f32;
            let envelope = (index.min(count - index - 1) as f32 / fade as f32).min(1.0);
            samples.push((phase.sin() * envelope * 0.3 * f32::from(i16::MAX)) as i16);
        }
    }
    encode(&samples)
}

fn encode(samples: &[i16]) -> Vec<u8> {
    let data_len = u32::try_from(samples.len() * 2).unwrap_or(u32::MAX);
    let block_align = CHANNELS * BITS_PER_SAMPLE / 8;
    let mut out = Vec::with_capacity(WAV_HEADER_LEN + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(data_len + 36).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&CHANNELS.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * u32::from(block_align)).to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(data: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(data[offset..offset + 4].try_into().expect("four bytes"))
    }

    fn u16_at(data: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes(data[offset..offset + 2].try_into().expect("two bytes"))
    }

    fn statuses(entries: &[(u64, AgentPaneStatus)]) -> BTreeMap<PaneId, AgentPaneStatus> {
        entries
            .iter()
            .map(|&(pane, status)| (PaneId(pane), status))
            .collect()
    }

    #[test]
    fn synthesized_chimes_carry_a_canonical_pcm_header() {
        for chime in [Chime::Request, Chime::Done] {
            let data = wav(chime);
            assert_eq!(&data[..4], b"RIFF");
            assert_eq!(u32_at(&data, 4) as usize, data.len() - 8);
            assert_eq!(&data[8..12], b"WAVE");
            assert_eq!(&data[12..16], b"fmt ");
            assert_eq!(u32_at(&data, 16), 16);
            assert_eq!(u16_at(&data, 20), 1);
            assert_eq!(u16_at(&data, 22), CHANNELS);
            assert_eq!(u32_at(&data, 24), SAMPLE_RATE);
            assert_eq!(u32_at(&data, 28), SAMPLE_RATE * 2);
            assert_eq!(u16_at(&data, 32), 2);
            assert_eq!(u16_at(&data, 34), BITS_PER_SAMPLE);
            assert_eq!(&data[36..40], b"data");
            assert_eq!(u32_at(&data, 40) as usize, data.len() - WAV_HEADER_LEN);
            assert_eq!((data.len() - WAV_HEADER_LEN) % 2, 0);
        }
    }

    #[test]
    fn the_two_chimes_are_distinct_and_audible() {
        let request = wav(Chime::Request);
        let done = wav(Chime::Done);
        assert_ne!(request, done);
        for data in [&request, &done] {
            assert!(data.len() > SAMPLE_RATE as usize / 10);
            assert!(data[WAV_HEADER_LEN..].iter().any(|&byte| byte != 0));
        }
    }

    #[test]
    fn chime_edges_follow_the_question_and_completion_rules() {
        use AgentPaneStatus::{Failed, Idle, NeedsInput, Working};
        assert_eq!(chime_for(Working, NeedsInput), Some(Chime::Request));
        assert_eq!(chime_for(Idle, NeedsInput), Some(Chime::Request));
        assert_eq!(chime_for(Failed, NeedsInput), Some(Chime::Request));
        assert_eq!(chime_for(Working, Idle), Some(Chime::Done));
        assert_eq!(chime_for(NeedsInput, Idle), None);
        assert_eq!(chime_for(Failed, Idle), None);
        assert_eq!(chime_for(NeedsInput, NeedsInput), None);
        assert_eq!(chime_for(Idle, Working), None);
        assert_eq!(chime_for(Working, Failed), None);
    }

    #[test]
    fn a_panes_first_observation_only_seeds_the_baseline() {
        let mut tracker = AgentAttentionTracker::default();
        let chime = tracker.observe(&statuses(&[(1, AgentPaneStatus::NeedsInput)]), None);
        assert_eq!(chime, None);
        assert_eq!(
            tracker.badge(PaneId(1), AgentPaneStatus::NeedsInput),
            Some(AgentBadge::NeedsInput)
        );
    }

    #[test]
    fn an_unwatched_completion_chimes_and_stays_unseen_until_focus() {
        let mut tracker = AgentAttentionTracker::default();
        tracker.observe(&statuses(&[(1, AgentPaneStatus::Working)]), None);

        let chime = tracker.observe(&statuses(&[(1, AgentPaneStatus::Idle)]), None);
        assert_eq!(chime, Some(Chime::Done));
        assert_eq!(
            tracker.badge(PaneId(1), AgentPaneStatus::Idle),
            Some(AgentBadge::Finished)
        );

        let chime = tracker.observe(&statuses(&[(1, AgentPaneStatus::Idle)]), Some(PaneId(1)));
        assert_eq!(chime, None);
        assert_eq!(tracker.badge(PaneId(1), AgentPaneStatus::Idle), None);
    }

    #[test]
    fn a_watched_pane_neither_chimes_nor_collects_a_badge() {
        let mut tracker = AgentAttentionTracker::default();
        tracker.observe(&statuses(&[(1, AgentPaneStatus::Working)]), Some(PaneId(1)));
        let chime = tracker.observe(&statuses(&[(1, AgentPaneStatus::Idle)]), Some(PaneId(1)));
        assert_eq!(chime, None);
        assert_eq!(tracker.badge(PaneId(1), AgentPaneStatus::Idle), None);

        let mut tracker = AgentAttentionTracker::default();
        tracker.observe(&statuses(&[(1, AgentPaneStatus::Working)]), Some(PaneId(1)));
        let chime = tracker.observe(
            &statuses(&[(1, AgentPaneStatus::NeedsInput)]),
            Some(PaneId(1)),
        );
        assert_eq!(chime, None);
    }

    #[test]
    fn a_question_outranks_a_completion_in_the_same_batch() {
        let mut tracker = AgentAttentionTracker::default();
        tracker.observe(
            &statuses(&[(1, AgentPaneStatus::Working), (2, AgentPaneStatus::Working)]),
            None,
        );
        let chime = tracker.observe(
            &statuses(&[(1, AgentPaneStatus::Idle), (2, AgentPaneStatus::NeedsInput)]),
            None,
        );
        assert_eq!(chime, Some(Chime::Request));
    }

    #[test]
    fn a_closed_pane_drops_its_baseline_and_its_badge() {
        let mut tracker = AgentAttentionTracker::default();
        tracker.observe(&statuses(&[(1, AgentPaneStatus::Working)]), None);
        tracker.observe(&statuses(&[(1, AgentPaneStatus::Idle)]), None);
        assert_eq!(
            tracker.badge(PaneId(1), AgentPaneStatus::Idle),
            Some(AgentBadge::Finished)
        );

        tracker.observe(&statuses(&[]), None);
        assert_eq!(tracker.badge(PaneId(1), AgentPaneStatus::Idle), None);
        let chime = tracker.observe(&statuses(&[(1, AgentPaneStatus::Idle)]), None);
        assert_eq!(chime, None);
    }

    #[test]
    fn rollup_badges_keep_the_most_urgent_pane() {
        let mut slot = None;
        AgentBadge::merge_into(&mut slot, AgentBadge::Finished);
        AgentBadge::merge_into(&mut slot, AgentBadge::Working);
        assert_eq!(slot, Some(AgentBadge::Working));
        AgentBadge::merge_into(&mut slot, AgentBadge::NeedsInput);
        AgentBadge::merge_into(&mut slot, AgentBadge::Failed);
        assert_eq!(slot, Some(AgentBadge::NeedsInput));
    }

    #[test]
    fn the_env_kill_switch_only_answers_to_zero() {
        assert!(enabled(None));
        assert!(enabled(Some(OsStr::new("1"))));
        assert!(enabled(Some(OsStr::new(""))));
        assert!(!enabled(Some(OsStr::new("0"))));
    }
}
