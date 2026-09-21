import Foundation
import Observation

enum ZZSettingValue: Codable, Equatable {
    case string(String), number(Double), boolean(Bool), null

    init(from decoder: Decoder) throws {
        let value = try decoder.singleValueContainer()
        if value.decodeNil() { self = .null }
        else if let bool = try? value.decode(Bool.self) { self = .boolean(bool) }
        else if let number = try? value.decode(Double.self) { self = .number(number) }
        else { self = .string(try value.decode(String.self)) }
    }

    func encode(to encoder: Encoder) throws {
        var value = encoder.singleValueContainer()
        switch self {
        case .string(let text): try value.encode(text)
        case .number(let number): try value.encode(number)
        case .boolean(let bool): try value.encode(bool)
        case .null: try value.encodeNil()
        }
    }

    var text: String {
        switch self {
        case .string(let text): text
        case .number(let number): Int64(exactly: number).map(String.init) ?? String(number)
        case .boolean(let bool): bool ? "true" : "false"
        case .null: ""
        }
    }
    var number: Double? { if case .number(let value) = self { value } else { Double(text) } }
    var bool: Bool? { if case .boolean(let value) = self { value } else { nil } }
}

struct ZZSetting: Decodable {
    let key: String
    let value: ZZSettingValue
    let overridden: Bool
}

struct ZZSettingsSnapshot: Decodable {
    let revision: UInt64
    let settings: [ZZSetting]
    let terminal_source: String?
    let editor_error: String?
    let error: String?
}

struct ZZTerminalTheme: Decodable, Identifiable {
    var id: String { name }
    let name: String
    let path: String
    let background: String
    let foreground: String
}

struct ZZMobileTerminalAppearance: Decodable, Equatable {
    let font_size: Double
    let font_weight: UInt16
    let background_opacity: Double
    let padding: [Double]
    let cursor_blink_ms: UInt64
    let cursor_style: String?
    let cursor_blink_policy: String?
    let foreground: UInt32
    let background: UInt32
    let cursor_color: UInt32
    let palette: [UInt32]
    let selection_background: UInt32?
    let selection_foreground: UInt32?
    let search_match_color: UInt32?
    let search_current_color: UInt32?
    let copy_cursor_color: UInt32?
}

private final class ZZSettingsHandle {
    let pointer: OpaquePointer?
    init(_ pointer: OpaquePointer?) { self.pointer = pointer }
    deinit { zz_settings_model_free(pointer) }
}

@Observable @MainActor
final class ZZSharedSettings {
    private(set) var snapshot: ZZSettingsSnapshot?
    private(set) var mobileAppearance: ZZMobileTerminalAppearance?
    private(set) var themes: [ZZTerminalTheme] = []
    private(set) var error: String?
    private(set) var terminalPreferencesReady = false
    var dark = true {
        didSet { if dark != oldValue { refreshAppearance() } }
    }
    @ObservationIgnored private let storage: ZZSettingsHandle
    let directory: URL
    let themeDirectory: String
    var handle: OpaquePointer? { storage.pointer }
    var revision: UInt64 { snapshot?.revision ?? 0 }

    init(directory: URL? = nil) {
        self.directory = directory ?? URL.applicationSupportDirectory.appending(path: "zz", directoryHint: .isDirectory)
        themeDirectory = Bundle.main.resourceURL?.appending(path: "themes").path ?? ""
        let config = self.directory.appending(path: "config").path
        let mux = self.directory.appending(path: "mux.conf").path
        storage = ZZSettingsHandle(config.withCString { config in
            mux.withCString { mux in
                "System".withCString { zz_settings_model_new($0, config, mux) }
            }
        })
        do { try FileManager.default.createDirectory(at: self.directory, withIntermediateDirectories: true) }
        catch { self.error = error.localizedDescription }
        refresh()
        if let source = snapshot?.terminal_source {
            let retained = source.split(separator: "\n", omittingEmptySubsequences: false).filter { line in
                let key = line.split(separator: "=", maxSplits: 1).first?
                    .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                return ["font-family", "font-size", "theme"].contains(key)
            }.joined(separator: "\n")
            terminalPreferencesReady = source.trimmingCharacters(in: .whitespacesAndNewlines) == retained
                || action("save-terminal", ["source": retained])
        } else {
            terminalPreferencesReady = error == nil
        }
        refreshAppearance()
        themes = themeDirectory.withCString {
            decode(zz_settings_model_themes_json(handle, $0, dark), as: [ZZTerminalTheme].self)
        } ?? []
    }

    func value(_ key: String) -> ZZSettingValue {
        snapshot?.settings.first(where: { $0.key == key })?.value ?? .null
    }
    func number(_ key: String, fallback: Double = 0) -> Double { value(key).number ?? fallback }
    func text(_ key: String) -> String { value(key).text }

    func refresh() {
        guard let next = decode(zz_settings_model_snapshot(handle, nil), as: ZZSettingsSnapshot.self) else { return }
        snapshot = next
        error = next.error ?? next.editor_error
        refreshAppearance()
    }

    func refreshAppearance() {
        guard terminalPreferencesReady else {
            mobileAppearance = nil
            return
        }
        mobileAppearance = themeDirectory.withCString {
            decode(zz_settings_model_terminal_appearance_json(handle, $0, dark), as: ZZMobileTerminalAppearance.self)
        }
    }

    @discardableResult
    func action(_ name: String, _ fields: [String: Any] = [:]) -> Bool {
        var object = fields
        object["action"] = name
        guard let data = try? JSONSerialization.data(withJSONObject: object),
              let source = String(data: data, encoding: .utf8) else { return false }
        let success = source.withCString {
            zz_settings_model_action(handle, nil, nil, $0)
        }
        refresh()
        return success
    }

    private func decode<T: Decodable>(_ json: OpaquePointer?, as type: T.Type) -> T? {
        guard let json else { return nil }
        defer { zz_json_free(json) }
        let bytes = zz_json_bytes(json)
        guard let pointer = bytes.ptr else { return nil }
        do { return try JSONDecoder().decode(type, from: Data(bytes: pointer, count: bytes.len)) }
        catch { self.error = error.localizedDescription; return nil }
    }
}
