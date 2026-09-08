import CZZClient
import Foundation
import Observation

public struct NativeUpdateResult: Decodable, Sendable {
    public let current: String
    public let channel: String
    public let state: String
    public let version: String?
    public let release_url: String?
    public let asset: Asset?
    public let error: String?
    public struct Asset: Decodable, Sendable { public let name: String; public let url: String }
}

@Observable @MainActor
public final class NativeUpdate {
    public private(set) var result: NativeUpdateResult?
    public private(set) var checking = false
    public private(set) var error: String?
    @ObservationIgnored private var nextCheck = Date().addingTimeInterval(10)
    @ObservationIgnored private var task: Task<Void, Never>?
    public init() {}

    func poll(enabled: Bool) {
        guard enabled, zz_update_checks_enabled(), Date() >= nextCheck else { return }
        check()
    }
    public func check() {
        guard !checking else { return }
        checking = true
        error = nil
        nextCheck = Date().addingTimeInterval(24 * 60 * 60)
        task = Task { [weak self] in
            let data = await Task.detached(priority: .utility) {
                guard let json = zz_update_check_native() else { return Data() }
                defer { zz_json_free(json) }
                let bytes = zz_json_bytes(json)
                return bytes.ptr.map { Data(bytes: $0, count: bytes.len) } ?? Data()
            }.value
            guard let self else { return }
            do { self.result = try JSONDecoder().decode(NativeUpdateResult.self, from: data) } catch {
                self.error = error.localizedDescription
            }
            self.checking = false
            self.task = nil
        }
    }
}
