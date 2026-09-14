import CryptoKit
import Network
import Observation
import SwiftUI
import WebKit

struct ZZBrowserDescriptor: Codable, Equatable, Sendable {
    var tabs: [String]
    var activeTab: Int
    var profile: String

    enum CodingKeys: String, CodingKey {
        case tabs, profile
        case activeTab = "active_tab"
    }

    static let blank = Self(tabs: ["about:blank"], activeTab: 0, profile: "default")

    var normalized: Self {
        let urls = tabs.isEmpty ? ["about:blank"] : tabs
        return Self(tabs: urls, activeTab: min(max(activeTab, 0), urls.count - 1), profile: profile)
    }

    var url: String { normalized.tabs[normalized.activeTab] }
}

enum ZZBrowserRoute: Equatable {
    case local
    case ssh(UInt16)
    case unavailable

    var available: Bool {
        switch self {
        case .local: true
        case let .ssh(port): port != 0
        case .unavailable: false
        }
    }
}

enum ZZBrowserAddress {
    static func loopbackPort(_ url: URL) -> UInt16? {
        guard ["localhost", "localhost.", "127.0.0.1", "::1", "[::1]"].contains(url.host?.lowercased() ?? ""),
              ["http", "https"].contains(url.scheme?.lowercased() ?? "") else { return nil }
        return UInt16(exactly: url.port ?? (url.scheme?.lowercased() == "https" ? 443 : 80))
    }

    static func isNumericLoopback(_ url: URL) -> Bool {
        guard let host = url.host?.lowercased() else { return false }
        return host.hasPrefix("127.") || host == "[::1]" || host == "::1" || host == "0.0.0.0"
    }

    static func url(_ input: String) -> URL? {
        let text = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return nil }
        if text == "about:blank" { return URL(string: text) }
        let value: String
        if text.contains("://") {
            value = text
        } else {
            let host = text.split(separator: "/", maxSplits: 1).first.map(String.init) ?? text
            let local = host == "localhost" || host.hasPrefix("localhost:") ||
                host == "localhost." || host.hasPrefix("localhost.:") ||
                host.hasPrefix("127.") || host.hasPrefix("[::1]")
            value = "\(local ? "http" : "https")://\(text)"
        }
        guard let url = URL(string: value), let host = url.host, !host.isEmpty,
              ["http", "https"].contains(url.scheme?.lowercased() ?? ""),
              !host.contains(where: \.isWhitespace),
              url.port.map({ (1...65535).contains($0) }) ?? true else { return nil }
        return url
    }

    static func profileID(host: String, profile: String) -> UUID {
        let data = Data("\(host.utf8.count):\(host)\(profile)".utf8)
        let bytes = Array(SHA256.hash(data: data).prefix(16))
        return UUID(uuid: (bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5],
                           bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11],
                           bytes[12], bytes[13], bytes[14], bytes[15]))
    }
}

@MainActor
final class ZZBrowserProfile {
    let dataStore: WKWebsiteDataStore
    var forwardLoopback: ((UInt16) -> String?)?
    private var route: ZZBrowserRoute?
    private var forwardedPorts: Set<UInt16> = []

    init(host: String, profile: String) {
        dataStore = WKWebsiteDataStore(forIdentifier: ZZBrowserAddress.profileID(host: host, profile: profile))
        apply(.unavailable)
    }

    func apply(_ next: ZZBrowserRoute) {
        guard next != route else { return }
        route = next
        switch next {
        case .local:
            dataStore.proxyConfigurations = []
        case .ssh, .unavailable:
            let port: UInt16
            if case let .ssh(value) = next { port = value } else { port = 0 }
            var proxy = ProxyConfiguration(socksv5Proxy: .hostPort(
                host: "127.0.0.1", port: NWEndpoint.Port(rawValue: port)!
            ))
            proxy.allowFailover = false
            proxy.excludedDomains = ["localhost", "localhost.", "127.0.0.1", "::1"]
            dataStore.proxyConfigurations = [proxy]
            if case .ssh = next {
                for port in forwardedPorts { _ = forwardLoopback?(port) }
            }
        }
    }

    func prepare(_ url: URL) -> String? {
        guard let route, case .ssh = route else { return nil }
        guard let port = ZZBrowserAddress.loopbackPort(url) else {
            return ZZBrowserAddress.isNumericLoopback(url) ? "Use localhost to reach this host’s loopback server." : nil
        }
        guard let forwardLoopback else { return "The host connection cannot forward localhost ports." }
        if let error = forwardLoopback(port) { return error }
        forwardedPorts.insert(port)
        return nil
    }
}

@MainActor
@Observable
final class ZZBrowserTab: NSObject, Identifiable, WKNavigationDelegate, WKUIDelegate {
    let id = UUID()
    let webView: WKWebView
    private let profile: ZZBrowserProfile
    var url: String
    var title = "New Tab"
    var canGoBack = false
    var canGoForward = false
    var isLoading = false
    var error: String?
    var available = false
    var onChange: (() -> Void)?
    var onOpen: ((URL) -> Void)?
    @ObservationIgnored private var observations: [NSKeyValueObservation] = []
    @ObservationIgnored private var needsLoad = true
    @ObservationIgnored private var navigating = false

    init(url: String, profile: ZZBrowserProfile) {
        self.url = url
        self.profile = profile
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = profile.dataStore
        webView = WKWebView(frame: .zero, configuration: configuration)
        super.init()
        webView.navigationDelegate = self
        webView.uiDelegate = self
        webView.allowsBackForwardNavigationGestures = true
        webView.isInspectable = true
        observations = [
            webView.observe(\.url, options: [.new]) { [weak self] _, _ in
                Task { @MainActor [weak self] in self?.refresh() }
            },
            webView.observe(\.title, options: [.new]) { [weak self] _, _ in
                Task { @MainActor [weak self] in self?.refresh() }
            },
            webView.observe(\.isLoading, options: [.new]) { [weak self] _, _ in
                Task { @MainActor [weak self] in self?.refresh() }
            }
        ]
    }

    func load(_ value: String) {
        guard let destination = ZZBrowserAddress.url(value) else {
            error = "Enter an HTTP or HTTPS address."
            return
        }
        url = destination.absoluteString
        error = nil
        needsLoad = true
        resume()
    }

    func resume() {
        guard available, needsLoad, let destination = ZZBrowserAddress.url(url) else { return }
        if let failure = profile.prepare(destination) {
            error = failure
            return
        }
        error = nil
        needsLoad = false
        navigating = true
        webView.load(URLRequest(url: destination))
    }

    func refresh() {
        if !navigating, !needsLoad, let current = webView.url {
            url = current.absoluteString
        }
        title = webView.title.flatMap { $0.isEmpty ? nil : $0 } ??
            webView.url?.host ?? "New Tab"
        canGoBack = webView.canGoBack
        canGoForward = webView.canGoForward
        isLoading = webView.isLoading
        onChange?()
    }

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction) async -> WKNavigationActionPolicy {
        guard available, let url = navigationAction.request.url,
              ["http", "https", "about", "blob"].contains(url.scheme?.lowercased() ?? "") else {
            error = available ? "This address cannot open in the browser pane." : nil
            return .cancel
        }
        if let failure = profile.prepare(url) {
            error = failure
            return .cancel
        }
        return .allow
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError failure: Error) {
        if (failure as NSError).code != NSURLErrorCancelled { error = failure.localizedDescription }
        refresh()
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError failure: Error) {
        if (failure as NSError).code != NSURLErrorCancelled { error = failure.localizedDescription }
        refresh()
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        navigating = false
        error = nil
        refresh()
    }

    func webView(_ webView: WKWebView, didCommit navigation: WKNavigation!) {
        navigating = false
        refresh()
    }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        error = "The page stopped. Reload to continue."
    }

    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                 for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
        if let url = navigationAction.request.url, navigationAction.targetFrame == nil {
            onOpen?(url)
        }
        return nil
    }
}

@MainActor
@Observable
final class ZZBrowserPaneRuntime {
    private(set) var tabs: [ZZBrowserTab] = []
    private(set) var activeTab = 0
    private(set) var route: ZZBrowserRoute = .unavailable
    let profile: ZZBrowserProfile
    let profileName: String
    var onDescriptorChange: ((ZZBrowserDescriptor) -> Void)?
    @ObservationIgnored private var lastReceived: ZZBrowserDescriptor
    @ObservationIgnored private var pending: [ZZBrowserDescriptor] = []
    @ObservationIgnored private var applying = false

    init(descriptor: ZZBrowserDescriptor, profile: ZZBrowserProfile) {
        self.profile = profile
        profileName = descriptor.profile
        lastReceived = descriptor.normalized
        tabs = descriptor.normalized.tabs.map { makeTab($0) }
        activeTab = descriptor.normalized.activeTab
    }

    var selected: ZZBrowserTab { tabs[activeTab] }
    var descriptor: ZZBrowserDescriptor {
        ZZBrowserDescriptor(tabs: tabs.map(\.url), activeTab: activeTab, profile: profileName)
    }

    func apply(_ incoming: ZZBrowserDescriptor, route nextRoute: ZZBrowserRoute) {
        let incoming = incoming.normalized
        applying = true
        defer { applying = false }
        setRoute(nextRoute)
        if incoming != lastReceived {
            lastReceived = incoming
            if let acknowledged = pending.firstIndex(of: incoming) {
                pending.removeFirst(acknowledged + 1)
            } else {
                pending.removeAll()
                for index in incoming.tabs.indices {
                    if index < tabs.count {
                        if tabs[index].url != incoming.tabs[index] { tabs[index].load(incoming.tabs[index]) }
                    } else {
                        tabs.append(makeTab(incoming.tabs[index]))
                    }
                }
                for tab in tabs.dropFirst(incoming.tabs.count) { tab.webView.stopLoading() }
                tabs = Array(tabs.prefix(incoming.tabs.count))
                activeTab = incoming.activeTab
            }
        }
        selected.resume()
    }

    func setRoute(_ next: ZZBrowserRoute) {
        guard route != next else { return }
        route = next
        profile.apply(next)
        for tab in tabs {
            tab.available = next.available
            if !next.available { tab.webView.stopLoading() }
            else if let destination = URL(string: tab.url), let failure = profile.prepare(destination) {
                tab.error = failure
            }
        }
        if next.available { selected.resume() }
    }

    func select(_ id: UUID) {
        guard let index = tabs.firstIndex(where: { $0.id == id }) else { return }
        selected.webView.endEditing(true)
        activeTab = index
        selected.resume()
        publish()
    }

    func add(_ url: String = "about:blank") {
        selected.webView.endEditing(true)
        tabs.append(makeTab(url))
        activeTab = tabs.count - 1
        selected.load(url)
        publish()
    }

    func close(_ id: UUID) {
        guard let index = tabs.firstIndex(where: { $0.id == id }) else { return }
        tabs[index].webView.stopLoading()
        tabs[index].webView.endEditing(true)
        tabs.remove(at: index)
        if tabs.isEmpty { tabs = [makeTab("about:blank")] }
        if index < activeTab { activeTab -= 1 }
        activeTab = min(activeTab, tabs.count - 1)
        selected.resume()
        publish()
    }

    func navigate(_ address: String) {
        selected.load(address)
        publish()
    }

    func suspend() {
        setRoute(.unavailable)
        tabs.forEach { $0.webView.endEditing(true) }
    }

    func close() {
        for tab in tabs {
            tab.available = false
            tab.webView.stopLoading()
            tab.webView.endEditing(true)
            tab.onChange = nil
            tab.onOpen = nil
        }
    }

    private func makeTab(_ url: String) -> ZZBrowserTab {
        let tab = ZZBrowserTab(url: url, profile: profile)
        tab.available = route.available
        tab.onChange = { [weak self] in self?.publish() }
        tab.onOpen = { [weak self] in self?.add($0.absoluteString) }
        return tab
    }

    private func publish() {
        guard !applying else { return }
        let value = descriptor
        guard value != (pending.last ?? lastReceived) else { return }
        pending.append(value)
        onDescriptorChange?(value)
    }
}

struct BrowserPaneView: View {
    @EnvironmentObject private var store: ZZStore
    let pane: ZZPane
    let runtime: ZZBrowserPaneRuntime
    @State private var address = ""
    @FocusState private var editingAddress: Bool

    var body: some View {
        VStack(spacing: 0) {
            ScrollView(.horizontal) {
                HStack(spacing: 2) {
                    ForEach(runtime.tabs) { tab in
                        HStack(spacing: 0) {
                            Button { editingAddress = false; runtime.select(tab.id) } label: {
                                Text(tab.title).lineLimit(1).frame(maxWidth: 150)
                                    .padding(.horizontal, 10).frame(minHeight: 44)
                            }
                            .accessibilityIdentifier("browser-tab-\(pane.id)-\(tab.id)")
                            Button { editingAddress = false; runtime.close(tab.id) } label: {
                                Image(systemName: "xmark").frame(width: 44, height: 44)
                            }
                            .accessibilityLabel("Close \(tab.title)")
                        }
                        .background(tab.id == runtime.selected.id ? Color.accentColor.opacity(0.12) : .clear)
                    }
                    Button { runtime.add(); address = ""; editingAddress = true } label: {
                        Image(systemName: "plus").frame(width: 44, height: 44)
                    }
                    .accessibilityLabel("New Tab")
                    .accessibilityIdentifier("browser-new-tab-\(pane.id)")
                }
            }
            .scrollIndicators(.hidden)
            .disabled(!runtime.route.available)
            HStack(spacing: 0) {
                Button { runtime.selected.webView.goBack() } label: {
                    Image(systemName: "chevron.left").frame(width: 44, height: 44)
                }
                .disabled(!runtime.selected.canGoBack)
                .accessibilityLabel("Back")
                Button { runtime.selected.webView.goForward() } label: {
                    Image(systemName: "chevron.right").frame(width: 44, height: 44)
                }
                .disabled(!runtime.selected.canGoForward)
                .accessibilityLabel("Forward")
                TextField("Address", text: $address)
                    .textInputAutocapitalization(.never).autocorrectionDisabled()
                    .keyboardType(.URL).submitLabel(.go).focused($editingAddress)
                    .textFieldStyle(.roundedBorder)
                    .accessibilityIdentifier("browser-address-\(pane.id)")
                    .onSubmit { runtime.navigate(address); editingAddress = false }
                Button {
                    if runtime.selected.isLoading { runtime.selected.webView.stopLoading() }
                    else { runtime.navigate(runtime.selected.url) }
                } label: {
                    Image(systemName: runtime.selected.isLoading ? "xmark" : "arrow.clockwise")
                        .frame(width: 44, height: 44)
                }
                .accessibilityLabel(runtime.selected.isLoading ? "Stop Loading" : "Reload")
            }
            .padding(.horizontal, 4)
            .disabled(!runtime.route.available)
            if !runtime.route.available {
                Text("Waiting for the host connection…").font(.caption).padding(8)
            } else if let error = runtime.selected.error {
                Text(error).font(.caption).foregroundStyle(.red).padding(8)
                    .accessibilityIdentifier("browser-error-\(pane.id)")
            }
            BrowserWebSurface(webView: runtime.selected.webView)
                .id(runtime.selected.id)
                .allowsHitTesting(runtime.route.available && store.isConnected)
        }
        .buttonStyle(.plain)
        .background(Color(uiColor: .systemBackground))
        .onChange(of: runtime.selected.url, initial: true) {
            if !editingAddress {
                address = runtime.selected.url == "about:blank" ? "" : runtime.selected.url
            }
        }
        .onChange(of: editingAddress) {
            if editingAddress { store.releaseTerminalInput() }
        }
        .onDisappear { runtime.selected.webView.endEditing(true) }
    }
}

private struct BrowserWebSurface: UIViewRepresentable {
    let webView: WKWebView

    func makeUIView(context: Context) -> WKWebView { webView }
    func updateUIView(_ uiView: WKWebView, context: Context) {}
}

struct BrowserPanePreview: View {
    let descriptor: ZZBrowserDescriptor

    var body: some View {
        VStack(spacing: 10) {
            Image(systemName: "globe").font(.title)
            Text(descriptor.url == "about:blank" ? "New Tab" : descriptor.url)
                .font(.caption).lineLimit(2)
            Text("\(descriptor.tabs.count) tab\(descriptor.tabs.count == 1 ? "" : "s")")
                .font(.caption2).foregroundStyle(.secondary)
        }
        .padding().frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
