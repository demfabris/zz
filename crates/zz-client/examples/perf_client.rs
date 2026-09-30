#[cfg(unix)]
fn main() {
    use std::{
        env,
        io::Read,
        path::PathBuf,
        process,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        thread,
    };

    use zz_client::{ClientCore, Outbound};
    use zz_daemon::InteractiveClient;
    use zz_protocol::{ClientHello, Event, EventPayload, ProtocolMessage};
    use zz_terminal::TerminalColorScheme;

    let mut args = env::args().skip(1);
    let (Some(socket), Some(session)) = (args.next(), args.next()) else {
        eprintln!("usage: perf_client SOCKET SESSION [COLUMNS ROWS]");
        process::exit(2);
    };
    let columns = args
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(180);
    let rows = args
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(51);
    let size = format!(
        "{}{columns}x{rows}",
        ClientHello::CLIENT_SIZE_CAPABILITY_PREFIX
    );
    let cell = format!("{}8x16", ClientHello::CLIENT_CELL_CAPABILITY_PREFIX);
    let client = match InteractiveClient::connect_with_capabilities(
        &PathBuf::from(socket),
        TerminalColorScheme::Dark,
        true,
        &[size.as_str(), cell.as_str()],
    ) {
        Ok(client) => Arc::new(client),
        Err(error) => {
            eprintln!("perf_client: connect failed: {error}");
            process::exit(1);
        }
    };
    let counters = Arc::new([
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
    ]);
    {
        let counters = Arc::clone(&counters);
        thread::spawn(move || {
            let _ = std::io::stdin().read_to_end(&mut Vec::new());
            let [fulls, patches, requests, messages] = counters
                .each_ref()
                .map(|counter| counter.load(Ordering::Relaxed));
            println!(
                "{{\"fulls\": {fulls}, \"patches\": {patches}, \"full_requests\": {requests}, \"messages\": {messages}}}"
            );
            process::exit(0);
        });
    }
    let mut core = ClientCore::new();
    core.handle_message(ProtocolMessage::ServerHello(Box::new(
        client.server_hello().clone(),
    )));
    if let Err(error) = client.attach(session) {
        eprintln!("perf_client: attach failed: {error}");
        process::exit(1);
    }
    while let Ok(message) = client.recv() {
        let slot = match &message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::TerminalViewport { .. },
                ..
            }) => 0,
            ProtocolMessage::Event(Event {
                payload: EventPayload::TerminalPatch { .. },
                ..
            }) => 1,
            _ => 3,
        };
        counters[slot].fetch_add(1, Ordering::Relaxed);
        if slot != 3 {
            counters[3].fetch_add(1, Ordering::Relaxed);
        }
        core.handle_message(message);
        while let Some(Outbound::RequestFull(pane)) = core.poll_outbound() {
            counters[2].fetch_add(1, Ordering::Relaxed);
            let _ = client.request_full(pane);
        }
        while core.poll_event().is_some() {}
    }
}

#[cfg(not(unix))]
fn main() {}
