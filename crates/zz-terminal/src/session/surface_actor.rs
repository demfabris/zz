use super::*;

pub(super) struct SurfaceActor<'a, 'b> {
    terminal: Terminal<'a, 'b>,
    geometry: Geometry,
    frames: Frames<'a>,
    active_views: ActiveTerminalViews,
    inactive_views: InactiveTerminalViews,
    word_separators: WordSeparators,
    wrap_search: bool,
    mode_keys_vi: bool,
    reported_color_scheme: Rc<Cell<ColorScheme>>,
    max_scrollback: usize,
    status: SessionStatus,
    pending_commands: VecDeque<Command>,
    pending_copy_source: Option<Box<CapturedCopySource>>,
    pane_search: Option<CopyModeSearch>,
    search_worker: SearchWorker,
    search_results: Receiver<SearchResults>,
    control_rx: Receiver<Command>,
    slot: Arc<Mutex<ControlSlot>>,
    publisher: Publisher,
    engine_filter: EngineFilter,
    mouse_encoder: mouse::Encoder<'static>,
    mouse_event: mouse::Event<'static>,
    input_bytes: Vec<u8>,
    writer: Box<dyn Write + Send>,
    bound_pasted_images: HashSet<u32>,
    compression: IdleCompression,
    captures: VecDeque<CaptureWork>,
    frozen: bool,
}

impl<'a, 'b> SurfaceActor<'a, 'b> {
    pub(super) fn new(
        control_rx: Receiver<Command>,
        slot: Arc<Mutex<ControlSlot>>,
        publisher: Publisher,
        surface: SurfaceTerminal<'a, 'b>,
        engine_filter: EngineFilter,
        frozen: bool,
    ) -> Result<Self, WorkerError> {
        let SurfaceTerminal {
            terminal,
            geometry,
            frames,
            active_views,
            inactive_views,
            word_separators,
            wrap_search,
            mode_keys_vi,
            reported_color_scheme,
            max_scrollback,
            status,
            pending_commands,
            captures,
            pending_copy_source,
            pane_search,
            search,
        } = surface;
        let (search_worker, search_results) =
            search.unwrap_or_else(|| SearchWorker::spawn(ActorWake::none()));
        let mut actor = Self {
            terminal,
            geometry,
            frames,
            active_views,
            inactive_views,
            word_separators,
            wrap_search,
            mode_keys_vi,
            reported_color_scheme,
            max_scrollback,
            status,
            pending_commands: pending_commands.into(),
            pending_copy_source,
            pane_search,
            search_worker,
            search_results,
            control_rx,
            slot,
            publisher,
            engine_filter,
            mouse_encoder: mouse::Encoder::new()?,
            mouse_event: mouse::Event::new()?,
            input_bytes: Vec::with_capacity(LINK_URI_SCRATCH_BYTES),
            writer: Box::new(std::io::sink()),
            bound_pasted_images: HashSet::new(),
            compression: IdleCompression::default(),
            captures,
            frozen,
        };
        if frozen {
            actor.frames.force_fallback = true;
            publish_views(
                &mut actor.terminal,
                &actor.publisher,
                &mut actor.frames,
                SnapshotChange::Content,
                &mut actor.active_views,
                &actor.word_separators,
                actor.status.clone(),
                false,
            )?;
        }
        Ok(actor)
    }

    pub(super) fn publish_started(&mut self) -> Result<(), WorkerError> {
        self.publisher
            .set_facts(self.engine_filter.facts(&self.terminal)?);
        publish_active_views(
            &mut self.terminal,
            &self.publisher,
            &mut self.frames,
            SnapshotChange::View,
            &mut self.active_views,
            &self.word_separators,
            self.status.clone(),
        )
    }

    pub(super) fn next_deadline(&self) -> Instant {
        let mut due = Instant::now() + IDLE_SLEEP;
        if !self.captures.is_empty() {
            due = Instant::now();
        }
        for deadline in [
            self.frames.synchronized_output_deadline,
            self.frames.settle_due(),
            self.compression.due(),
        ]
        .into_iter()
        .flatten()
        {
            due = due.min(deadline);
        }
        due
    }

    pub(super) fn on_deadline(&mut self) -> Result<(), WorkerError> {
        step_capture_work(&mut self.captures);
        let now = Instant::now();
        if self
            .frames
            .synchronized_output_deadline
            .is_some_and(|due| due <= now)
        {
            publish_active_views(
                &mut self.terminal,
                &self.publisher,
                &mut self.frames,
                SnapshotChange::Content,
                &mut self.active_views,
                &self.word_separators,
                self.status.clone(),
            )?;
        }
        if self.frames.settle_due().is_some_and(|due| due <= now) {
            settle_unwatched(
                &mut self.terminal,
                &self.publisher,
                &mut self.frames,
                &mut self.active_views,
                &self.word_separators,
                self.status.clone(),
            )?;
        }
        self.compression.observe(&self.terminal, now);
        self.compression.run(&mut self.terminal);
        Ok(())
    }

    pub(super) fn try_wake(&mut self) -> Result<Option<Wake>, WorkerError> {
        if let Some(command) = self.pending_commands.pop_front() {
            return Ok(Some(Wake::Command(command)));
        }
        match self.control_rx.try_recv() {
            Ok(command) => return Ok(Some(Wake::Command(command))),
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                return Ok(Some(Wake::CommandsClosed));
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
        }
        match self.search_results.try_recv() {
            Ok(result) => Ok(Some(Wake::Search(result))),
            Err(crossbeam_channel::TryRecvError::Empty) => Ok(None),
            Err(crossbeam_channel::TryRecvError::Disconnected) => Err(WorkerError::Thread(
                "terminal search worker stopped".to_owned(),
            )),
        }
    }

    pub(super) fn wait_for_wake(&mut self) -> Result<Wake, WorkerError> {
        if let Some(wake) = self.try_wake()? {
            return Ok(wake);
        }
        crossbeam_channel::select_biased! {
            recv(self.control_rx) -> command => Ok(command.map_or(Wake::CommandsClosed, Wake::Command)),
            recv(self.search_results) -> result => result.map(Wake::Search).map_err(|_| WorkerError::Thread("terminal search worker stopped".to_owned())),
            default(self.next_deadline().saturating_duration_since(Instant::now())) => Ok(Wake::Deadline),
        }
    }

    pub(super) fn on_wake(&mut self, wake: Wake) -> Result<bool, WorkerError> {
        match wake {
            Wake::Command(message) => {
                for command in take_control_slot(&self.slot, Some(message), true) {
                    if !self.on_command(command)? {
                        return Ok(false);
                    }
                }
            }
            Wake::Search(result) => {
                if apply_search_results(
                    &mut self.terminal,
                    &mut self.active_views,
                    &mut self.inactive_views,
                    &mut self.search_worker,
                    result,
                )? {
                    publish_active_views(
                        &mut self.terminal,
                        &self.publisher,
                        &mut self.frames,
                        SnapshotChange::View,
                        &mut self.active_views,
                        &self.word_separators,
                        self.status.clone(),
                    )?;
                }
            }
            Wake::CommandsClosed => return Ok(false),
            Wake::Deadline => {}
            _ => unreachable!("surface has no PTY"),
        }
        Ok(true)
    }

    fn on_command(&mut self, command: Command) -> Result<bool, WorkerError> {
        match command {
            Command::AttachView(view_id) => {
                if self.frozen {
                    if let Entry::Vacant(entry) = self.active_views.entry(view_id) {
                        let state = self.inactive_views.remove(&view_id).map_or_else(
                            || output_view_state(&mut self.terminal).map(Box::new),
                            Ok,
                        )?;
                        entry.insert(state);
                    }
                } else {
                    activate_view(
                        &mut self.terminal,
                        view_id,
                        &mut self.active_views,
                        &mut self.inactive_views,
                        &self.word_separators,
                    )?;
                }
                if let Some(state) = self.active_views.get_mut(&view_id) {
                    let _ = refresh_view_search(
                        &self.terminal,
                        view_id,
                        state,
                        &mut self.search_worker,
                    )?;
                }
                publish_active_views(
                    &mut self.terminal,
                    &self.publisher,
                    &mut self.frames,
                    SnapshotChange::Content,
                    &mut self.active_views,
                    &self.word_separators,
                    self.status.clone(),
                )?;
            }
            Command::DetachView(view_id) => {
                if self.frozen {
                    if let Some(state) = self.active_views.remove(&view_id) {
                        self.inactive_views.insert(view_id, state);
                    }
                } else {
                    deactivate_view(
                        &mut self.terminal,
                        view_id,
                        &mut self.active_views,
                        &mut self.inactive_views,
                        &self.word_separators,
                    )?;
                }
                self.search_worker.cancel(view_id);
                publish_active_views(
                    &mut self.terminal,
                    &self.publisher,
                    &mut self.frames,
                    SnapshotChange::View,
                    &mut self.active_views,
                    &self.word_separators,
                    self.status.clone(),
                )?;
            }
            Command::ReleaseView(view_id) => {
                self.frames.forget_view(view_id);
                let released = if self.frozen {
                    self.inactive_views.remove(&view_id);
                    self.active_views.remove(&view_id).is_some()
                } else {
                    release_view(
                        &mut self.terminal,
                        view_id,
                        &mut self.active_views,
                        &mut self.inactive_views,
                    )?
                };
                self.search_worker.forget(view_id);
                if released {
                    publish_active_views(
                        &mut self.terminal,
                        &self.publisher,
                        &mut self.frames,
                        SnapshotChange::View,
                        &mut self.active_views,
                        &self.word_separators,
                        self.status.clone(),
                    )?;
                }
            }
            Command::Resize(next) => {
                if next != self.geometry {
                    self.geometry = next;
                    self.terminal.resize(
                        self.geometry.columns.max(1),
                        self.geometry.rows.max(1),
                        self.geometry.cell_width_px,
                        self.geometry.cell_height_px,
                    )?;
                    self.terminal
                        .set_scrollback_max_bytes(Some(scrollback_backstop_bytes(
                            self.max_scrollback.min(MAX_HISTORY_LIMIT),
                            self.geometry.columns.max(1),
                        )))?;
                    if !self.frozen {
                        resize_copy_modes(
                            &mut self.active_views,
                            self.geometry.columns.max(1),
                            self.geometry.rows.max(1),
                            &mut self.search_worker,
                        )?;
                        resize_copy_modes(
                            &mut self.inactive_views,
                            self.geometry.columns.max(1),
                            self.geometry.rows.max(1),
                            &mut self.search_worker,
                        )?;
                    }
                    for view in self.inactive_views.values_mut() {
                        if self.frozen {
                            refresh_output_view(&mut self.terminal, view)?;
                        } else {
                            view.invalidate_layout();
                        }
                    }
                    for view in self.active_views.values_mut() {
                        if self.frozen {
                            refresh_output_view(&mut self.terminal, view)?;
                        } else {
                            view.invalidate_layout();
                            reconcile_view_screen(&mut self.terminal, view, &self.word_separators)?;
                        }
                    }
                    publish_active_views(
                        &mut self.terminal,
                        &self.publisher,
                        &mut self.frames,
                        SnapshotChange::Content,
                        &mut self.active_views,
                        &self.word_separators,
                        self.status.clone(),
                    )?;
                }
            }
            Command::SetWordSeparators(next) => {
                self.word_separators = *next;
            }
            Command::SetWrapSearch(next) => {
                self.wrap_search = next;
            }
            Command::SetAppearance(next) => {
                self.reported_color_scheme
                    .set(ghostty_color_scheme(next.color_scheme));
                apply_terminal_appearance(&mut self.terminal, &next)?;
                self.frames.dictionary.class_hints = ClassHints::new(&next);
                self.frames.reset_render();
                for view in self
                    .active_views
                    .values_mut()
                    .chain(self.inactive_views.values_mut())
                {
                    refresh_frozen_view_appearance(&mut self.terminal, view)?;
                }
                publish_active_views(
                    &mut self.terminal,
                    &self.publisher,
                    &mut self.frames,
                    SnapshotChange::Content,
                    &mut self.active_views,
                    &self.word_separators,
                    self.status.clone(),
                )?;
            }
            Command::ViewAction { view, action } => {
                self.compression.rearm();
                let Some(state) = self.active_views.get_mut(&view) else {
                    return Ok(true);
                };
                let result = normalize_view_action_result(apply_view_action(
                    &mut self.terminal,
                    view,
                    state,
                    action,
                    self.geometry,
                    &mut self.writer,
                    &mut self.mouse_encoder,
                    &mut self.mouse_event,
                    &mut self.input_bytes,
                    &mut self.search_worker,
                    self.wrap_search,
                    self.mode_keys_vi,
                    &self.word_separators,
                    &self.bound_pasted_images,
                    &mut self.pending_copy_source,
                    &mut self.pane_search,
                ))?;
                stamp_copy_mode_marks(&self.terminal, &self.engine_filter, &self.active_views);
                let state = self
                    .active_views
                    .get_mut(&view)
                    .expect("active view was checked above");
                self.publisher
                    .publish_search_string(self.pane_search.as_ref());
                let closed = self.frozen && state.copy_mode.is_none();
                match result {
                    ViewActionResult::Snapshot | ViewActionResult::ContentSnapshot if !closed => {
                        publish_active_views(
                            &mut self.terminal,
                            &self.publisher,
                            &mut self.frames,
                            SnapshotChange::View,
                            &mut self.active_views,
                            &self.word_separators,
                            self.status.clone(),
                        )?;
                    }
                    ViewActionResult::OverlaySnapshot if !closed => {
                        publish_active_views(
                            &mut self.terminal,
                            &self.publisher,
                            &mut self.frames,
                            SnapshotChange::Overlay,
                            &mut self.active_views,
                            &self.word_separators,
                            self.status.clone(),
                        )?;
                    }
                    ViewActionResult::Copy(copy) => self.publisher.copy_ready(view, copy)?,
                    ViewActionResult::OpenUri(uri) => self.publisher.open_uri(view, uri)?,
                    ViewActionResult::None
                    | ViewActionResult::Snapshot
                    | ViewActionResult::OverlaySnapshot
                    | ViewActionResult::ContentSnapshot => {}
                }
                if closed {
                    self.search_worker.forget(view);
                    self.active_views.remove(&view);
                    self.inactive_views.remove(&view);
                    self.publisher.view_closed(view)?;
                    publish_active_views(
                        &mut self.terminal,
                        &self.publisher,
                        &mut self.frames,
                        SnapshotChange::View,
                        &mut self.active_views,
                        &self.word_separators,
                        self.status.clone(),
                    )?;
                }
            }
            Command::Capture(request) => {
                let mut copy_modes = self
                    .active_views
                    .values()
                    .filter_map(|view| view.copy_mode.as_deref());
                let mode = match (copy_modes.next(), copy_modes.next()) {
                    (Some(mode), None) => Some(mode),
                    _ => None,
                };
                if let Some(capture) =
                    CaptureWork::start(&self.terminal, mode, &self.engine_filter, *request)
                {
                    self.captures.push_back(capture);
                }
                self.compression.rearm();
            }
            Command::PointerContext(request) => {
                let PointerContextRequest {
                    view,
                    column,
                    row,
                    reply,
                } = *request;
                let mode = self
                    .active_views
                    .get(&view)
                    .and_then(|view| view.copy_mode.as_deref());
                let _ = reply.send(
                    pointer_context(&self.terminal, mode, column, row, &self.word_separators)
                        .unwrap_or_default(),
                );
            }
            Command::SemanticCapture(request) => {
                let _ = request.reply.send(capture_last_command(&self.terminal));
            }
            Command::History(request) => {
                let HistoryCommand {
                    start,
                    count,
                    reply,
                } = *request;
                let _ = reply.send(capture_history(
                    &self.terminal,
                    start,
                    count,
                    &self.frames.dictionary.class_hints,
                    self.frames.frame_bound(&self.active_views),
                ));
            }
            Command::KittyImage(request) => {
                let image = self
                    .frames
                    .generations
                    .kitty
                    .as_mut()
                    .map_or(Ok(None), |kitty| {
                        kitty.image(&self.terminal, request.image_id)
                    })
                    .unwrap_or_else(|error| {
                        log::warn!("could not export Kitty image {}: {error}", request.image_id);
                        None
                    });
                let _ = request.reply.send(image);
            }
            Command::KittyImageGeneration(request) => {
                let generation = self
                    .frames
                    .generations
                    .kitty
                    .as_ref()
                    .map_or(Ok(None), |_| {
                        KittyGraphicsState::image_generation(&self.terminal, request.image_id)
                    })
                    .unwrap_or_else(|error| {
                        log::warn!(
                            "could not read Kitty image {} generation: {error}",
                            request.image_id
                        );
                        None
                    });
                let _ = request.reply.send(generation);
            }
            Command::SetEngineKnobs(next) => self.mode_keys_vi = next.mode_keys_vi,
            Command::SetPendingCopySource(source) => self.pending_copy_source = source,
            Command::Text { .. }
            | Command::Key { .. }
            | Command::PastePreparedBytes { .. }
            | Command::RawInput(_)
            | Command::SetAllowPassthrough(_)
            | Command::WriteDeadNotice(_)
            | Command::PendingPasteOpened { .. }
            | Command::ResetScreen
            | Command::UnbindPastedImage { .. }
            | Command::Wake => {}
            Command::CaptureCopySource { reply } => {
                let _ = reply.send(
                    capture_copy_source(&mut self.terminal)
                        .inspect(|source| {
                            source.revision.stamp_output_rows(|| {
                                self.engine_filter.output_rows(&self.terminal)
                            });
                        })
                        .map_err(|_| TerminalCaptureError::ActorStopped),
                );
            }
            Command::Output(bytes) => {
                self.publisher.output(&bytes);
                let mut bar = None;
                let mut last_command_status = None;
                if !bytes.is_empty() {
                    self.engine_filter.count_output();
                }
                self.engine_filter.write(
                    &bytes,
                    EngineKnobs::default(),
                    &mut self.terminal,
                    &mut Vec::new(),
                    &mut bar,
                    &mut last_command_status,
                );
                if let Some(bar) = bar {
                    self.publisher.set_progress_bar(bar);
                }
                if let Some(command_status) = last_command_status {
                    self.publisher
                        .set_last_command_status(command_status.code());
                }
                self.publisher
                    .set_facts(self.engine_filter.facts(&self.terminal)?);
                self.publisher.mark_output_activity();
                publish_active_views(
                    &mut self.terminal,
                    &self.publisher,
                    &mut self.frames,
                    SnapshotChange::Content,
                    &mut self.active_views,
                    &self.word_separators,
                    self.status.clone(),
                )?;
            }
            Command::Settle { reply } => {
                let _ = reply.send(());
            }
            Command::Terminate | Command::Shutdown => return Ok(false),
            Command::SetViewStream(view, stream) => {
                if self.frames.set_stream(view, stream) && self.active_views.contains_key(&view) {
                    publish_active_views(
                        &mut self.terminal,
                        &self.publisher,
                        &mut self.frames,
                        SnapshotChange::View,
                        &mut self.active_views,
                        &self.word_separators,
                        self.status.clone(),
                    )?;
                }
            }
            Command::SetViewAreas(areas) => {
                let before = self
                    .frames
                    .frame_extent(&self.terminal, &self.active_views)?;
                self.frames.areas = areas;
                if self
                    .frames
                    .frame_extent(&self.terminal, &self.active_views)?
                    != before
                {
                    publish_active_views(
                        &mut self.terminal,
                        &self.publisher,
                        &mut self.frames,
                        SnapshotChange::View,
                        &mut self.active_views,
                        &self.word_separators,
                        self.status.clone(),
                    )?;
                }
            }
            Command::SetPreviewWatch(watch) => {
                let started = watch && !self.frames.preview;
                self.frames.preview = watch;
                if started {
                    publish_active_views(
                        &mut self.terminal,
                        &self.publisher,
                        &mut self.frames,
                        SnapshotChange::View,
                        &mut self.active_views,
                        &self.word_separators,
                        self.status.clone(),
                    )?;
                } else {
                    self.frames.release_unused(&self.active_views);
                }
            }
            Command::FreshViewport(reply) => {
                self.frames.force_fallback = true;
                publish_views(
                    &mut self.terminal,
                    &self.publisher,
                    &mut self.frames,
                    SnapshotChange::View,
                    &mut self.active_views,
                    &self.word_separators,
                    self.status.clone(),
                    false,
                )?;
                self.frames.force_fallback = false;
                let _ = reply.send(self.publisher.latest_fallback());
            }
        }
        Ok(true)
    }
}
