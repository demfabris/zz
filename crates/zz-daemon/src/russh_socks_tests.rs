use super::*;
use russh::{
    Channel, ChannelId, Disconnect,
    keys::{PrivateKey, PublicKeyOrCertificate, ssh_key::private::Ed25519Keypair},
    server,
};
use tokio::sync::mpsc;

struct TestClient;

impl client::Handler for TestClient {
    type Error = russh::Error;

    async fn check_server_key(&mut self, _: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

#[derive(Default)]
struct Inventory {
    output: Vec<u8>,
    status: u32,
    stall: bool,
    mappings: BTreeMap<u16, u16>,
}

struct TestServer {
    inventory: Arc<parking_lot::Mutex<Inventory>>,
    loopback_port: Option<u16>,
    destinations: mpsc::UnboundedSender<(String, u32)>,
}

impl server::Handler for TestServer {
    type Error = russh::Error;

    async fn auth_none(&mut self, _: &str) -> Result<server::Auth, Self::Error> {
        Ok(server::Auth::Accept)
    }

    async fn channel_open_session(
        &mut self,
        _: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        command: &[u8],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        assert_eq!(command, INVENTORY_COMMAND.as_bytes());
        session.channel_success(channel)?;
        let inventory = self.inventory.lock();
        if !inventory.stall {
            session.data(channel, inventory.output.clone())?;
            session.exit_status_request(channel, inventory.status)?;
            session.eof(channel)?;
            session.close(channel)?;
        }
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<server::Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        self.destinations.send((host.to_owned(), port)).unwrap();
        if host == "denied.example" {
            reply
                .reject(ChannelOpenFailure::AdministrativelyProhibited)
                .await;
            return Ok(());
        }
        let destination = if host == "remote-only.invalid" {
            "127.0.0.1"
        } else {
            host
        };
        let port = if host == "localhost" {
            self.inventory
                .lock()
                .mappings
                .get(&u16::try_from(port).unwrap())
                .copied()
                .or(self.loopback_port)
                .unwrap_or(u16::try_from(port).unwrap())
        } else {
            u16::try_from(port).unwrap()
        };
        let connected = if destination == "localhost" {
            match TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await {
                Ok(remote) => Ok(remote),
                Err(_) => TcpStream::connect((Ipv6Addr::LOCALHOST, port)).await,
            }
        } else {
            TcpStream::connect((destination, port)).await
        };
        match connected {
            Ok(mut remote) => {
                reply.accept().await;
                tokio::spawn(async move {
                    let _ = tokio::io::copy_bidirectional(&mut remote, &mut channel.into_stream())
                        .await;
                });
            }
            Err(_) => reply.reject(ChannelOpenFailure::ConnectFailed).await,
        }
        Ok(())
    }
}

struct Fixture {
    inventory: Arc<parking_lot::Mutex<Inventory>>,
    session: Arc<client::Handle<TestClient>>,
    server: JoinHandle<()>,
    destinations: mpsc::UnboundedReceiver<(String, u32)>,
    port: Arc<AtomicU16>,
    socks: Option<SocksForward>,
}

impl Fixture {
    async fn start() -> Self {
        Self::start_with_loopback(None).await
    }

    async fn start_with_loopback(loopback_port: Option<u16>) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = server::Config {
            keys: vec![PrivateKey::from(Ed25519Keypair::from_seed(&[7; 32]))],
            ..Default::default()
        };
        let (destinations_tx, destinations) = mpsc::unbounded_channel();
        let inventory = Arc::new(parking_lot::Mutex::new(Inventory {
            output: b"lsof\n".to_vec(),
            ..Default::default()
        }));
        let worker_inventory = Arc::clone(&inventory);
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let session = server::run_stream(
                Arc::new(config),
                stream,
                TestServer {
                    inventory: worker_inventory,
                    loopback_port,
                    destinations: destinations_tx,
                },
            )
            .await
            .unwrap();
            let _ = session.await;
        });
        let mut session = client::connect(Arc::new(client::Config::default()), address, TestClient)
            .await
            .unwrap();
        assert!(session.authenticate_none("test").await.unwrap().success());
        let session = Arc::new(session);
        let port = Arc::new(AtomicU16::new(0));
        let socks = SocksForward::start(Arc::clone(&session), Arc::clone(&port))
            .await
            .unwrap();
        Self {
            inventory,
            session,
            server,
            destinations,
            port,
            socks: Some(socks),
        }
    }

    async fn connect(&self) -> TcpStream {
        TcpStream::connect((Ipv4Addr::LOCALHOST, self.port.load(Ordering::Acquire)))
            .await
            .unwrap()
    }

    async fn request(&self, address: &[u8], port: u16) -> (TcpStream, u8) {
        tokio::time::timeout(Duration::from_secs(5), self.request_inner(address, port))
            .await
            .unwrap()
    }

    async fn request_inner(&self, address: &[u8], port: u16) -> (TcpStream, u8) {
        let mut stream = self.connect().await;
        stream.write_all(&[5, 1, 0]).await.unwrap();
        let mut selection = [0; 2];
        stream.read_exact(&mut selection).await.unwrap();
        assert_eq!(selection, [5, 0]);
        let mut request = vec![5, 1, 0];
        request.extend_from_slice(address);
        request.extend_from_slice(&port.to_be_bytes());
        stream.write_all(&request).await.unwrap();
        let mut reply = [0; 10];
        stream.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply[0], 5);
        (stream, reply[1])
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn domain(name: &str) -> Vec<u8> {
    let mut address = vec![3, u8::try_from(name.len()).unwrap()];
    address.extend_from_slice(name.as_bytes());
    address
}

async fn echo_server(address: &str) -> (u16, JoinHandle<()>) {
    let listener = TcpListener::bind((address, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let (mut read, mut write) = stream.split();
        tokio::io::copy(&mut read, &mut write).await.unwrap();
    });
    (port, task)
}

async fn assert_echo(mut stream: TcpStream) {
    let payload = vec![123; 65536];
    stream.write_all(&payload).await.unwrap();
    stream.shutdown().await.unwrap();
    let mut received = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut received))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received, payload);
}

#[tokio::test]
async fn resolves_domains_on_ssh_host_and_preserves_tcp_half_close() {
    let mut fixture = Fixture::start().await;
    let (port, echo) = echo_server("127.0.0.1").await;
    let (stream, status) = fixture.request(&domain("remote-only.invalid"), port).await;
    assert_eq!(status, 0);
    assert_eq!(
        fixture.destinations.recv().await.unwrap(),
        ("remote-only.invalid".to_owned(), u32::from(port))
    );
    assert_echo(stream).await;
    echo.await.unwrap();
}

#[tokio::test]
async fn forwards_remote_loopback_ipv4_and_ipv6() {
    let mut fixture = Fixture::start().await;
    for host in ["127.0.0.1", "::1"] {
        let (port, echo) = echo_server(host).await;
        let address = if host == "::1" {
            let mut address = vec![4];
            address.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
            address
        } else {
            vec![1, 127, 0, 0, 1]
        };
        let (stream, status) = fixture.request(&address, port).await;
        assert_eq!(status, 0);
        assert_eq!(
            fixture.destinations.recv().await.unwrap(),
            (host.to_owned(), u32::from(port))
        );
        assert_echo(stream).await;
        echo.await.unwrap();
    }
}

#[tokio::test]
async fn reports_ssh_denial_and_connection_failure() {
    let fixture = Fixture::start().await;
    let (_, status) = fixture.request(&domain("denied.example"), 80).await;
    assert_eq!(status, 2);
    let unused = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = unused.local_addr().unwrap().port();
    drop(unused);
    let (_, status) = fixture.request(&[1, 127, 0, 0, 1], port).await;
    assert_eq!(status, 5);
}

#[tokio::test]
async fn rejects_authentication_commands_and_malformed_addresses() {
    let fixture = Fixture::start().await;
    let mut stream = fixture.connect().await;
    stream.write_all(&[5, 1, 2]).await.unwrap();
    let mut selection = [0; 2];
    stream.read_exact(&mut selection).await.unwrap();
    assert_eq!(selection, [5, 255]);
    for (request, status) in [
        (&[5, 2, 0, 1][..], 7),
        (&[5, 3, 0, 1][..], 7),
        (&[5, 1, 0, 9][..], 8),
        (&[5, 1, 0, 3, 0][..], 8),
        (&[4, 1, 0, 1][..], 1),
    ] {
        let mut stream = fixture.connect().await;
        stream.write_all(&[5, 1, 0]).await.unwrap();
        stream.read_exact(&mut selection).await.unwrap();
        stream.write_all(request).await.unwrap();
        let mut response = [0; 10];
        stream.read_exact(&mut response).await.unwrap();
        assert_eq!(response[1], status);
    }
}

#[tokio::test]
async fn shutdown_closes_listener_active_connections_and_pending_handshakes() {
    let mut fixture = Fixture::start().await;
    let mut pending = fixture.connect().await;
    pending.write_all(&[5]).await.unwrap();
    let (remote_port, echo) = echo_server("127.0.0.1").await;
    let (mut active, status) = fixture.request(&domain("localhost"), remote_port).await;
    assert_eq!(status, 0);
    let port = fixture.port.load(Ordering::Acquire);
    drop(fixture.socks.take());
    assert_eq!(fixture.port.load(Ordering::Acquire), 0);
    let mut buffer = [0; 1];
    for stream in [&mut active, &mut pending] {
        let result = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buffer))
            .await
            .unwrap();
        assert!(matches!(result, Ok(0) | Err(_)));
    }
    assert!(
        TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .is_err()
    );
    fixture
        .session
        .disconnect(Disconnect::ByApplication, "test complete", "")
        .await
        .unwrap();
    echo.abort();
    let reconnected = Fixture::start().await;
    let (remote_port, echo) = echo_server("127.0.0.1").await;
    let (stream, status) = reconnected.request(&domain("localhost"), remote_port).await;
    assert_eq!(status, 0);
    assert_echo(stream).await;
    echo.await.unwrap();
}

fn unused_loopback_port() -> u16 {
    let ipv4 = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = ipv4.local_addr().unwrap().port();
    let _ipv6 = std::net::TcpListener::bind((Ipv6Addr::LOCALHOST, port)).unwrap();
    port
}

#[tokio::test]
async fn loopback_forwards_http_and_tcp_in_both_families_with_original_port() {
    for local_host in ["127.0.0.1", "::1"] {
        let (remote_port, echo) = echo_server("::1").await;
        let mut fixture = Fixture::start_with_loopback(Some(remote_port)).await;
        let mut forward = LoopbackForward::new(Arc::clone(&fixture.session));
        let port = unused_loopback_port();
        forward = std::thread::spawn(move || {
            forward.ensure(port).unwrap();
            forward.ensure(port).unwrap();
            forward
        })
        .join()
        .unwrap();
        let stream = TcpStream::connect((local_host, port)).await.unwrap();
        assert_echo(stream).await;
        assert_eq!(
            fixture.destinations.recv().await.unwrap(),
            ("localhost".to_owned(), u32::from(port))
        );
        echo.await.unwrap();
        drop(forward);
    }
    let remote = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).await.unwrap();
    let remote_port = remote.local_addr().unwrap().port();
    let http = tokio::spawn(async move {
        for _ in 0..2 {
            let (mut stream, _) = remote.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(stream.read_u8().await.unwrap());
            }
            assert!(request.starts_with(b"GET /original HTTP/1.1\r\nHost: localhost:"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nremote",
                )
                .await
                .unwrap();
        }
    });
    let mut fixture = Fixture::start_with_loopback(Some(remote_port)).await;
    let mut forward = LoopbackForward::new(Arc::clone(&fixture.session));
    let port = unused_loopback_port();
    forward.ensure(port).unwrap();
    for host in ["127.0.0.1", "::1"] {
        let mut stream = TcpStream::connect((host, port)).await.unwrap();
        stream
            .write_all(
                format!("GET /original HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n").as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_string(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.ends_with("remote"));
        assert_eq!(
            fixture.destinations.recv().await.unwrap(),
            ("localhost".to_owned(), u32::from(port))
        );
    }
    http.await.unwrap();
    let task = forward.ports.remove(&port).unwrap();
    task.task.abort();
    let _ = task.task.await;
    drop(forward);
    let mut replacement = LoopbackForward::new(Arc::clone(&fixture.session));
    replacement.ensure(port).unwrap();
}

#[tokio::test]
async fn loopback_rejects_invalid_ports_and_conflicts_without_partial_listeners() {
    let fixture = Fixture::start().await;
    let mut forward = LoopbackForward::new(Arc::clone(&fixture.session));
    assert_eq!(
        forward.ensure(0).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    for host in ["127.0.0.1", "::1"] {
        let occupied = std::net::TcpListener::bind((host, 0)).unwrap();
        let port = occupied.local_addr().unwrap().port();
        assert_eq!(
            forward.ensure(port).unwrap_err().kind(),
            io::ErrorKind::AddrInUse
        );
        assert!(!forward.ports.contains_key(&port));
        let other_host = if host == "::1" { "127.0.0.1" } else { "::1" };
        let other = std::net::TcpListener::bind((other_host, port)).unwrap();
        drop((occupied, other));
        forward.ensure(port).unwrap();
    }
}

#[tokio::test]
async fn loopback_shutdown_closes_connections_and_reconnect_rebinds_same_port() {
    let (remote_port, echo) = echo_server("::1").await;
    let mut fixture = Fixture::start_with_loopback(Some(remote_port)).await;
    let port = unused_loopback_port();
    let mut forward = LoopbackForward::new(Arc::clone(&fixture.session));
    forward.ensure(port).unwrap();
    let mut active = TcpStream::connect((Ipv6Addr::LOCALHOST, port))
        .await
        .unwrap();
    active.write_all(b"ready").await.unwrap();
    let mut echoed = [0; 5];
    active.read_exact(&mut echoed).await.unwrap();
    assert_eq!(&echoed, b"ready");
    fixture.destinations.recv().await.unwrap();
    drop(forward);
    let mut byte = [0; 1];
    let closed = tokio::time::timeout(Duration::from_secs(2), active.read(&mut byte))
        .await
        .unwrap();
    assert!(matches!(closed, Ok(0) | Err(_)));
    for host in ["127.0.0.1", "::1"] {
        assert!(TcpStream::connect((host, port)).await.is_err());
    }
    fixture
        .session
        .disconnect(Disconnect::ByApplication, "test disconnect", "")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !fixture.session.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut disconnected = LoopbackForward::new(Arc::clone(&fixture.session));
    assert_eq!(
        disconnected.ensure(port).unwrap_err().kind(),
        io::ErrorKind::NotConnected
    );
    echo.abort();
    let (remote_port, echo) = echo_server("::1").await;
    let reconnected = Fixture::start_with_loopback(Some(remote_port)).await;
    let mut forward = LoopbackForward::new(Arc::clone(&reconnected.session));
    forward.ensure(port).unwrap();
    assert_echo(
        TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap(),
    )
    .await;
    echo.await.unwrap();
}

#[test]
fn listener_inventory_filters_interfaces_and_parses_both_formats() {
    let lsof = b"lsof\np1\nf4\nn*:3000\nn[::1]:9229\nn127.0.0.1:8080\nn*:8080\nn0.0.0.0:8084\nn[::]:7880\nn192.168.1.4:5000\nn[fe80::1%en0]:5001\n";
    let ss = b"ss\nLISTEN 0 128 0.0.0.0:3000 0.0.0.0:*\nLISTEN 0 128 [::1]:9229 [::]:*\nLISTEN 0 128 127.0.0.1:8080 0.0.0.0:*\nLISTEN 0 128 *:8084 *:*\nLISTEN 0 128 [::]:7880 [::]:*\nLISTEN 0 128 10.0.0.1:9000 0.0.0.0:*\n";
    for input in [lsof.as_slice(), ss.as_slice()] {
        assert_eq!(
            parse_listener_inventory(input).unwrap(),
            BTreeSet::from([3000, 7880, 8080, 8084, 9229])
        );
    }
    for input in [
        b"".as_slice(),
        b"garbage\n",
        b"lsof\nn*:65536\n",
        b"ss\nLISTEN bad\n",
        b"lsof\nn[::1]:bad\n",
    ] {
        assert!(parse_listener_inventory(input).is_err());
    }
    assert!(parse_listener_inventory(b"lsof\n").unwrap().is_empty());
}

async fn http_server(body: &'static str) -> (u16, JoinHandle<()>) {
    let listener = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(stream.read_u8().await.unwrap());
        }
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    });
    (port, task)
}

async fn assert_http(host: &str, port: u16, body: &str) {
    let mut stream = TcpStream::connect((host, port)).await.unwrap();
    stream
        .write_all(format!("GET / HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut response = String::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_string(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.ends_with(body));
}

#[tokio::test]
async fn ssh_inventory_prepares_page_and_api_ports_then_refreshes_without_dropping_streams() {
    let fixture = Fixture::start().await;
    let (page_remote, page_server) = http_server("page").await;
    let (api_remote, api_server) = http_server("api").await;
    let page = unused_loopback_port();
    let api = unused_loopback_port();
    {
        let mut inventory = fixture.inventory.lock();
        inventory.output = format!("lsof\nn[::1]:{page}\nn*:{api}\n").into_bytes();
        inventory.mappings = BTreeMap::from([(page, page_remote), (api, api_remote)]);
    }
    let mut forward = LoopbackForward::new(Arc::clone(&fixture.session));
    forward.reconcile(&discover_loopback_ports(&fixture.session).await.unwrap());
    assert_http("127.0.0.1", page, "page").await;
    assert_http("::1", api, "api").await;
    page_server.await.unwrap();
    api_server.await.unwrap();
    forward.ensure(page).unwrap();
    let (new_remote, echo) = echo_server("::1").await;
    let new_port = unused_loopback_port();
    {
        let mut inventory = fixture.inventory.lock();
        inventory.output = format!("ss\nLISTEN 0 128 [::1]:{new_port} [::]:*\n").into_bytes();
        inventory.mappings.insert(new_port, new_remote);
    }
    forward.reconcile(&discover_loopback_ports(&fixture.session).await.unwrap());
    let mut active = TcpStream::connect((Ipv4Addr::LOCALHOST, new_port))
        .await
        .unwrap();
    active.write_all(b"a").await.unwrap();
    assert_eq!(active.read_u8().await.unwrap(), b'a');
    assert!(forward.ports.contains_key(&page));
    assert!(!forward.ports.contains_key(&api));
    assert!(
        TcpStream::connect((Ipv6Addr::LOCALHOST, api))
            .await
            .is_err()
    );
    fixture.inventory.lock().output = b"lsof\n".to_vec();
    forward.reconcile(&discover_loopback_ports(&fixture.session).await.unwrap());
    active.write_all(b"b").await.unwrap();
    assert_eq!(active.read_u8().await.unwrap(), b'b');
    assert!(
        TcpStream::connect((Ipv4Addr::LOCALHOST, new_port))
            .await
            .is_err()
    );
    assert!(forward.ports.contains_key(&page));
    drop(active);
    echo.await.unwrap();
}

#[tokio::test]
async fn ssh_inventory_bounds_output_and_time_and_recovers_after_failure() {
    let fixture = Fixture::start().await;
    fixture.inventory.lock().output = vec![b'x'; MAX_INVENTORY_BYTES + 1];
    assert_eq!(
        discover_loopback_ports(&fixture.session)
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    fixture.inventory.lock().stall = true;
    assert_eq!(
        discover_loopback_ports(&fixture.session)
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
    {
        let mut inventory = fixture.inventory.lock();
        inventory.stall = false;
        inventory.status = 127;
        inventory.output = b"lsof\n".to_vec();
    }
    assert!(discover_loopback_ports(&fixture.session).await.is_err());
    fixture.inventory.lock().status = 1;
    assert!(
        discover_loopback_ports(&fixture.session)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!fixture.session.is_closed());
}

#[tokio::test]
async fn discovered_port_conflicts_are_isolated_with_room_for_forty_services() {
    let fixture = Fixture::start().await;
    let occupied = std::net::TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).unwrap();
    let conflict = occupied.local_addr().unwrap().port();
    let mut forward = LoopbackForward::new(Arc::clone(&fixture.session));
    let mut ports = BTreeSet::from([conflict]);
    while ports.len() < 41 {
        ports.insert(unused_loopback_port());
    }
    use std::io::Write as _;
    let mut output = b"lsof\n".to_vec();
    for port in &ports {
        writeln!(&mut output, "n*:{port}").unwrap();
    }
    fixture.inventory.lock().output = output;
    forward.reconcile(&discover_loopback_ports(&fixture.session).await.unwrap());
    assert_eq!(forward.ports.len(), 40);
    assert_eq!(
        forward.ensure(conflict).unwrap_err().kind(),
        io::ErrorKind::AddrInUse
    );
    assert!(std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, conflict)).is_ok());
    assert!(!fixture.session.is_closed());
}
