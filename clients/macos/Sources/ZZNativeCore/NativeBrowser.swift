import AppKit
import CZZClient
import CoreImage
import IOSurface
import Observation

public final class NativeBrowserFrame: @unchecked Sendable {
    private let handle: OpaquePointer
    public let image: CIImage?
    public let size: CGSize

    init(_ handle: OpaquePointer) {
        self.handle = handle
        let width = Int(zz_browser_frame_width(handle))
        let height = Int(zz_browser_frame_height(handle))
        size = CGSize(width: width, height: height)
        if let pointer = zz_browser_frame_surface(handle) {
            let surface = Unmanaged<IOSurfaceRef>.fromOpaque(pointer).takeUnretainedValue()
            image = CIImage(ioSurface: surface, options: [.colorSpace: CGColorSpace(name: CGColorSpace.sRGB)!])
        } else {
            let bytes = zz_browser_frame_bgra(handle)
            if let pointer = bytes.ptr, width > 0, height > 0 {
                let data = Data(
                    bytesNoCopy: UnsafeMutableRawPointer(mutating: pointer), count: bytes.len, deallocator: .none)
                image = CIImage(
                    bitmapData: data, bytesPerRow: width * 4, size: size, format: .BGRA8,
                    colorSpace: CGColorSpace(name: CGColorSpace.sRGB))
            } else {
                image = nil
            }
        }
    }

    deinit { zz_browser_frame_free(handle) }
}

@Observable @MainActor
public final class NativeBrowserTab: Identifiable {
    public let id = UUID().uuidString
    public var url: String
    public var title = "New tab"
    public var loading = false
    public var canGoBack = false
    public var canGoForward = false
    public var error: String?
    public var frame: NativeBrowserFrame?
    public var cursor = "Arrow"
    public var picking = false
    public var contextMenu: [String: Any]?
    public var contextMenuVersion = 0
    public var session: UInt64 = 0
    @ObservationIgnored var pendingCommands: [String] = []
    @ObservationIgnored var pendingProtocol: [String] = []
    @ObservationIgnored var viewport = CGSize(width: 800, height: 600)
    @ObservationIgnored var scale: CGFloat = 2
    @ObservationIgnored var screenOrigin = CGPoint.zero
    @ObservationIgnored var focused = false
    @ObservationIgnored var visible = false

    init(url: String) { self.url = url }
}

public struct NativeBrowserHistoryEntry: Decodable, Identifiable, Equatable, Sendable {
    public let url: String
    public let title: String
    public let display_url: String
    public let inline_completion: String?
    public var id: String { url }
}

public struct NativeChromeProfile: Decodable, Identifiable, Sendable {
    public let zz_profile: String
    public let display_name: String
    public let email: String?
    public var id: String { zz_profile }
    public var label: String {
        guard let email, email.caseInsensitiveCompare(display_name) != .orderedSame else { return display_name }
        return "\(display_name) · \(email)"
    }
}

public struct NativeChromeImportResult: Decodable, Identifiable, Sendable {
    public let request_id: UInt64
    public let profile: String
    public let history_imported: Int
    public let history_skipped: Int
    public let cookies_imported: Int
    public let cookies_skipped: Int
    public let cookies_rejected: Int
    public let persisted: Bool
    public let errors: [String]
    public let permission_denied: Bool
    public var id: UInt64 { request_id }
}

@Observable @MainActor
public final class NativeBrowserPane: Identifiable {
    public let id: UInt64
    public var tabs: [NativeBrowserTab]
    public var selection: String
    public var profile: String
    public var address: String
    public var addressFocus = 0
    public var addressBlur = 0
    public var addressEditing = false
    public var chromeImportRequest: UInt64?
    public var pickRequest = 0
    @ObservationIgnored var remote: NativeBrowserDescriptor
    @ObservationIgnored var pending: NativeBrowserDescriptor?
    @ObservationIgnored var pendingRequest: UInt64 = 0
    public var activeTab: NativeBrowserTab? { tabs.first(where: { $0.id == selection }) }
    var descriptor: NativeBrowserDescriptor {
        NativeBrowserDescriptor(
            tabs: tabs.map(\.url), active_tab: tabs.firstIndex(where: { $0.id == selection }) ?? 0, profile: profile)
    }

    init(id: UInt64, descriptor: NativeBrowserDescriptor) {
        self.id = id
        remote = descriptor
        profile = descriptor.profile
        let tabs = (descriptor.tabs.isEmpty ? ["about:blank"] : descriptor.tabs).map(NativeBrowserTab.init)
        self.tabs = tabs
        let selected = tabs[min(max(0, descriptor.active_tab), tabs.count - 1)]
        selection = selected.id
        address = selected.url == "about:blank" ? "" : selected.url
    }
}

@Observable @MainActor
public final class NativeBrowserEngine {
    public private(set) var panes: [UInt64: NativeBrowserPane] = [:]
    public private(set) var error: String?
    public var searchProvider = "google"
    public var remoteEgress = true {
        didSet { if oldValue != remoteEgress { synchronizeEgress() } }
    }
    public private(set) var chromeProfiles: [NativeChromeProfile] = []
    public private(set) var chromeProfilesLoading = false
    public private(set) var chromeProfilesError: String?
    public private(set) var chromeImports: [UInt64: NativeChromeImportResult] = [:]
    public private(set) var pendingImports: [UInt64: String] = [:]
    public private(set) var historyRevision = 0
    @ObservationIgnored private weak var client: NativeClient?
    @ObservationIgnored private var runtime: OpaquePointer?
    @ObservationIgnored private var timer: Timer?
    @ObservationIgnored private var visible = Set<UInt64>()
    @ObservationIgnored private var terminating = false
    @ObservationIgnored private var endpoint = ""
    @ObservationIgnored private var connectionGeneration = -1
    @ObservationIgnored private var profilesRequest: UInt64 = 0

    public init() {}

    func reconcile(_ source: [NativePane], client: NativeClient) {
        self.client = client
        if endpoint != client.endpoint {
            for pane in panes.values { for tab in pane.tabs { close(tab) } }
            panes.removeAll()
            endpoint = client.endpoint
        }
        synchronizeEgress()
        let ids = Set(source.filter { $0.browser != nil }.map(\.id))
        for id in Set(panes.keys).subtracting(ids) {
            if let pane = panes.removeValue(forKey: id) { for tab in pane.tabs { close(tab) } }
            visible.remove(id)
        }
        for item in source {
            guard let descriptor = item.browser else { continue }
            guard let pane = panes[item.id] else {
                panes[item.id] = NativeBrowserPane(id: item.id, descriptor: descriptor)
                continue
            }
            if descriptor == pane.pending {
                pane.remote = descriptor
                pane.pending = nil
                pane.pendingRequest = 0
                persist(pane)
                continue
            }
            guard descriptor != pane.remote, pane.pending == nil else { continue }
            if pane.profile != descriptor.profile {
                for tab in pane.tabs { close(tab) }
                panes[item.id] = NativeBrowserPane(id: item.id, descriptor: descriptor)
                continue
            }
            let old = pane.tabs
            var unused = old
            pane.tabs = (descriptor.tabs.isEmpty ? ["about:blank"] : descriptor.tabs).map { url in
                if let index = unused.firstIndex(where: { $0.url == url }) { return unused.remove(at: index) }
                return NativeBrowserTab(url: url)
            }
            for tab in unused { close(tab) }
            pane.remote = descriptor
            select(pane, tab: pane.tabs[min(max(0, descriptor.active_tab), pane.tabs.count - 1)], persist: false)
        }
    }

    public func setVisible(_ pane: UInt64, _ value: Bool) {
        if value { visible.insert(pane); start() } else { visible.remove(pane) }
        updateVisibility()
    }

    private func start() {
        guard !terminating, runtime == nil else { return }
        var message = [CChar](repeating: 0, count: 4096)
        runtime = zz_browser_runtime_new(nil, &message, message.count)
        guard runtime != nil else {
            error = String(decoding: message.prefix(while: { $0 != 0 }).map { UInt8(bitPattern: $0) }, as: UTF8.self)
            return
        }
        error = nil
        let timer = Timer(timeInterval: 1.0 / 60.0, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.pump() }
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
        pump()
    }

    private func pump() {
        guard let runtime, !terminating else { return }
        synchronizeEgress()
        if zz_browser_runtime_pump(runtime), client?.connected == true {
            for pane in panes.values where visible.contains(pane.id) {
                for tab in pane.tabs where tab.session == 0 {
                    tab.session = pane.profile.withCString { profile in
                        tab.url.withCString { url in
                            zz_browser_session_new(
                                runtime, profile, url, UInt32(max(1, tab.viewport.width)),
                                UInt32(max(1, tab.viewport.height)), Float(tab.scale))
                        }
                    }
                    if tab.session != 0 { updateViewport(tab) }
                }
            }
        }
        while let event = zz_browser_event_next(runtime) {
            let bytes = zz_browser_event_json(event)
            if let pointer = bytes.ptr,
                let value = try? JSONSerialization.jsonObject(with: Data(bytes: pointer, count: bytes.len))
                    as? [String: Any]
            {
                let imageBytes = zz_browser_event_image(event)
                let image = imageBytes.ptr.map { Data(bytes: $0, count: imageBytes.len) }
                receive(value, image: image)
            }
            zz_browser_event_free(event)
        }
        for pane in panes.values {
            for tab in pane.tabs where tab.session != 0 {
                if let frame = zz_browser_frame_take(runtime, tab.session) { tab.frame = NativeBrowserFrame(frame) }
            }
        }
        let message = NativeClient.string(zz_browser_runtime_error(runtime))
        if !message.isEmpty { error = message }
    }

    private func receive(_ value: [String: Any], image: Data?) {
        guard let kind = value["kind"] as? String else { return }
        if kind == "chrome-profiles" {
            guard (value["request_id"] as? NSNumber)?.uint64Value == profilesRequest else { return }
            chromeProfilesLoading = false
            chromeProfilesError = value["error"] as? String
            if let data = try? JSONSerialization.data(withJSONObject: value["profiles"] ?? []),
                let profiles = try? JSONDecoder().decode([NativeChromeProfile].self, from: data)
            {
                chromeProfiles = profiles
            }
            return
        }
        if kind == "chrome-import" {
            if let data = try? JSONSerialization.data(withJSONObject: value),
                let result = try? JSONDecoder().decode(NativeChromeImportResult.self, from: data),
                pendingImports[result.id] == result.profile
            {
                pendingImports.removeValue(forKey: result.id)
                chromeImports[result.id] = result
                historyRevision += 1
            }
            return
        }
        guard let id = (value["session"] as? NSNumber)?.uint64Value,
            let pane = panes.values.first(where: { $0.tabs.contains(where: { $0.session == id }) }),
            let tab = pane.tabs.first(where: { $0.session == id })
        else { return }
        switch kind {
        case "ready":
            updateVisibility()
            let commands = tab.pendingCommands
            tab.pendingCommands.removeAll()
            for command in commands { dispatch(tab, command: command) }
            let protocolCommands = tab.pendingProtocol
            tab.pendingProtocol.removeAll()
            for command in protocolCommands { dispatchProtocol(tab, command: command) }
        case "address":
            if let url = value["url"] as? String {
                tab.url = url
                if pane.activeTab === tab && !pane.addressEditing { pane.address = url == "about:blank" ? "" : url }
                tab.error = nil
                historyRevision += 1
                persist(pane)
            }
        case "title":
            tab.title = value["title"] as? String ?? "New tab"
            historyRevision += 1
        case "loading":
            tab.loading = value["loading"] as? Bool ?? false
            tab.canGoBack = value["back"] as? Bool ?? false
            tab.canGoForward = value["forward"] as? Bool ?? false
        case "cursor": tab.cursor = value["cursor"] as? String ?? "Arrow"
        case "crashed", "load-error":
            if (value["code"] as? Int) != -3 {
                tab.error = value["message"] as? String ?? "The page could not be loaded."
            }
        case "popup", "popup-request":
            let child = NativeBrowserTab(url: value["url"] as? String ?? "about:blank")
            child.session = (value["popup"] as? NSNumber)?.uint64Value ?? 0
            pane.tabs.append(child)
            if value["foreground"] as? Bool == true { select(pane, tab: child) } else { persist(pane) }
        case "context-menu": tab.contextMenu = value; tab.contextMenuVersion += 1
        case "picked":
            tab.picking = false
            NSPasteboard.general.clearContents()
            if let text = value["text"] as? String { NSPasteboard.general.setString(text, forType: .string) }
            if let image { NSPasteboard.general.setData(image, forType: .png) }
        case "pick-cancelled", "pick-error": tab.picking = false
        case "closed": tab.session = 0
        default: break
        }
    }

    public func select(_ pane: NativeBrowserPane, tab: NativeBrowserTab, persist shouldPersist: Bool = true) {
        pane.selection = tab.id
        pane.addressEditing = false
        pane.address = tab.url == "about:blank" ? "" : tab.url
        updateVisibility()
        if shouldPersist { persist(pane) }
    }

    public func newTab(_ pane: NativeBrowserPane, url: String = "about:blank") {
        let tab = NativeBrowserTab(url: url)
        pane.tabs.append(tab)
        select(pane, tab: tab)
        if url == "about:blank" { pane.addressFocus += 1 }
        pump()
    }

    public func closeTab(_ pane: NativeBrowserPane, id: String) {
        guard let index = pane.tabs.firstIndex(where: { $0.id == id }) else { return }
        close(pane.tabs.remove(at: index))
        if pane.tabs.isEmpty { pane.tabs.append(NativeBrowserTab(url: "about:blank")) }
        if pane.selection == id { select(pane, tab: pane.tabs[min(index, pane.tabs.count - 1)]) } else { persist(pane) }
    }

    private func close(_ tab: NativeBrowserTab) {
        if let runtime, tab.session != 0 { zz_browser_session_close(runtime, tab.session) }
        tab.session = 0
    }

    private func persist(_ pane: NativeBrowserPane) {
        guard let client, client.connected, pane.pending == nil, pane.descriptor != pane.remote else { return }
        pane.pending = pane.descriptor
        pane.pendingRequest = client.execute(
            "set-browser-tabs",
            ["-t", "%\(pane.id)", "-a", String(pane.descriptor.active_tab), "--"] + pane.descriptor.tabs)
        if pane.pendingRequest == 0 { pane.pending = nil }
    }

    func commandReply(_ request: UInt64, ok: Bool) {
        if !ok {
            for pane in panes.values where pane.pendingRequest == request {
                pane.pending = nil
                pane.pendingRequest = 0
            }
        }
    }

    private func synchronizeEgress() {
        guard let client, client.connected else { return }
        let reconnected = connectionGeneration != client.connectionGeneration
        connectionGeneration = client.connectionGeneration
        if reconnected {
            for pane in panes.values {
                pane.pending = nil
                pane.pendingRequest = 0
            }
        }
        guard let runtime else { return }
        let changed = client.endpoint.withCString {
            zz_browser_runtime_set_egress(runtime, client.handle, $0, remoteEgress)
        }
        guard changed || reconnected else { return }
        for pane in panes.values {
            for tab in pane.tabs { close(tab); tab.error = nil }
        }
    }

    public func history(_ pane: NativeBrowserPane, input: String? = nil, limit: Int = 100)
        -> [NativeBrowserHistoryEntry]
    {
        guard let runtime else { return [] }
        let value = pane.profile.withCString { profile in
            if let input {
                return input.withCString { zz_browser_history_suggestions(runtime, profile, $0, max(0, limit)) }
            }
            return zz_browser_history_recent(runtime, profile, max(0, limit))
        }
        guard let value else { return [] }
        defer { zz_json_free(value) }
        let bytes = zz_json_bytes(value)
        guard let pointer = bytes.ptr else { return [] }
        return
            (try? JSONDecoder().decode([NativeBrowserHistoryEntry].self, from: Data(bytes: pointer, count: bytes.len)))
            ?? []
    }

    public func removeHistory(_ pane: NativeBrowserPane, entry: NativeBrowserHistoryEntry) {
        guard let runtime else { return }
        if pane.profile.withCString({ profile in
            entry.url.withCString { zz_browser_history_remove(runtime, profile, $0) }
        }) {
            historyRevision += 1
        }
    }

    public func discoverChromeProfiles() {
        guard !chromeProfilesLoading else { return }
        start()
        guard let runtime else { chromeProfilesError = error ?? "The browser is unavailable."; return }
        profilesRequest = zz_browser_chrome_profiles(runtime)
        chromeProfilesLoading = profilesRequest != 0
        chromeProfilesError = profilesRequest == 0 ? "Chrome profiles could not be read." : nil
    }

    @discardableResult
    public func importChrome(_ pane: NativeBrowserPane, source: NativeChromeProfile) -> Bool {
        guard let runtime, let tab = pane.activeTab, tab.session != 0 else { return false }
        let request = source.zz_profile.withCString { zz_browser_import_chrome(runtime, tab.session, $0) }
        guard request != 0 else { return false }
        pane.chromeImportRequest = request
        pendingImports[request] = pane.profile
        return true
    }

    public func navigate(_ pane: NativeBrowserPane, suggestion: NativeBrowserHistoryEntry? = nil) {
        guard let tab = pane.activeTab else { return }
        var fields: [String: Any] = ["input": pane.address, "provider": searchProvider, "selected": suggestion != nil]
        if let suggestion { fields["url"] = suggestion.url }
        action(tab, "submit-address", fields)
        pane.addressEditing = false
        pane.addressBlur += 1
        tab.error = nil
    }

    public func action(_ tab: NativeBrowserTab, _ action: String, _ fields: [String: Any] = [:]) {
        var payload = fields
        payload["action"] = action
        guard let data = try? JSONSerialization.data(withJSONObject: payload),
            let json = String(data: data, encoding: .utf8)
        else { return }
        if tab.session == 0 { tab.pendingCommands.append(json); start() } else { dispatch(tab, command: json) }
    }

    private func dispatch(_ tab: NativeBrowserTab, command: String) {
        guard let runtime, tab.session != 0 else { return }
        if !command.withCString({ zz_browser_session_action(runtime, tab.session, $0) }) {
            tab.error = "The browser could not complete that action."
        }
    }

    private func updateVisibility() {
        for pane in panes.values {
            for tab in pane.tabs {
                tab.visible = visible.contains(pane.id) && pane.activeTab === tab
                updateViewport(tab)
            }
        }
    }

    public func viewport(_ tab: NativeBrowserTab, size: CGSize, scale: CGFloat, screen: CGPoint, focused: Bool) {
        tab.viewport = size
        tab.scale = scale
        tab.screenOrigin = screen
        tab.focused = focused
        updateViewport(tab)
    }

    private func updateViewport(_ tab: NativeBrowserTab) {
        guard let runtime, tab.session != 0 else { return }
        zz_browser_session_viewport(
            runtime, tab.session, UInt32(max(1, tab.viewport.width)), UInt32(max(1, tab.viewport.height)),
            Float(tab.scale), 1,
            Int32(clamping: Int(tab.screenOrigin.x)), Int32(clamping: Int(tab.screenOrigin.y)), tab.visible,
            tab.visible && tab.focused)
    }

    public func pointer(
        _ tab: NativeBrowserTab, point: CGPoint, phase: UInt32, button: UInt32, clicks: Int, flags: UInt8
    ) {
        guard let runtime, tab.session != 0 else { return }
        zz_browser_session_pointer(
            runtime, tab.session, Int32(clamping: Int(point.x)), Int32(clamping: Int(point.y)), phase, button,
            Int32(clamping: clicks), flags)
    }

    public func wheel(_ tab: NativeBrowserTab, point: CGPoint, delta: CGPoint, precise: Bool, flags: UInt8) {
        guard let runtime, tab.session != 0 else { return }
        zz_browser_session_wheel(
            runtime, tab.session, Int32(clamping: Int(point.x)), Int32(clamping: Int(point.y)),
            Int32(clamping: Int(delta.x)), Int32(clamping: Int(delta.y)), precise, flags)
    }

    func handle(_ value: [String: Any], client: NativeClient) {
        guard value["kind"] as? String == "browser", let id = (value["pane"] as? NSNumber)?.uint64Value,
            let pane = panes[id], let tab = pane.activeTab, let command = value["command"]
        else { return }
        if let object = command as? [String: Any] {
            if let screenshot = object["Screenshot"] as? [String: Any],
                let request = (screenshot["request_id"] as? NSNumber)?.uint64Value
            {
                do {
                    guard let path = screenshot["path"] as? String, let frame = tab.frame, let image = frame.image
                    else { throw CocoaError(.fileReadUnknown) }
                    try CIContext().writePNGRepresentation(
                        of: image, to: URL(fileURLWithPath: path), format: .RGBA8,
                        colorSpace: CGColorSpace(name: CGColorSpace.sRGB)!)
                    client.guiResponse(request, ok: true, text: path)
                } catch { client.guiResponse(request, ok: false, text: error.localizedDescription) }
            }
        }
        if let object = command as? [String: Any], object["Screenshot"] != nil { return }
        if let data = try? JSONSerialization.data(withJSONObject: command, options: .fragmentsAllowed),
            let json = String(data: data, encoding: .utf8)
        {
            dispatchProtocol(tab, command: json)
        }
    }

    private func dispatchProtocol(_ tab: NativeBrowserTab, command: String) {
        guard let runtime, tab.session != 0 else { tab.pendingProtocol.append(command); return }
        _ = command.withCString { zz_browser_session_dispatch(runtime, tab.session, $0) }
    }

    public func shutdown() -> Bool {
        terminating = true
        timer?.invalidate()
        timer = nil
        guard let runtime else { return true }
        if zz_browser_runtime_free(runtime) { self.runtime = nil; return true }
        return false
    }
}
