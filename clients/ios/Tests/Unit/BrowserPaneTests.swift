import CryptoKit
import Network
import UIKit
import WebKit
import XCTest
@testable import ZZ

@MainActor
final class BrowserPaneTests: XCTestCase {
    func testRemoteDNSPagesAndWebSocketsUseSOCKS() async throws {
        let proxy = try BrowserSOCKSFixture()
        defer { proxy.stop() }
        try await eventually { proxy.port != nil }
        let port = try XCTUnwrap(proxy.port)
        let host = "remote-only.invalid"
        let runtime = makeRuntime("http://\(host):38761/page", route: .ssh(port))
        let mount = BrowserTestMount(runtime.selected.webView)
        defer { mount.close(); runtime.suspend() }
        runtime.apply(runtime.descriptor, route: .ssh(port))
        do {
            try await loaded(runtime.selected, marker: proxy.marker)
        } catch {
            XCTFail("HTTP \(host) failed: \(runtime.selected.error ?? "no WebKit error"); url=\(runtime.selected.webView.url?.absoluteString ?? "nil"); \(proxy.diagnostics)")
            throw error
        }
        XCTAssertTrue(proxy.destinations.contains { $0.host == host && $0.port == 38761 })
        XCTAssertEqual(runtime.selected.webView.url?.host, host)
        XCTAssertEqual(runtime.descriptor.tabs, ["http://\(host):38761/page"])
        _ = try await javaScript("""
            window.socketResult = 'waiting';
            window.socket = new WebSocket('ws://' + location.host + '/socket');
            socket.onmessage = event => { window.socketResult = event.data; };
            socket.onerror = () => { window.socketResult = 'failed'; };
            'started';
            """, in: runtime.selected.webView)
        do {
            try await eventually {
                try await self.javaScript("window.socketResult", in: runtime.selected.webView) == proxy.marker
            }
        } catch {
            let result = try? await javaScript("window.socketResult", in: runtime.selected.webView)
            XCTFail("WebSocket \(host) failed: result=\(result ?? "nil"); \(proxy.diagnostics)")
            throw error
        }
        XCTAssertTrue(proxy.requests.contains { $0.contains("GET /socket ") })
        XCTAssertGreaterThanOrEqual(proxy.destinations.count, 2)
    }

    func testLocalhostKeepsOriginForPagesFetchAndWebSockets() async throws {
        let proxy = try BrowserSOCKSFixture()
        let server = try BrowserSOCKSFixture(socks: false)
        defer { proxy.stop(); server.stop() }
        try await eventually { proxy.port != nil && server.port != nil }
        let port = try XCTUnwrap(server.port)
        let origin = "http://localhost:\(port)"
        var forwards: [UInt16] = []
        let runtime = makeRuntime(origin + "/page", forwardLoopback: {
            forwards.append($0)
            return $0 == port ? nil : "Unexpected port"
        })
        let mount = BrowserTestMount(runtime.selected.webView)
        defer { mount.close(); runtime.suspend() }
        runtime.apply(runtime.descriptor, route: .ssh(try XCTUnwrap(proxy.port)))
        try await loaded(runtime.selected, marker: server.marker)
        XCTAssertEqual(runtime.selected.webView.url?.absoluteString, origin + "/page")
        XCTAssertEqual(runtime.descriptor.tabs, [origin + "/page"])
        let actualOrigin = try await javaScript("location.origin", in: runtime.selected.webView)
        XCTAssertEqual(actualOrigin, origin)
        XCTAssertEqual(Set(forwards), [port])
        _ = try await javaScript("""
            window.fetchResult = 'waiting';
            fetch('\(origin)/api').then(r => r.text()).then(text => { window.fetchResult = text; });
            window.socketResult = 'waiting';
            window.socket = new WebSocket('ws://localhost:\(port)/socket');
            socket.onmessage = event => { window.socketResult = event.data; };
            'started';
            """, in: runtime.selected.webView)
        try await eventually {
            try await self.javaScript("window.fetchResult.includes('\(server.marker)') && window.socketResult === '\(server.marker)' ? 'ready' : 'waiting'", in: runtime.selected.webView) == "ready"
        }
        _ = try await javaScript("location.href = '\(origin)/next'; 'started';", in: runtime.selected.webView)
        try await eventually { runtime.selected.webView.url?.path == "/next" }
        try await loaded(runtime.selected, marker: server.marker)
        XCTAssertEqual(runtime.descriptor.tabs, [origin + "/next"])
        XCTAssertTrue(proxy.destinations.isEmpty)
        XCTAssertTrue(server.requests.contains { $0.contains("Host: localhost:\(port)\r\n") })
        let beforeReconnect = forwards.count
        runtime.suspend()
        runtime.apply(runtime.descriptor, route: .ssh(try XCTUnwrap(proxy.port)))
        XCTAssertGreaterThan(forwards.count, beforeReconnect)
    }

    func testLoopbackForwardFailureBlocksNavigationAndCanRetry() async throws {
        let proxy = try BrowserSOCKSFixture()
        let server = try BrowserSOCKSFixture(socks: false)
        defer { proxy.stop(); server.stop() }
        try await eventually { proxy.port != nil && server.port != nil }
        let url = "http://127.0.0.1:\(try XCTUnwrap(server.port))/page"
        var occupied = true
        let runtime = makeRuntime(url, forwardLoopback: { _ in occupied ? "Address already in use" : nil })
        let mount = BrowserTestMount(runtime.selected.webView)
        defer { mount.close(); runtime.suspend() }
        runtime.apply(runtime.descriptor, route: .ssh(try XCTUnwrap(proxy.port)))
        XCTAssertEqual(runtime.selected.error, "Address already in use")
        XCTAssertNil(runtime.selected.webView.url)
        XCTAssertTrue(server.requests.isEmpty)
        occupied = false
        runtime.navigate(url)
        try await loaded(runtime.selected, marker: server.marker)
        XCTAssertEqual(runtime.selected.webView.url?.absoluteString, url)
        XCTAssertTrue(proxy.destinations.isEmpty)
    }

    func testLocalhostAuthenticationAPIAndWebSocketUseSeparatePorts() async throws {
        let proxy = try BrowserSOCKSFixture()
        let page = try BrowserSOCKSFixture(socks: false)
        let auth = try BrowserSOCKSFixture(socks: false)
        let api = try BrowserSOCKSFixture(socks: false)
        defer { proxy.stop(); page.stop(); auth.stop(); api.stop() }
        try await eventually { proxy.port != nil && page.port != nil && auth.port != nil && api.port != nil }
        let pagePort = try XCTUnwrap(page.port)
        let authPort = try XCTUnwrap(auth.port)
        let apiPort = try XCTUnwrap(api.port)
        let origin = "http://localhost:\(pagePort)"
        var navigationPorts: [UInt16] = []
        let runtime = makeRuntime(origin + "/page", forwardLoopback: {
            navigationPorts.append($0)
            return nil
        })
        let mount = BrowserTestMount(runtime.selected.webView)
        defer { mount.close(); runtime.suspend() }
        runtime.apply(runtime.descriptor, route: .ssh(try XCTUnwrap(proxy.port)))
        try await loaded(runtime.selected, marker: page.marker)
        _ = try await javaScript("""
            window.authResult = 'waiting';
            window.apiResult = 'waiting';
            window.socketResult = 'waiting';
            fetch('http://localhost:\(authPort)/login', {
                method: 'POST',
                headers: {'Content-Type': 'application/json', 'X-Amz-Target': 'Fixture.InitiateAuth'},
                body: JSON.stringify({username: 'fixture-user'})
            }).then(r => r.text()).then(text => {
                window.authResult = text;
                return fetch('http://localhost:\(apiPort)/account', {
                    headers: {'Authorization': 'Bearer fixture-token'}
                });
            }).then(r => r.text()).then(text => { window.apiResult = text; });
            window.socket = new WebSocket('ws://localhost:\(apiPort)/socket');
            socket.onmessage = event => { window.socketResult = event.data; };
            'started';
            """, in: runtime.selected.webView)
        try await eventually {
            try await self.javaScript("window.authResult.includes('\(auth.marker)') && window.apiResult.includes('\(api.marker)') && window.socketResult === '\(api.marker)' ? 'ready' : 'waiting'", in: runtime.selected.webView) == "ready"
        }
        XCTAssertEqual(runtime.selected.webView.url?.absoluteString, origin + "/page")
        XCTAssertEqual(Set(navigationPorts), [pagePort])
        XCTAssertTrue(proxy.destinations.isEmpty)
        XCTAssertTrue(auth.requests.contains { $0.hasPrefix("OPTIONS /login ") })
        XCTAssertTrue(auth.requests.contains { $0.hasPrefix("POST /login ") && $0.contains("{\"username\":\"fixture-user\"}") })
        XCTAssertTrue(api.requests.contains { $0.hasPrefix("GET /account ") && $0.contains("Bearer fixture-token") })
    }

    func testSnapshotUpdatesTabSwitchesAndRemountsPreserveLiveFormState() async throws {
        let proxy = try BrowserSOCKSFixture()
        defer { proxy.stop() }
        try await eventually { proxy.port != nil }
        let runtime = makeRuntime("http://remote-only.invalid:38761/first")
        let mount = BrowserTestMount(runtime.selected.webView)
        defer { mount.close(); runtime.suspend() }
        runtime.apply(runtime.descriptor, route: .ssh(try XCTUnwrap(proxy.port)))
        let original = runtime.selected
        try await loaded(original, marker: proxy.marker)
        _ = try await javaScript("document.querySelector('input').value = 'unfinished work'; window.retained = 'alive';", in: original.webView)
        let requestsBefore = proxy.requests.count
        runtime.apply(runtime.descriptor, route: runtime.route)
        runtime.add("http://remote-only.invalid:38761/second")
        mount.show(runtime.selected.webView)
        try await loaded(runtime.selected, marker: proxy.marker)
        runtime.select(original.id)
        mount.show(original.webView)
        XCTAssertTrue(runtime.selected === original)
        let value = try await javaScript("document.querySelector('input').value + ':' + window.retained", in: original.webView)
        XCTAssertEqual(value, "unfinished work:alive")
        XCTAssertEqual(proxy.requests.count, requestsBefore + 1)
    }

    func testSSHRejectsLoopbackAddressesWithoutListeners() async throws {
        let proxy = try BrowserSOCKSFixture()
        defer { proxy.stop() }
        try await eventually { proxy.port != nil }
        let runtime = makeRuntime("http://127.0.0.2:38761/initial")
        defer { runtime.suspend() }
        runtime.apply(runtime.descriptor, route: .ssh(try XCTUnwrap(proxy.port)))
        XCTAssertTrue(runtime.route.available)
        XCTAssertTrue(runtime.selected.error?.contains("localhost") == true)
        XCTAssertNil(runtime.selected.webView.url)
        XCTAssertFalse(runtime.selected.webView.isLoading)

        for host in ["127.0.0.2", "0.0.0.0"] {
            runtime.selected.error = nil
            runtime.navigate("http://\(host):38761/navigation")
            XCTAssertTrue(runtime.selected.error?.contains("localhost") == true, host)
            XCTAssertNil(runtime.selected.webView.url, host)
            XCTAssertFalse(runtime.selected.webView.isLoading, host)
        }
        XCTAssertTrue(proxy.destinations.isEmpty)
        XCTAssertTrue(proxy.requests.isEmpty)
    }

    func testUnavailableRouteBlocksLoadedPageFetchAndNewNavigation() async throws {
        let proxy = try BrowserSOCKSFixture()
        defer { proxy.stop() }
        try await eventually { proxy.port != nil }
        let runtime = makeRuntime("http://remote-only.invalid:38761/before")
        let mount = BrowserTestMount(runtime.selected.webView)
        defer { mount.close(); runtime.suspend() }
        runtime.apply(runtime.descriptor, route: .ssh(try XCTUnwrap(proxy.port)))
        try await loaded(runtime.selected, marker: proxy.marker)
        runtime.suspend()
        let requestsBefore = proxy.requests.count
        _ = try await javaScript("""
            window.fetchResult = 'waiting';
            fetch('/must-not-leave-device', {cache: 'no-store', signal: AbortSignal.timeout(1500)})
                .then(() => { window.fetchResult = 'leaked'; })
                .catch(() => { window.fetchResult = 'blocked'; });
            'started';
            """, in: runtime.selected.webView)
        try await eventually {
            try await self.javaScript("window.fetchResult", in: runtime.selected.webView) != "waiting"
        }
        let result = try await javaScript("window.fetchResult", in: runtime.selected.webView)
        XCTAssertEqual(result, "blocked")
        runtime.navigate("http://remote-only.invalid:38761/also-blocked")
        XCTAssertEqual(proxy.requests.count, requestsBefore)
        XCTAssertFalse(runtime.selected.available)
    }

    func testReconnectUsesReplacementProxyAndLoadsChangedDescriptor() async throws {
        let first = try BrowserSOCKSFixture()
        let second = try BrowserSOCKSFixture()
        defer { first.stop(); second.stop() }
        try await eventually { first.port != nil && second.port != nil }
        let runtime = makeRuntime("http://remote-only.invalid:38761/old")
        let mount = BrowserTestMount(runtime.selected.webView)
        defer { mount.close(); runtime.suspend() }
        runtime.apply(runtime.descriptor, route: .ssh(try XCTUnwrap(first.port)))
        try await loaded(runtime.selected, marker: first.marker)
        runtime.suspend()
        let next = ZZBrowserDescriptor(tabs: ["http://remote-only.invalid:38761/new"], activeTab: 0, profile: "default")
        runtime.apply(next, route: .unavailable)
        XCTAssertFalse(first.requests.contains { $0.contains("GET /new ") })
        XCTAssertTrue(second.requests.isEmpty)
        runtime.apply(next, route: .ssh(try XCTUnwrap(second.port)))
        try await loaded(runtime.selected, marker: second.marker)
        XCTAssertTrue(second.requests.contains { $0.contains("GET /new ") })
        XCTAssertFalse(first.requests.contains { $0.contains("GET /new ") })
    }

    func testCookiesPersistForSameHostProfileAndStaySeparateFromOthers() async throws {
        let identity = UUID().uuidString
        let original = ZZBrowserProfile(host: identity, profile: "work")
        let cookie = try XCTUnwrap(HTTPCookie(properties: [
            .domain: "remote-only.invalid", .path: "/", .name: "zz-browser-test",
            .value: identity, .expires: Date().addingTimeInterval(3600),
        ]))
        await original.dataStore.httpCookieStore.setCookie(cookie)
        let reopened = ZZBrowserProfile(host: identity, profile: "work")
        let otherProfile = ZZBrowserProfile(host: identity, profile: "personal")
        let otherHost = ZZBrowserProfile(host: identity + "-other", profile: "work")
        let sameCookies = await reopened.dataStore.httpCookieStore.allCookies()
        let differentProfileCookies = await otherProfile.dataStore.httpCookieStore.allCookies()
        let differentHostCookies = await otherHost.dataStore.httpCookieStore.allCookies()
        XCTAssertTrue(original.dataStore.isPersistent)
        XCTAssertEqual(reopened.dataStore.identifier, original.dataStore.identifier)
        XCTAssertEqual(sameCookies.first { $0.name == cookie.name }?.value, identity)
        XCTAssertFalse(differentProfileCookies.contains { $0.name == cookie.name })
        XCTAssertFalse(differentHostCookies.contains { $0.name == cookie.name })
        await original.dataStore.httpCookieStore.deleteCookie(cookie)
    }

    private func makeRuntime(_ url: String, route: ZZBrowserRoute = .unavailable,
                             forwardLoopback: ((UInt16) -> String?)? = nil) -> ZZBrowserPaneRuntime {
        let profile = ZZBrowserProfile(host: "BrowserTests-\(UUID().uuidString)", profile: "default")
        profile.forwardLoopback = forwardLoopback
        profile.apply(route)
        return ZZBrowserPaneRuntime(
            descriptor: ZZBrowserDescriptor(tabs: [url], activeTab: 0, profile: "default"),
            profile: profile
        )
    }

    private func loaded(_ tab: ZZBrowserTab, marker: String,
                        file: StaticString = #filePath, line: UInt = #line) async throws {
        try await eventually(file: file, line: line) {
            if let error = tab.error {
                XCTFail("Browser tab failed: \(error)", file: file, line: line)
                throw BrowserFixtureError.closed
            }
            return (try? await self.javaScript("document.body.dataset.proxy || ''", in: tab.webView)) == marker
        }
        XCTAssertNil(tab.error)
    }

    private func javaScript(_ script: String, in webView: WKWebView) async throws -> String {
        try await withCheckedThrowingContinuation { continuation in
            webView.evaluateJavaScript(script) { value, error in
                if let error { continuation.resume(throwing: error) }
                else { continuation.resume(returning: value as? String ?? "") }
            }
        }
    }

    private func eventually(file: StaticString = #filePath, line: UInt = #line,
                            _ condition: () async throws -> Bool) async throws {
        let timeout = Duration.seconds(60)
        let deadline = ContinuousClock.now + timeout
        while ContinuousClock.now < deadline {
            if try await condition() { return }
            try await Task.sleep(for: .milliseconds(50))
        }
        XCTFail("Condition not met within \(timeout)", file: file, line: line)
        throw BrowserFixtureError.timedOut
    }
}

private enum BrowserFixtureError: Error { case closed, malformed, timedOut }

@MainActor
private final class BrowserTestMount {
    private let window: UIWindow

    init(_ webView: WKWebView) {
        let scene = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first!
        window = UIWindow(windowScene: scene)
        window.rootViewController = UIViewController()
        window.isHidden = false
        show(webView)
    }

    func show(_ webView: WKWebView) {
        let container = window.rootViewController!.view!
        container.subviews.forEach { $0.removeFromSuperview() }
        webView.frame = container.bounds
        webView.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        container.addSubview(webView)
    }

    func close() { window.isHidden = true }
}

@MainActor
private final class BrowserSOCKSFixture {
    let marker = UUID().uuidString
    private let listener: NWListener
    private var connections: [NWConnection] = []
    private(set) var destinations: [(host: String, port: UInt16)] = []
    private(set) var requests: [String] = []
    private var failures: [String] = []
    var port: UInt16? { listener.port.flatMap { $0.rawValue == 0 ? nil : $0.rawValue } }
    var diagnostics: String { "destinations=\(destinations); requests=\(requests); failures=\(failures)" }

    init(socks: Bool = true) throws {
        let parameters = NWParameters.tcp
        parameters.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: .any)
        listener = try NWListener(using: parameters)
        listener.newConnectionHandler = { [weak self] connection in
            Task { @MainActor [weak self] in
                guard let self else { connection.cancel(); return }
                self.connections.append(connection)
                connection.start(queue: .main)
                do {
                    let stream = BrowserFixtureConnection(connection)
                    if socks { try await self.negotiate(stream) }
                    try await self.serve(stream)
                }
                catch { self.failures.append(error.localizedDescription); connection.cancel() }
            }
        }
        listener.start(queue: .main)
    }

    func stop() {
        listener.cancel()
        connections.forEach { $0.cancel() }
        connections.removeAll()
    }

    private func negotiate(_ connection: BrowserFixtureConnection) async throws {
        let greeting = try await connection.read(2)
        guard greeting[0] == 5 else { throw BrowserFixtureError.malformed }
        let methods = try await connection.read(Int(greeting[1]))
        guard methods.contains(0) else { throw BrowserFixtureError.malformed }
        try await connection.send(Data([5, 0]))
        let header = try await connection.read(4)
        guard header[0] == 5, header[1] == 1 else { throw BrowserFixtureError.malformed }
        let host: String
        switch header[3] {
        case 1:
            host = try await connection.read(4).map(String.init).joined(separator: ".")
        case 3:
            let count = try await connection.read(1)[0]
            host = String(decoding: try await connection.read(Int(count)), as: UTF8.self)
        case 4:
            let bytes = try await connection.read(16)
            host = IPv6Address(Data(bytes))?.debugDescription ?? "invalid-ipv6"
        default: throw BrowserFixtureError.malformed
        }
        let port = try await connection.read(2)
        destinations.append((host, UInt16(port[0]) << 8 | UInt16(port[1])))
        try await connection.send(Data([5, 0, 0, 1, 127, 0, 0, 1, 0, 0]))
    }

    private func serve(_ connection: BrowserFixtureConnection) async throws {
        var request = try await connection.headers()
        let lines = request.components(separatedBy: "\r\n")
        if let length = lines.first(where: { $0.lowercased().hasPrefix("content-length:") }),
           let count = Int(length.dropFirst("content-length:".count).trimmingCharacters(in: .whitespaces)),
           count > 0 {
            guard count <= 16384 else { throw BrowserFixtureError.malformed }
            request += String(decoding: try await connection.read(count), as: UTF8.self)
        }
        requests.append(request)
        if let keyLine = request.components(separatedBy: "\r\n").first(where: {
            $0.lowercased().hasPrefix("sec-websocket-key:")
        }) {
            let key = keyLine.dropFirst("sec-websocket-key:".count).trimmingCharacters(in: .whitespaces)
            let digest = Insecure.SHA1.hash(data: Data((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").utf8))
            try await connection.send(Data("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: \(Data(digest).base64EncodedString())\r\n\r\n".utf8))
            try await connection.send(Data([0x81, UInt8(marker.utf8.count)]) + Data(marker.utf8))
        } else {
            let preflight = request.hasPrefix("OPTIONS ")
            let body = preflight ? Data() : Data("<!doctype html><html><head><title>Proxy fixture</title><link rel='icon' href='data:,'></head><body data-proxy='\(marker)'><input value='original'></body></html>".utf8)
            let origin = lines.first(where: { $0.lowercased().hasPrefix("origin:") })?
                .dropFirst("origin:".count).trimmingCharacters(in: .whitespaces)
            let cors = origin.map { "Access-Control-Allow-Origin: \($0)\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization, X-Amz-Target\r\n" } ?? ""
            let response = Data("HTTP/1.1 \(preflight ? "204 No Content" : "200 OK")\r\n\(cors)Content-Type: text/html\r\nCache-Control: no-store\r\nContent-Length: \(body.count)\r\nConnection: close\r\n\r\n".utf8) + body
            try await connection.send(response, final: true)
        }
    }
}

@MainActor
private final class BrowserFixtureConnection {
    private let connection: NWConnection
    private var buffered = Data()

    init(_ connection: NWConnection) { self.connection = connection }

    func read(_ count: Int) async throws -> [UInt8] {
        while buffered.count < count {
            let chunk: Data = try await withCheckedThrowingContinuation { continuation in
                connection.receive(minimumIncompleteLength: 1, maximumLength: 8192) { content, _, _, error in
                    if let error { continuation.resume(throwing: error) }
                    else if let content, !content.isEmpty { continuation.resume(returning: content) }
                    else { continuation.resume(throwing: BrowserFixtureError.closed) }
                }
            }
            buffered.append(chunk)
        }
        let bytes = Array(buffered.prefix(count))
        buffered.removeFirst(count)
        return bytes
    }

    func headers() async throws -> String {
        var bytes: [UInt8] = []
        while bytes.count < 16384 {
            bytes.append(contentsOf: try await read(1))
            if bytes.suffix(4) == [13, 10, 13, 10] { return String(decoding: bytes, as: UTF8.self) }
        }
        throw BrowserFixtureError.malformed
    }

    func send(_ data: Data, final: Bool = false) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            connection.send(content: data, contentContext: final ? .finalMessage : .defaultMessage,
                            isComplete: true, completion: .contentProcessed { error in
                if let error { continuation.resume(throwing: error) }
                else { continuation.resume() }
            })
        }
    }
}
