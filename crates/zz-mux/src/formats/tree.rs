use super::*;

thread_local! {
    static BORROWED_OVERRIDE: Cell<Option<bool>> = const { Cell::new(None) };
}

pub(super) fn borrowed_formats() -> bool {
    BORROWED_OVERRIDE.with(Cell::get).unwrap_or(true)
}

#[doc(hidden)]
pub fn with_borrowed_formats<R>(enabled: bool, body: impl FnOnce() -> R) -> R {
    struct Restore(Option<bool>);
    impl Drop for Restore {
        fn drop(&mut self) {
            BORROWED_OVERRIDE.with(|value| value.set(self.0));
        }
    }
    let _restore = Restore(BORROWED_OVERRIDE.with(|value| value.replace(Some(enabled))));
    body()
}

#[derive(Clone)]
pub(super) struct FormatTree<'e> {
    pub(super) engine: &'e MuxEngine,
    pub(super) session: Option<SessionId>,
    pub(super) window: Option<WindowId>,
    pub(super) pane: Option<PaneId>,
    pub(super) format_client: FormatClient,
}

impl std::fmt::Debug for FormatTree<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FormatTree")
            .field("session", &self.session)
            .field("window", &self.window)
            .field("pane", &self.pane)
            .finish_non_exhaustive()
    }
}

impl FormatTree<'_> {
    pub(super) fn resolve(
        &self,
        scope: FormatScope,
        backing: FormatBacking,
        format_type: FormatType,
    ) -> Cow<'_, str> {
        let engine = self.engine;
        let state = &engine.state;
        let session = self.session.and_then(|id| state.sessions.get(&id));
        let window = self.window.and_then(|id| state.windows.get(&id));
        let pane = window.and_then(|window| self.pane.and_then(|id| window.panes.get(&id)));
        let available = match scope {
            FormatScope::Server | FormatScope::Buffer | FormatScope::Client => true,
            FormatScope::Session => session.is_some(),
            FormatScope::Window => window.is_some(),
            FormatScope::Pane | FormatScope::Terminal => pane.is_some(),
        };
        if !available {
            return Cow::Borrowed("");
        }
        let boolean = |value| Cow::Borrowed(bool_string(value));
        match backing {
            FormatBacking::Empty
            | FormatBacking::StatusHook
            | FormatBacking::ConfigFiles
            | FormatBacking::SessionAttachedList
            | FormatBacking::WindowActiveClientsList => Cow::Borrowed(""),
            FormatBacking::Zero
            | FormatBacking::SessionAttached
            | FormatBacking::SessionManyAttached
            | FormatBacking::WindowActiveClients
            | FormatBacking::WindowLinked => Cow::Borrowed("0"),
            FormatBacking::One | FormatBacking::WindowLinkedSessions | FormatBacking::PaneZ => {
                Cow::Borrowed("1")
            }
            FormatBacking::Host => Cow::Borrowed(engine.format_host()),
            FormatBacking::HostShort => Cow::Borrowed(engine.format_host_short()),
            FormatBacking::NextSessionId => Cow::Owned(state.next_session_id().to_string()),
            FormatBacking::Pid => Cow::Owned(engine.format_pid().to_string()),
            FormatBacking::ServerSessions => Cow::Owned(state.sessions.len().to_string()),
            FormatBacking::SocketPath => Cow::Borrowed(engine.format_socket_path()),
            FormatBacking::StartTime => optional_display(
                i64::try_from(engine.format_start_time())
                    .ok()
                    .filter(|time| *time != 0),
            ),
            FormatBacking::Uid => Cow::Borrowed(engine.format_uid()),
            FormatBacking::User => Cow::Borrowed(engine.format_user()),
            FormatBacking::Version => Cow::Borrowed(
                CommandSpec::TMUX_VERSION_OUTPUT
                    .strip_prefix("tmux ")
                    .expect("tmux version prefix"),
            ),
            FormatBacking::ActiveWindowIndex => optional_display(session.and_then(|session| {
                state
                    .windows
                    .get(&session.active_window)
                    .map(|window| window.index)
            })),
            FormatBacking::LastWindowIndex => optional_display(session.and_then(|session| {
                session
                    .windows
                    .iter()
                    .filter_map(|id| state.windows.get(id).map(|window| window.index))
                    .max()
            })),
            FormatBacking::SessionId => optional_display(self.session),
            FormatBacking::SessionName => {
                Cow::Borrowed(session.map_or("", |session| session.name.as_str()))
            }
            FormatBacking::SessionPath => Cow::Borrowed(
                self.session
                    .and_then(|id| state.session_working_directory(id))
                    .and_then(|path| path.to_str())
                    .unwrap_or_default(),
            ),
            FormatBacking::SessionActivity => {
                optional_display(session.and_then(|session| session.activity))
            }
            FormatBacking::SessionCreated => {
                optional_display(session.and_then(|session| session.created))
            }
            FormatBacking::SessionActive => optional_bool(
                self.session
                    .and_then(|id| self.format_client.session_active(id)),
            ),
            FormatBacking::SessionWindows => {
                optional_display(session.map(|session| session.windows.len()))
            }
            FormatBacking::SessionFormat => boolean(format_type == FormatType::Session),
            FormatBacking::SessionMarked => boolean(
                state
                    .marked_pane()
                    .and_then(|pane| state.window_for_pane(pane))
                    .and_then(|id| state.windows.get(&id))
                    .map(|window| window.session)
                    == self.session,
            ),
            FormatBacking::SessionBell => boolean(
                session
                    .and_then(|session| session.windows.first())
                    .and_then(|id| state.windows.get(id))
                    .is_some_and(|window| window.panes.values().any(|pane| pane.bell)),
            ),
            FormatBacking::SessionAlert | FormatBacking::SessionAlerts => {
                let mut windows = session
                    .into_iter()
                    .flat_map(|session| &session.windows)
                    .filter_map(|id| state.windows.get(id))
                    .filter_map(|window| {
                        let bell = window.panes.values().any(|pane| pane.bell);
                        (window.activity_flag || bell || window.silence_flag).then_some((
                            window.index,
                            window.activity_flag,
                            bell,
                            window.silence_flag,
                        ))
                    })
                    .collect::<Vec<_>>();
                windows.sort_unstable_by_key(|(index, ..)| *index);
                if backing == FormatBacking::SessionAlert {
                    let mut value = String::new();
                    for (_, activity, bell, silence) in windows {
                        for (flag, present) in [('#', activity), ('!', bell), ('~', silence)] {
                            if present && !value.contains(flag) {
                                value.push(flag);
                            }
                        }
                    }
                    Cow::Owned(value)
                } else {
                    Cow::Owned(
                        windows
                            .into_iter()
                            .map(|(index, activity, bell, silence)| {
                                let mut entry = index.to_string();
                                for (flag, present) in
                                    [('#', activity), ('!', bell), ('~', silence)]
                                {
                                    if present {
                                        entry.push(flag);
                                    }
                                }
                                entry
                            })
                            .collect::<Vec<_>>()
                            .join(","),
                    )
                }
            }
            FormatBacking::SessionStack => {
                let mut stack = session
                    .and_then(|session| state.windows.get(&session.active_window))
                    .map(|window| window.index.to_string())
                    .into_iter()
                    .collect::<Vec<_>>();
                if let Some(last) = session
                    .and_then(crate::model::Session::last_window)
                    .and_then(|id| state.windows.get(&id))
                {
                    stack.push(last.index.to_string());
                }
                Cow::Owned(stack.join(","))
            }
            FormatBacking::WindowId => optional_display(self.window),
            FormatBacking::WindowIndex => optional_display(window.map(|window| window.index)),
            FormatBacking::WindowName => {
                Cow::Borrowed(window.map_or("", |window| window.name.as_str()))
            }
            FormatBacking::WindowPanes => optional_display(window.map(|window| window.panes.len())),
            FormatBacking::WindowWidth => {
                optional_display(window.map(|window| window.layout.extent().0))
            }
            FormatBacking::WindowHeight => {
                optional_display(window.map(|window| window.layout.extent().1))
            }
            FormatBacking::WindowManualWidth | FormatBacking::WindowManualHeight => {
                optional_display(
                    window
                        .filter(|window| engine.window_size(window.id) == WindowSize::Manual)
                        .map(|window| {
                            if backing == FormatBacking::WindowManualWidth {
                                window.manual_extent.0
                            } else {
                                window.manual_extent.1
                            }
                        }),
                )
            }
            FormatBacking::WindowLayout => Cow::Owned(
                window
                    .map(|window| {
                        window.layout_string(LayoutFormat::V2, state.pane_base_index(window.id))
                    })
                    .unwrap_or_default(),
            ),
            FormatBacking::WindowVisibleLayout => Cow::Owned(
                window
                    .map(|window| {
                        window.visible_layout_string(
                            LayoutFormat::V2,
                            state.pane_base_index(window.id),
                        )
                    })
                    .unwrap_or_default(),
            ),
            FormatBacking::WindowActive => {
                boolean(session.is_some_and(|session| Some(session.active_window) == self.window))
            }
            FormatBacking::WindowZoomed => {
                boolean(window.is_some_and(|window| window.zoomed_pane.is_some()))
            }
            FormatBacking::WindowBell => {
                boolean(window.is_some_and(|window| window.panes.values().any(|pane| pane.bell)))
            }
            FormatBacking::WindowActivity => {
                optional_display(window.and_then(|window| window.activity_time))
            }
            FormatBacking::WindowActivityFlag => {
                boolean(window.is_some_and(|window| window.activity_flag))
            }
            FormatBacking::WindowSilenceFlag => {
                boolean(window.is_some_and(|window| window.silence_flag))
            }
            FormatBacking::WindowLast => {
                boolean(session.is_some_and(|session| session.last_window() == self.window))
            }
            FormatBacking::WindowStart => boolean(
                session.is_some_and(|session| session.windows.first().copied() == self.window),
            ),
            FormatBacking::WindowEnd => boolean(
                session.is_some_and(|session| session.windows.last().copied() == self.window),
            ),
            FormatBacking::WindowStackIndex => Cow::Owned(
                usize::from(session.is_some_and(|session| session.last_window() == self.window))
                    .to_string(),
            ),
            FormatBacking::WindowLinkedSessionsList => {
                Cow::Borrowed(session.map_or("", |session| session.name.as_str()))
            }
            FormatBacking::WindowActiveSessions => {
                boolean(session.is_some_and(|session| Some(session.active_window) == self.window))
            }
            FormatBacking::WindowActiveSessionsList => Cow::Borrowed(
                session
                    .filter(|session| Some(session.active_window) == self.window)
                    .map_or("", |session| session.name.as_str()),
            ),
            FormatBacking::WindowMarkedFlag => boolean(
                state
                    .marked_pane()
                    .and_then(|pane| state.window_for_pane(pane))
                    == self.window,
            ),
            FormatBacking::WindowFormat => boolean(format_type == FormatType::Window),
            FormatBacking::WindowFlags | FormatBacking::WindowRawFlags => {
                let mut flags = String::new();
                for (flag, present) in [
                    ('#', window.is_some_and(|window| window.activity_flag)),
                    (
                        '!',
                        window.is_some_and(|window| window.panes.values().any(|pane| pane.bell)),
                    ),
                    ('~', window.is_some_and(|window| window.silence_flag)),
                    (
                        '*',
                        session.is_some_and(|session| Some(session.active_window) == self.window),
                    ),
                    (
                        '-',
                        session.is_some_and(|session| session.last_window() == self.window),
                    ),
                    (
                        'M',
                        state
                            .marked_pane()
                            .and_then(|pane| state.window_for_pane(pane))
                            == self.window,
                    ),
                    (
                        'Z',
                        window.is_some_and(|window| window.zoomed_pane.is_some()),
                    ),
                ] {
                    if present {
                        flags.push(flag);
                    }
                }
                Cow::Owned(if backing == FormatBacking::WindowFlags {
                    flags.replace('#', "##")
                } else {
                    flags
                })
            }
            FormatBacking::HistoryLimit => optional_display(
                window.map(|window| engine.history_limit_for_session(window.session)),
            ),
            FormatBacking::PaneId => optional_display(self.pane),
            FormatBacking::PaneIndex => optional_display(window.and_then(|window| {
                self.pane
                    .and_then(|pane| engine.pane_index(window.id, pane))
            })),
            FormatBacking::PaneActive => {
                boolean(window.is_some_and(|window| Some(window.active_pane) == self.pane))
            }
            FormatBacking::PaneDead => boolean(pane.is_some_and(|pane| pane.dead)),
            FormatBacking::PaneDeadStatus => {
                optional_display(pane.and_then(|pane| pane.dead_status))
            }
            FormatBacking::PaneDeadTime => optional_display(
                pane.and_then(|pane| pane.dead_time)
                    .and_then(|time| i64::try_from(time).ok()),
            ),
            FormatBacking::PaneInputOff => boolean(pane.is_some_and(|pane| pane.input_off)),
            FormatBacking::PaneMarked => boolean(state.marked_pane() == self.pane),
            FormatBacking::PaneMarkedSet => boolean(state.marked_pane().is_some()),
            FormatBacking::PaneLast => {
                boolean(window.is_some_and(|window| window.last_pane() == self.pane))
            }
            FormatBacking::PaneSynchronized => boolean(
                window
                    .and_then(|window| {
                        self.pane
                            .and_then(|pane| state.pane_synchronize_panes_in(window.id, pane).ok())
                    })
                    .unwrap_or_default(),
            ),
            FormatBacking::PaneTitle => Cow::Borrowed(pane.map_or("", |pane| pane.title.as_str())),
            FormatBacking::PaneZoomed => {
                boolean(window.is_some_and(|window| window.zoomed_pane == self.pane))
            }
            FormatBacking::PaneFormat => boolean(format_type == FormatType::Pane),
            FormatBacking::PaneFlags => {
                let mut flags = String::new();
                for (flag, present) in [
                    (
                        '*',
                        window.is_some_and(|window| Some(window.active_pane) == self.pane),
                    ),
                    (
                        '-',
                        window.is_some_and(|window| window.last_pane() == self.pane),
                    ),
                    (
                        'Z',
                        window.is_some_and(|window| window.zoomed_pane == self.pane),
                    ),
                ] {
                    if present {
                        flags.push(flag);
                    }
                }
                Cow::Owned(flags)
            }
            FormatBacking::PaneStartCommand | FormatBacking::PaneStartCommandList => Cow::Owned(
                self.pane
                    .and_then(|pane| engine.pane_start_command(pane))
                    .map(|command| {
                        command
                            .iter()
                            .map(|argument| {
                                if backing == FormatBacking::PaneStartCommand {
                                    quote_argument(argument)
                                } else {
                                    quote_single(argument.as_bytes()).to_string()
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default(),
            ),
            FormatBacking::PaneCurrentCommand => {
                let live = self
                    .pane
                    .and_then(|pane| engine.pane_runtime_facts(pane))
                    .filter(|_| !pane.is_some_and(|pane| pane.dead))
                    .map(|facts| facts.current_command.as_str())
                    .filter(|command| !command.is_empty());
                match (live, self.pane, pane) {
                    (Some(command), _, _) => Cow::Borrowed(command),
                    (None, Some(id), Some(pane)) if matches!(pane.kind, PaneKind::Terminal) => {
                        Cow::Owned(engine.pane_command_fallback(id))
                    }
                    _ => Cow::Borrowed(""),
                }
            }
            FormatBacking::PaneCurrentPath
            | FormatBacking::PanePath
            | FormatBacking::PaneStartPath
            | FormatBacking::PanePid
            | FormatBacking::PaneTty
            | FormatBacking::PaneDeadSignal => {
                if let Some(facts) = self.pane.and_then(|pane| engine.pane_runtime_facts(pane)) {
                    match backing {
                        FormatBacking::PaneCurrentPath => {
                            Cow::Borrowed(if pane.is_some_and(|pane| pane.dead) {
                                ""
                            } else {
                                &facts.current_path
                            })
                        }
                        FormatBacking::PanePath => Cow::Borrowed(&facts.reported_path),
                        FormatBacking::PaneStartPath => Cow::Borrowed(&facts.start_path),
                        FormatBacking::PanePid => optional_display(facts.pid),
                        FormatBacking::PaneTty => Cow::Borrowed(&facts.tty),
                        FormatBacking::PaneDeadSignal => {
                            Cow::Borrowed(if pane.is_some_and(|pane| pane.dead) {
                                &facts.dead_signal
                            } else {
                                ""
                            })
                        }
                        _ => unreachable!(),
                    }
                } else if matches!(
                    backing,
                    FormatBacking::PaneCurrentPath | FormatBacking::PaneStartPath
                ) {
                    match pane.map(|pane| &pane.kind) {
                        Some(PaneKind::Agent(agent)) => agent
                            .cwd
                            .as_deref()
                            .map_or(Cow::Borrowed(""), |path| path.to_string_lossy()),
                        Some(PaneKind::Editor(editor)) => Cow::Borrowed(&editor.cwd),
                        _ => Cow::Borrowed(""),
                    }
                } else {
                    Cow::Borrowed("")
                }
            }
            FormatBacking::PaneUnzoomedHeight | FormatBacking::PaneUnzoomedWidth => {
                let Some(cell) = window.zip(self.pane).and_then(|(window, pane)| {
                    window
                        .layout
                        .pane_geometry_with_border(pane, engine.pane_border_status(window.id))
                }) else {
                    return Cow::Borrowed("");
                };
                optional_display(Some(if backing == FormatBacking::PaneUnzoomedWidth {
                    cell.sx
                } else {
                    cell.sy
                }))
            }
            FormatBacking::PaneAtBottom
            | FormatBacking::PaneAtLeft
            | FormatBacking::PaneAtRight
            | FormatBacking::PaneAtTop
            | FormatBacking::PaneBottom
            | FormatBacking::PaneHeight
            | FormatBacking::PaneLeft
            | FormatBacking::PaneRight
            | FormatBacking::PaneTop
            | FormatBacking::PaneWidth
            | FormatBacking::PaneX
            | FormatBacking::PaneY => {
                let Some(window) = window else {
                    return Cow::Borrowed("");
                };
                let border = engine.pane_border_status(window.id);
                let Some(cell) = self
                    .pane
                    .and_then(|pane| window.displayed_pane_cell(pane, border))
                else {
                    return Cow::Borrowed("");
                };
                let (width, height) = window.layout.extent();
                match backing {
                    FormatBacking::PaneWidth => optional_display(Some(cell.sx)),
                    FormatBacking::PaneHeight => optional_display(Some(cell.sy)),
                    FormatBacking::PaneLeft | FormatBacking::PaneX => {
                        optional_display(Some(cell.xoff))
                    }
                    FormatBacking::PaneTop | FormatBacking::PaneY => {
                        optional_display(Some(cell.yoff))
                    }
                    FormatBacking::PaneRight => optional_display(
                        cell.xoff
                            .checked_add(cell.sx)
                            .and_then(|right| right.checked_sub(1)),
                    ),
                    FormatBacking::PaneBottom => optional_display(
                        cell.yoff
                            .checked_add(cell.sy)
                            .and_then(|bottom| bottom.checked_sub(1)),
                    ),
                    FormatBacking::PaneAtLeft => boolean(cell.xoff == 0),
                    FormatBacking::PaneAtTop => {
                        boolean(cell.yoff == u16::from(border == PaneBorderStatus::Top))
                    }
                    FormatBacking::PaneAtRight => {
                        boolean(cell.xoff.saturating_add(cell.sx) == width)
                    }
                    FormatBacking::PaneAtBottom => boolean(if border == PaneBorderStatus::Bottom {
                        cell.yoff.saturating_add(cell.sy) == height.saturating_sub(1) && height > 0
                    } else {
                        cell.yoff.saturating_add(cell.sy) == height
                    }),
                    _ => unreachable!(),
                }
            }
        }
    }
}
