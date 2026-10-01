use std::sync::LazyLock;

#[cfg(any(windows, test))]
use std::io::{self, IoSlice, Write};

use super::*;

pub(super) static ATTACH_DEDUP: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("ZZ_PERF_ATTACH_DEDUP").is_none_or(|value| value != "0"));

pub(super) static ATTACH_PRESIZE: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("ZZ_PERF_ATTACH_PRESIZE").is_none_or(|value| value != "0"));

pub(super) static ATTACH_BATCH: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("ZZ_PERF_ATTACH_BATCH").is_none_or(|value| value != "0"));

pub(super) static BATCHED_WRITES: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("ZZ_PERF_WRITEV").is_none_or(|value| value != "0"));

pub(super) const MAX_BATCHED_WRITE_BYTES: usize = 256 * 1024;

#[cfg(windows)]
const INBOUND_BUFFER_BYTES: usize = 8 * 1024;

#[cfg(windows)]
pub(super) fn inbound_reader<S: io::Read>(stream: S) -> io::BufReader<S> {
    io::BufReader::with_capacity(
        if *BATCHED_WRITES {
            INBOUND_BUFFER_BYTES
        } else {
            0
        },
        stream,
    )
}

pub(super) fn log_knobs() {
    log::info!(
        target: "zz_daemon::perf",
        "attach knobs: ZZ_PERF_ATTACH_DEDUP={} ZZ_PERF_ATTACH_BATCH={} ZZ_PERF_ATTACH_PRESIZE={} ZZ_PERF_WRITEV={}",
        u8::from(*ATTACH_DEDUP),
        u8::from(*ATTACH_BATCH),
        u8::from(*ATTACH_PRESIZE),
        u8::from(*BATCHED_WRITES),
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ResyncScope {
    Attach,
    Full,
}

impl ResyncScope {
    pub(super) fn sends_everything(self) -> bool {
        self == Self::Full || !*ATTACH_DEDUP
    }
}

pub(super) fn terminal_option_capabilities(engine: &MuxEngine) -> [String; 2] {
    [
        format!(
            "{SERVER_OPTION_CAPABILITY_PREFIX}extended-keys={}",
            engine.extended_keys()
        ),
        format!(
            "{SERVER_OPTION_CAPABILITY_PREFIX}focus-events={}",
            if engine.focus_events() { "on" } else { "off" }
        ),
    ]
}

pub(super) fn client_cell_fact(capabilities: &[String]) -> Option<(u32, u32)> {
    capabilities.iter().find_map(|capability| {
        let value = capability.strip_prefix(ClientHello::CLIENT_CELL_CAPABILITY_PREFIX)?;
        let (width, height) = value.split_once('x')?;
        let width = width.parse::<u32>().ok().filter(|width| *width > 0)?;
        let height = height.parse::<u32>().ok().filter(|height| *height > 0)?;
        Some((width, height))
    })
}

pub(super) fn presize_client_terminals(
    inner: &mut ServerState,
    client: ClientId,
    session: SessionId,
) -> BTreeSet<PaneId> {
    let mut seeded = BTreeSet::new();
    if !*ATTACH_PRESIZE
        || inner.client(client).and_then(|c| c.kind) != Some(ClientKind::Interactive)
        || !inner.client(client).is_some_and(|c| c.has_terminal)
    {
        return seeded;
    }
    let Some((cell_width_px, cell_height_px)) = inner.client(client).and_then(|c| c.cell_pixels)
    else {
        return seeded;
    };
    let Some(session_state) = inner.engine.state.sessions.get(&session) else {
        return seeded;
    };
    let window = client_focused_window(inner, client, session_state);
    let Some((columns, rows)) = inner
        .client(client)
        .and_then(|c| c.size.as_ref())
        .and_then(|_| interactive_client_window_extent(inner, client, session, window))
    else {
        return seeded;
    };
    let panes = inner
        .client(client)
        .and_then(|c| c.visible_terminals.as_ref())
        .into_iter()
        .flatten()
        .copied()
        .filter(|pane| inner.terminals.contains_key(pane))
        .filter(|pane| inner.engine.state.window_for_pane(*pane) == Some(window))
        .filter(|pane| {
            inner
                .terminal_geometries
                .get(pane)
                .is_none_or(|geometries| !geometries.contains_key(&client))
        })
        .filter_map(|pane| {
            inner
                .engine
                .pane_geometry_at_window_extent(pane, columns, rows)
                .map(|geometry| (pane, geometry))
        })
        .collect::<Vec<_>>();
    for (pane, (columns, rows)) in panes {
        inner.terminal_geometries.entry(pane).or_default().insert(
            client,
            TerminalGeometry {
                columns,
                rows,
                cell_width_px,
                cell_height_px,
            },
        );
        seeded.insert(pane);
    }
    seeded
}

pub(super) fn attach_frame_superseded(
    inner: &ServerState,
    pane: PaneId,
    viewport: &TerminalViewport,
) -> bool {
    if inner.engine.state.pane(pane).is_none_or(|pane| pane.dead) {
        return false;
    }
    terminal_resize_for_pane(inner, pane).is_some_and(|(_, geometry)| {
        (geometry.columns.max(1), geometry.rows.max(1)) != (viewport.columns, viewport.rows)
    })
}

#[cfg(any(windows, test))]
pub(super) fn write_frames(stream: &mut impl Write, frames: &[impl AsRef<[u8]>]) -> io::Result<()> {
    let mut slices = frames
        .iter()
        .map(|frame| IoSlice::new(frame.as_ref()))
        .collect::<Vec<_>>();
    let mut remaining = &mut slices[..];
    while !remaining.is_empty() {
        match stream.write_vectored(remaining) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write whole outbound batch",
                ));
            }
            Ok(written) => IoSlice::advance_slices(&mut remaining, written),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    stream.flush()
}

impl Shared {
    pub(super) fn hold_attach_terminals(&self, client: ClientId) -> AttachHold {
        let subscriber =
            self.read_client(client, |c| c.and_then(|c| c.subscriber.as_ref()).cloned());
        if let Some(subscriber) = &subscriber {
            subscriber.hold_terminals();
        }
        AttachHold(subscriber)
    }

    pub(super) fn request_full(&self, client: ClientId, pane: PaneId, outbound: &OutboundMailbox) {
        outbound.forget_delivered_terminal(pane);
        self.send_full(client, pane, outbound);
    }
}

pub(super) struct AttachHold(Option<Arc<OutboundMailbox>>);

impl AttachHold {
    pub(super) fn release(&mut self) {
        if let Some(held) = self.0.take() {
            held.release_terminals();
        }
    }
}

impl Drop for AttachHold {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(windows)]
pub(super) struct WriterThread(crossbeam_channel::Receiver<()>);

#[cfg(windows)]
impl WriterThread {
    pub(super) fn join(self) -> Result<(), ()> {
        self.0.recv().map_err(drop)
    }
}

#[cfg(windows)]
pub(super) fn spawn_writer(
    threads: &Arc<exec::ConnectionThreads>,
    client: ClientId,
    write: impl FnOnce() + Send + 'static,
) -> std::io::Result<WriterThread> {
    let (done, finished) = crossbeam_channel::bounded(1);
    let job = move || {
        write();
        let _ = done.send(());
    };
    if *BATCHED_WRITES {
        threads.run(Box::new(job))?;
    } else {
        thread::Builder::new()
            .name(format!("zz-client-writer-{}", client.0))
            .spawn(job)?;
    }
    Ok(WriterThread(finished))
}
