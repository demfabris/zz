use std::{
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use zz_daemon::CommandClient;
use zz_protocol::CommandInvocation;

use super::TrayEvent;

const SESSIONS: &str = "#{session_name}\t#{session_windows}\t#{session_attached}";
const PANES: &str = "#{pane_id}\t#{session_name}\t#{window_index}\t#{pane_index}\t#{pane_kind}\t#{pane_title}\t#{agent_state}\t#{agent_pending_permission}";
const START_TIME: &str = "#{start_time}";
static MENU_FETCH: AtomicBool = AtomicBool::new(false);
static POLL_FETCH: AtomicBool = AtomicBool::new(false);

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Session {
    pub name: String,
    pub windows: usize,
    pub attached: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Attention {
    pub pane: String,
    pub session: String,
    pub window: usize,
    pub title: String,
    pub permission: bool,
}

#[derive(Debug)]
pub(super) struct TrayFacts {
    pub sessions: Vec<Session>,
    pub attention: Vec<Attention>,
    pub start_time: u64,
    pub update_available: bool,
}

#[derive(Clone)]
pub(super) struct Source {
    pub socket: PathBuf,
    pub server_id: u64,
    pub active: Arc<AtomicBool>,
}

pub(super) enum MenuEntry {
    Separator,
    Item {
        title: String,
        hint: String,
        action: Option<TrayEvent>,
    },
}

impl MenuEntry {
    pub fn label(&self) -> String {
        match self {
            Self::Separator => String::new(),
            Self::Item { title, hint, .. } if !hint.is_empty() => format!("{title} · {hint}"),
            Self::Item { title, .. } => title.clone(),
        }
    }
}

impl Source {
    pub fn menu(&self) -> Vec<MenuEntry> {
        let facts = fetch(&self.socket, self.server_id, Duration::from_millis(300)).ok();
        menu(facts.as_ref(), self.active.load(Ordering::Acquire))
    }
}

fn item(title: impl Into<String>, hint: impl Into<String>, action: Option<TrayEvent>) -> MenuEntry {
    MenuEntry::Item {
        title: title.into(),
        hint: hint.into(),
        action,
    }
}

fn menu(facts: Option<&TrayFacts>, active: bool) -> Vec<MenuEntry> {
    let mut menu = vec![
        item(
            if active { "Hide zz" } else { "Show zz" },
            "",
            Some(TrayEvent::Toggle),
        ),
        MenuEntry::Separator,
    ];
    if let Some(facts) = facts {
        if !facts.attention.is_empty() {
            menu.push(item("Needs attention", "", None));
            for pane in &facts.attention {
                menu.push(item(
                    format!("{} in {}:{}", pane.title, pane.session, pane.window),
                    if pane.permission {
                        "needs approval"
                    } else {
                        "waiting"
                    },
                    Some(TrayEvent::FocusPane(pane.pane.clone())),
                ));
            }
            menu.push(MenuEntry::Separator);
        }
        menu.push(item("Sessions", "", None));
        for session in &facts.sessions {
            menu.push(item(
                &session.name,
                format!(
                    "{} windows{}",
                    session.windows,
                    if session.attached { " · attached" } else { "" }
                ),
                Some(TrayEvent::SwitchSession(session.name.clone())),
            ));
        }
    }
    menu.push(item("New Session", "", Some(TrayEvent::NewSession)));
    menu.push(MenuEntry::Separator);
    if let Some(facts) = facts {
        let uptime = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(facts.start_time);
        let duration = if uptime >= 86400 {
            format!("{}d {}h", uptime / 86400, uptime % 86400 / 3600)
        } else if uptime >= 3600 {
            format!("{}h {}m", uptime / 3600, uptime % 3600 / 60)
        } else {
            format!("{}m", uptime / 60)
        };
        menu.push(item(
            format!("zz {} · up {duration}", env!("CARGO_PKG_VERSION")),
            "",
            None,
        ));
        if facts.update_available {
            menu.push(item(
                "Restart Daemon to Update…",
                "",
                Some(TrayEvent::RestartDaemon),
            ));
        }
    } else {
        menu.push(item("Daemon unavailable", "", None));
    }
    menu.extend([
        MenuEntry::Separator,
        item("Settings…", "", Some(TrayEvent::OpenSettings)),
        item("Open Logs", "", Some(TrayEvent::OpenLogs)),
        MenuEntry::Separator,
        item("Quit and Stop Sessions", "", Some(TrayEvent::Quit)),
    ]);
    menu
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid tray facts")
}

fn parse_sessions(output: &str) -> io::Result<Vec<Session>> {
    output
        .lines()
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            let [name, windows, attached] = fields.as_slice() else {
                return Err(invalid());
            };
            if name.is_empty() {
                return Err(invalid());
            }
            Ok(Session {
                name: (*name).into(),
                windows: windows.parse().map_err(|_| invalid())?,
                attached: attached.parse::<usize>().map_err(|_| invalid())? > 0,
            })
        })
        .collect()
}

fn parse_attention(output: &str) -> io::Result<Vec<Attention>> {
    let mut attention = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for line in output.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() < 8 {
            return Err(invalid());
        }
        let state = fields[fields.len() - 2];
        let permission = fields[fields.len() - 1];
        if fields[4] != "agent" || (state != "blocked" && permission != "1") {
            continue;
        }
        fields[0]
            .strip_prefix('%')
            .ok_or_else(invalid)?
            .parse::<u64>()
            .map_err(|_| invalid())?;
        if !seen.insert(fields[0]) {
            continue;
        }
        attention.push(Attention {
            pane: fields[0].into(),
            session: fields[1].into(),
            window: fields[2].parse().map_err(|_| invalid())?,
            title: fields[5..fields.len() - 2].join(" "),
            permission: permission == "1",
        });
    }
    Ok(attention)
}

fn parse_start_time(output: &str) -> io::Result<u64> {
    output.trim().parse().map_err(|_| invalid())
}

fn bounded<T: Send + 'static>(
    flag: &'static AtomicBool,
    budget: Duration,
    work: impl FnOnce(Instant) -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    if flag
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "tray fetch in progress",
        ));
    }
    let deadline = Instant::now() + budget;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("zz-tray-facts".into())
        .spawn(move || {
            struct Release(&'static AtomicBool);
            impl Drop for Release {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::Release);
                }
            }
            let _release = Release(flag);
            let _ = sender.send(work(deadline));
        });
    if let Err(error) = worker {
        flag.store(false, Ordering::Release);
        return Err(error);
    }
    receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "tray fetch timed out"))?
}

fn connect(socket: &Path, server_id: u64) -> io::Result<CommandClient> {
    let client = CommandClient::connect(socket).map_err(io::Error::other)?;
    if client.server_hello().server_id != server_id {
        return Err(io::Error::other("tray daemon changed"));
    }
    Ok(client)
}

fn execute(
    client: &mut CommandClient,
    name: &str,
    args: &[&str],
    deadline: Instant,
) -> io::Result<String> {
    if Instant::now() >= deadline {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "tray fetch timed out",
        ));
    }
    client
        .execute(CommandInvocation::new(name, args.iter().copied()))
        .map_err(io::Error::other)
}

pub(super) fn fetch(socket: &Path, server_id: u64, budget: Duration) -> io::Result<TrayFacts> {
    let socket = socket.to_path_buf();
    bounded(&MENU_FETCH, budget, move |deadline| {
        let mut client = connect(&socket, server_id)?;
        let sessions = parse_sessions(&execute(
            &mut client,
            "list-sessions",
            &["-F", SESSIONS],
            deadline,
        )?)?;
        let attention = parse_attention(&execute(
            &mut client,
            "list-panes",
            &["-a", "-F", PANES],
            deadline,
        )?)?;
        let start_time = parse_start_time(&execute(
            &mut client,
            "display-message",
            &["-p", START_TIME],
            deadline,
        )?)?;
        let update_available =
            disk_version(deadline).is_some_and(|disk| disk != env!("CARGO_PKG_VERSION"));
        Ok(TrayFacts {
            sessions,
            attention,
            start_time,
            update_available,
        })
    })
}

pub(super) fn attention_count(
    socket: &Path,
    server_id: u64,
    budget: Duration,
) -> io::Result<usize> {
    let socket = socket.to_path_buf();
    bounded(&POLL_FETCH, budget, move |deadline| {
        let mut client = connect(&socket, server_id)?;
        Ok(parse_attention(&execute(
            &mut client,
            "list-panes",
            &["-a", "-F", PANES],
            deadline,
        )?)?
        .len())
    })
}

fn disk_version(deadline: Instant) -> Option<String> {
    if Instant::now() >= deadline {
        return None;
    }
    let mut child = Command::new(std::env::current_exe().ok()?)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let output = child.wait_with_output().ok()?;
                let text = String::from_utf8(output.stdout).ok()?;
                return text.trim().strip_prefix("zz ").map(str::to_owned);
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(2)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_parse_attachment_counts_and_empty_output() {
        assert_eq!(
            parse_sessions("work\t2\t3\nsolo\t1\t0\n").unwrap(),
            vec![
                Session {
                    name: "work".into(),
                    windows: 2,
                    attached: true
                },
                Session {
                    name: "solo".into(),
                    windows: 1,
                    attached: false
                }
            ]
        );
        assert!(parse_sessions("").unwrap().is_empty());
        assert!(parse_sessions("work\ttwo\t0\n").is_err());
    }

    #[test]
    fn attention_includes_blocked_and_permission_agents_once() {
        let panes = "%1\twork\t0\t0\tagent\tBuild\tblocked\t0\n%2\twork\t1\t0\tagent\tReview\tidle\t1\n%3\twork\t1\t1\tagent\tBoth\tblocked\t1\n%4\twork\t1\t2\tagent\tBusy\tworking\t0\n%5\twork\t2\t0\tterminal\tShell\t\t\n";
        let rows = parse_attention(panes).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0],
            Attention {
                pane: "%1".into(),
                session: "work".into(),
                window: 0,
                title: "Build".into(),
                permission: false
            }
        );
        assert!(rows[1].permission && rows[2].permission);
        assert!(parse_attention("").unwrap().is_empty());
        assert!(parse_attention("broken").is_err());
        assert_eq!(parse_attention("%1\twork\t0\t0\tagent\tBuild\tblocked\t0\n%1\tlinked\t1\t0\tagent\tBuild\tblocked\t0\n").unwrap().len(), 1);
    }

    #[test]
    fn start_time_is_a_unix_timestamp() {
        assert_eq!(parse_start_time("1750000000\n").unwrap(), 1_750_000_000);
        assert!(parse_start_time("yesterday").is_err());
        assert!(parse_start_time("").is_err());
    }

    #[test]
    fn unavailable_facts_keep_static_actions() {
        let rows = menu(None, false);
        assert!(rows.iter().any(|row| matches!(
            row,
            MenuEntry::Item {
                action: Some(TrayEvent::NewSession),
                ..
            }
        )));
        assert!(!rows.iter().any(|row| matches!(row, MenuEntry::Item { title, .. } if title == "Sessions" || title == "Needs attention")));
    }
}
