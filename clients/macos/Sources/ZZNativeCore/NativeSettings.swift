import AppKit
import CZZClient
import Observation

public enum NativeSettingValue: Codable, Equatable {
    case string(String), number(Double), boolean(Bool), null

    public init(from decoder: Decoder) throws {
        let value = try decoder.singleValueContainer()
        if value.decodeNil() {
            self = .null
        } else if let bool = try? value.decode(Bool.self) {
            self = .boolean(bool)
        } else if let number = try? value.decode(Double.self) {
            self = .number(number)
        } else {
            self = .string(try value.decode(String.self))
        }
    }
    public func encode(to encoder: Encoder) throws {
        var value = encoder.singleValueContainer()
        switch self {
        case .string(let text): try value.encode(text)
        case .number(let number): try value.encode(number)
        case .boolean(let bool): try value.encode(bool)
        case .null: try value.encodeNil()
        }
    }
    public var text: String {
        switch self {
        case .string(let text): text
        case .number(let number): String(number)
        case .boolean(let bool): String(bool)
        case .null: ""
        }
    }
    public var number: Double? { if case .number(let value) = self { value } else { nil } }
    public var bool: Bool? { if case .boolean(let value) = self { value } else { nil } }
}

public struct NativeSetting: Decodable, Identifiable {
    public var id: String { key }
    public let key: String
    public let title: String
    public let section: String
    public let value: NativeSettingValue
    public let default_value: NativeSettingValue
    public let overridden: Bool
    public let enabled: Bool
    public let control: String
    public let range: [Double]?
    public let choices: [Choice]
    public struct Choice: Decodable { public let value: String; public let title: String }
}

public struct NativeSettingsSnapshot: Decodable {
    public let revision: UInt64
    public let settings: [NativeSetting]
    public let config_path: String?
    public let mux_path: String?
    public let terminal_source: String?
    public let mux_source: String?
    public let editor_error: String?
    public let error: String?
    public let hosts: [Host]
    public let diagnostics: [Diagnostic]
    public let ghostty_path: String?
    public let mux_sources: [String]
    public let prefix_bindings: [Binding]
    public let horizontal: SplitBinding
    public let vertical: SplitBinding
    public let presets: [Preset]
    public let version: String
    public let platform: String
    public let architecture: String
    public struct Host: Decodable, Identifiable {
        public var id: String { name }
        public let name: String
        public let endpoint: String
    }
    public struct Diagnostic: Decodable { public let line: Int; public let message: String }
    public struct Binding: Decodable { public let key: String; public let command: String }
    public struct SplitBinding: Decodable {
        public let key: String?
        public let kind: String?
        public let editable: Bool
    }
    public struct Preset: Decodable, Identifiable {
        public let id: String
        public let name: String
        public let dark: Bool
        public let background: String
        public let foreground: String
        public let accent: String
        public let success: String
        public let warning: String
        public let danger: String
    }
}

public struct NativeTerminalAppearance: Decodable, Equatable {
    public var font_families: [String] = []
    public var font_size: Double = 14
    public var font_weight: UInt16 = 400
    public var background_opacity: Double = 1
    public var padding: [Double] = [10, 10, 10, 10]
    public var cursor_blink_ms: UInt64 = 600
    public init() {}
}

private final class SettingsHandle {
    let pointer: OpaquePointer?
    init(_ pointer: OpaquePointer?) { self.pointer = pointer }
    deinit { zz_settings_model_free(pointer) }
}

@Observable @MainActor
public final class NativeSettings {
    public let updates = NativeUpdate()
    public private(set) var snapshot: NativeSettingsSnapshot?
    public private(set) var error: String?
    public var terminalDraft = ""
    public var muxDraft = ""
    public private(set) var uiZoom = 100.0
    public func setUIZoom(_ percent: Double) { if percent.isFinite { uiZoom = min(300, max(50, percent)) } }
    public var visible = false
    public var section = "interface"
    @ObservationIgnored private let storage: SettingsHandle
    private var handle: OpaquePointer? { storage.pointer }
    @ObservationIgnored private weak var client: NativeClient?
    @ObservationIgnored private var timer: Timer?
    @ObservationIgnored private var terminalSaved = ""
    @ObservationIgnored private var muxSaved = ""

    public init(config: String? = nil, mux: String? = nil) {
        let configString = config.map { strdup($0) }
        let muxString = mux.map { strdup($0) }
        defer { if let configString { free(configString) }; if let muxString { free(muxString) } }
        storage = SettingsHandle(
            (NSFont.systemFont(ofSize: 13).familyName ?? "").withCString {
                zz_settings_model_new(
                    $0, configString.flatMap { UnsafePointer($0) }, muxString.flatMap { UnsafePointer($0) })
            })
        refresh()
    }

    public var terminalDirty: Bool { terminalDraft != terminalSaved }
    public var muxDirty: Bool { muxDraft != muxSaved }
    public func value(_ key: String) -> NativeSettingValue {
        snapshot?.settings.first { $0.key == key }?.value ?? .null
    }
    public func bool(_ key: String) -> Bool { value(key).bool ?? false }
    public func number(_ key: String, fallback: Double = 0) -> Double { value(key).number ?? fallback }
    public func text(_ key: String) -> String { value(key).text }

    func connect(_ client: NativeClient) {
        self.client = client
        if timer == nil {
            timer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] timer in
                guard let self else { timer.invalidate(); return }
                MainActor.assumeIsolated {
                    self.client?.sampleFrameRate()
                    self.updates.poll(enabled: self.bool("check-for-updates"))
                    if zz_settings_model_poll(self.handle) { self.apply(); self.refresh() }
                }
            }
        }
        apply()
        refresh()
    }
    func apply() {
        guard let client, let connection = client.handle else { return }
        _ = client.endpoint.withCString { zz_settings_model_apply(handle, connection, $0) }
    }
    public func refresh(discardDrafts: Bool = false) {
        guard let json = zz_settings_model_snapshot(handle, client?.handle) else {
            error = "Could not open settings. Configuration paths must be absolute."
            return
        }
        defer { zz_json_free(json) }
        let bytes = zz_json_bytes(json)
        guard let pointer = bytes.ptr else { return }
        do {
            let next = try JSONDecoder().decode(
                NativeSettingsSnapshot.self, from: Data(bytes: pointer, count: bytes.len))
            if discardDrafts || !terminalDirty {
                terminalDraft = next.terminal_source ?? ""; terminalSaved = terminalDraft
            }
            if discardDrafts || !muxDirty { muxDraft = next.mux_source ?? ""; muxSaved = muxDraft }
            snapshot = next
            client?.browsers.searchProvider = text("browser-search-provider")
            client?.browsers.remoteEgress = bool("browser-egress")
            error = next.error ?? next.editor_error
        } catch { self.error = error.localizedDescription }
    }
    @discardableResult public func action(_ name: String, _ fields: [String: Any] = [:]) -> Bool {
        var object = fields
        object["action"] = name
        guard let data = try? JSONSerialization.data(withJSONObject: object),
            let source = String(data: data, encoding: .utf8)
        else { return false }
        let ok = (client?.endpoint ?? "").withCString { endpoint in
            source.withCString { zz_settings_model_action(handle, client?.handle, endpoint, $0) }
        }
        client?.trackRequest(zz_settings_model_take_reload_request(handle))
        if ok {
            if name == "save-terminal" { terminalSaved = terminalDraft }
            if name == "save-mux" { muxSaved = muxDraft }
        }
        refresh()
        return ok
    }
    public func set(_ key: String, _ value: NativeSettingValue) {
        guard let data = try? JSONEncoder().encode(value),
            let object = try? JSONSerialization.jsonObject(with: data, options: .fragmentsAllowed)
        else { return }
        action("set", ["key": key, "value": object])
    }
    public func makeChromeKeymap() -> OpaquePointer? { zz_settings_model_chrome_keymap(handle) }
    public func reset(_ key: String) { action("reset", ["key": key]) }
    public func reloadDrafts() { _ = zz_settings_model_poll(handle); refresh(discardDrafts: true) }
}
