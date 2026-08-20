use crate::{
    layout::{CellLayout, LayoutError, SplitSize},
    model::LayoutPreset,
};
use zz_protocol::{Axis, PaneId, SplitId};

struct Fixture<'a> {
    name: &'a str,
    expected_layout: &'a str,
    expected_sizes: Vec<(u16, u16)>,
    steps: Vec<&'a str>,
}

struct Replay {
    layout: CellLayout,
    panes: Vec<u64>,
    next_pane_id: u64,
    next_split_id: u64,
    active: u64,
    broken: Option<u64>,
}

impl Replay {
    fn new() -> Self {
        Self {
            layout: CellLayout::new(PaneId(0), 80, 24),
            panes: vec![0],
            next_pane_id: 1,
            next_split_id: 1,
            active: 0,
            broken: None,
        }
    }

    fn run(&mut self, command: &str) {
        let words = command.split_whitespace().collect::<Vec<_>>();
        match words[0] {
            "split-window" => self.split_window(&words),
            "kill-pane" => self.kill_pane(&words),
            "resize-pane" => self.resize_pane(&words),
            "select-layout" => self.select_layout(&words),
            "resize-window" => self.resize_window(&words),
            "break-pane" => self.break_pane(&words),
            "join-pane" => self.join_pane(&words),
            command => panic!("unsupported fixture command {command}"),
        }
    }

    fn split_window(&mut self, words: &[&str]) {
        let index = target_index(flag_value(words, "-t").unwrap());
        let target = PaneId(self.panes[index]);
        let axis = if has_flag(words, "-h") {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };
        let size = if let Some(value) = flag_value(words, "-l") {
            split_size(value)
        } else if let Some(value) = flag_value(words, "-p") {
            SplitSize::Percent(value.parse().unwrap())
        } else {
            SplitSize::Default
        };
        let before = has_flag(words, "-b");
        let full = has_flag(words, "-f");
        let new_pane = self.next_pane_id;
        let result = {
            let next_split_id = &mut self.next_split_id;
            let mut ids = || {
                let id = SplitId(*next_split_id);
                *next_split_id += 1;
                id
            };
            self.layout
                .split(target, axis, size, before, full, PaneId(new_pane), &mut ids)
        };
        match result {
            Ok(()) => {}
            Err(LayoutError::NoSpace) => return,
            Err(error) => panic!("split-window fixture failed: {error:?}"),
        }
        self.next_pane_id += 1;
        let insertion = if full {
            if before { 0 } else { self.panes.len() }
        } else if before {
            index
        } else {
            index + 1
        };
        self.panes.insert(insertion, new_pane);
        self.active = new_pane;
    }

    fn kill_pane(&mut self, words: &[&str]) {
        let index = target_index(flag_value(words, "-t").unwrap());
        let pane = self.panes[index];
        self.layout.remove(PaneId(pane)).unwrap();
        self.panes.remove(index);
        if self.active == pane {
            let fallback = index.saturating_sub(1).min(self.panes.len() - 1);
            self.active = self.panes[fallback];
        }
    }

    fn resize_pane(&mut self, words: &[&str]) {
        let index = target_index(flag_value(words, "-t").unwrap());
        let pane = PaneId(self.panes[index]);
        if let Some(value) = flag_value(words, "-x") {
            let size = absolute_size(value, self.layout.extent().0);
            let _ = self.layout.resize_pane_to(pane, Axis::Horizontal, size);
            return;
        }
        if let Some(value) = flag_value(words, "-y") {
            let size = absolute_size(value, self.layout.extent().1);
            let _ = self.layout.resize_pane_to(pane, Axis::Vertical, size);
            return;
        }
        for (flag, axis, sign) in [
            ("-L", Axis::Horizontal, -1),
            ("-R", Axis::Horizontal, 1),
            ("-U", Axis::Vertical, -1),
            ("-D", Axis::Vertical, 1),
        ] {
            if let Some(value) = flag_value(words, flag) {
                let amount = value.parse::<i32>().unwrap();
                let _ = self.layout.resize_pane(pane, axis, sign * amount);
                return;
            }
        }
        panic!("resize-pane fixture has no resize operation");
    }

    fn select_layout(&mut self, words: &[&str]) {
        if has_flag(words, "-E") {
            let pane = flag_value(words, "-t")
                .and_then(|target| {
                    target
                        .rsplit_once('.')
                        .map(|(_, index)| index.parse::<usize>().unwrap())
                })
                .map_or(self.active, |index| self.panes[index]);
            self.layout.spread(PaneId(pane)).unwrap();
            return;
        }
        let preset = match words[words.len() - 1] {
            "even-horizontal" => LayoutPreset::EvenHorizontal,
            "even-vertical" => LayoutPreset::EvenVertical,
            "main-horizontal" => LayoutPreset::MainHorizontal,
            "main-horizontal-mirrored" => LayoutPreset::MainHorizontalMirrored,
            "main-vertical" => LayoutPreset::MainVertical,
            "main-vertical-mirrored" => LayoutPreset::MainVerticalMirrored,
            "tiled" => LayoutPreset::Tiled,
            name => panic!("unsupported fixture preset {name}"),
        };
        let panes = self.panes.iter().copied().map(PaneId).collect::<Vec<_>>();
        let next_split_id = &mut self.next_split_id;
        let mut ids = || {
            let id = SplitId(*next_split_id);
            *next_split_id += 1;
            id
        };
        self.layout
            .apply_preset(preset, &panes, &crate::PresetOptions::default(), &mut ids);
    }

    fn resize_window(&mut self, words: &[&str]) {
        let sx = flag_value(words, "-x").unwrap().parse().unwrap();
        let sy = flag_value(words, "-y").unwrap().parse().unwrap();
        self.layout.resize(sx, sy);
    }

    fn break_pane(&mut self, words: &[&str]) {
        let index = target_index(flag_value(words, "-s").unwrap());
        let pane = self.panes[index];
        self.layout.remove(PaneId(pane)).unwrap();
        self.panes.remove(index);
        self.broken = Some(pane);
        if self.active == pane {
            let fallback = index.saturating_sub(1).min(self.panes.len() - 1);
            self.active = self.panes[fallback];
        }
    }

    fn join_pane(&mut self, words: &[&str]) {
        let index = target_index(flag_value(words, "-t").unwrap());
        let target = PaneId(self.panes[index]);
        let pane = self.broken.take().unwrap();
        let axis = if has_flag(words, "-h") {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };
        let result = {
            let next_split_id = &mut self.next_split_id;
            let mut ids = || {
                let id = SplitId(*next_split_id);
                *next_split_id += 1;
                id
            };
            self.layout.split(
                target,
                axis,
                SplitSize::Default,
                false,
                false,
                PaneId(pane),
                &mut ids,
            )
        };
        match result {
            Ok(()) => {}
            Err(LayoutError::NoSpace) => {
                self.broken = Some(pane);
                return;
            }
            Err(error) => panic!("join-pane fixture failed: {error:?}"),
        }
        self.panes.insert(index + 1, pane);
        self.active = pane;
    }
}

fn flag_value<'a>(words: &'a [&'a str], flag: &str) -> Option<&'a str> {
    words
        .windows(2)
        .find_map(|pair| (pair[0] == flag).then_some(pair[1]))
}

fn has_flag(words: &[&str], flag: &str) -> bool {
    words.contains(&flag)
}

fn target_index(target: &str) -> usize {
    target
        .rsplit_once('.')
        .map_or(0, |(_, index)| index.parse().unwrap())
}

fn split_size(value: &str) -> SplitSize {
    value.strip_suffix('%').map_or_else(
        || SplitSize::Cells(value.parse().unwrap()),
        |percent| SplitSize::Percent(percent.parse().unwrap()),
    )
}

fn absolute_size(value: &str, extent: u16) -> u16 {
    value.strip_suffix('%').map_or_else(
        || value.parse().unwrap(),
        |percent| {
            let percent = percent.parse::<u16>().unwrap();
            ((u32::from(extent) * u32::from(percent)) / 100) as u16
        },
    )
}

fn fixtures() -> Vec<Fixture<'static>> {
    include_str!("../tests/fixtures/layout-pin.txt")
        .split("\n\n")
        .filter(|block| !block.trim().is_empty())
        .map(|block| {
            let lines = block.lines().collect::<Vec<_>>();
            let name = lines[0];
            let expected_layout = lines[1].strip_prefix("  layout: ").unwrap();
            let panes_line = lines
                .iter()
                .find_map(|line| line.strip_prefix("  panes:  "))
                .unwrap();
            let expected_sizes = panes_line
                .split_whitespace()
                .enumerate()
                .map(|(expected_index, pane)| {
                    let (index, size) = pane.split_once(':').unwrap();
                    assert_eq!(index.parse::<usize>().unwrap(), expected_index);
                    let (sx, sy) = size.split_once('x').unwrap();
                    (sx.parse().unwrap(), sy.parse().unwrap())
                })
                .collect();
            let steps_start = lines.iter().position(|line| *line == "  steps:").unwrap() + 1;
            let steps = lines[steps_start..]
                .iter()
                .map(|line| line.trim())
                .filter(|line| !line.is_empty())
                .collect();
            Fixture {
                name,
                expected_layout,
                expected_sizes,
                steps,
            }
        })
        .collect()
}

#[test]
fn pinned_tmux_layout_fixtures_replay_exactly() {
    let fixtures = fixtures();
    assert_eq!(fixtures.len(), 48);
    for fixture in fixtures {
        let mut replay = Replay::new();
        for step in fixture.steps {
            replay.run(step);
        }
        assert_eq!(
            replay.layout.dump(),
            fixture.expected_layout,
            "{} layout",
            fixture.name
        );
        assert_eq!(
            replay.panes.len(),
            fixture.expected_sizes.len(),
            "{} pane count",
            fixture.name
        );
        for (index, expected) in fixture.expected_sizes.into_iter().enumerate() {
            let pane = PaneId(replay.panes[index]);
            let geometry = replay.layout.pane_geometry(pane).unwrap();
            assert_eq!(
                (geometry.sx, geometry.sy),
                expected,
                "{} pane {index}",
                fixture.name
            );
        }
    }
}

#[test]
fn pinned_tmux_layout_fixtures_parse_and_dump_exactly() {
    let fixtures = fixtures();
    assert_eq!(fixtures.len(), 48);
    for fixture in fixtures {
        let mut replay = Replay::new();
        for step in fixture.steps {
            replay.run(step);
        }
        let panes = replay.layout.panes_in_order();
        let parsed = CellLayout::parse(fixture.expected_layout).unwrap();
        assert_eq!(
            parsed.pane_count(),
            panes.len(),
            "{} pane count",
            fixture.name
        );
        let mut next_split_id = 10_000;
        let mut ids = || {
            let id = SplitId(next_split_id);
            next_split_id += 1;
            id
        };
        let rebuilt = parsed.into_layout(&panes, &mut ids);
        assert_eq!(rebuilt.dump(), fixture.expected_layout, "{}", fixture.name);
    }
}
