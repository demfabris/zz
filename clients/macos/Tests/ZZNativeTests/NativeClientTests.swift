import AppKit
import CZZClient
import Testing

@testable import ZZNativeCore

private struct WaitExpired: Error { let line: Int }

@MainActor
private func eventually(line: Int = #line, _ condition: () -> Bool) async throws {
    let deadline = ContinuousClock.now + .seconds(15)
    while !condition() {
        if ContinuousClock.now >= deadline { throw WaitExpired(line: line) }
        try await Task.sleep(for: .milliseconds(20))
    }
}

@Test @MainActor func defaultEndpointGetterPreservesSizingAndTermination() {
    let count = zz_client_default_endpoint(nil, 0)
    #expect(count > 0)
    var full = [CChar](repeating: -1, count: count + 1)
    #expect(zz_client_default_endpoint(&full, full.count) == count)
    #expect(full.last == 0)
    var tiny = [CChar](repeating: -1, count: 2)
    #expect(zz_client_default_endpoint(&tiny, tiny.count) == count)
    #expect(tiny[1] == 0)
    #expect(tiny[0] == full[0])
}

@Test(.enabled(if: ProcessInfo.processInfo.environment["ZZ_NATIVE_TEST_FIXTURE"] != nil))
@MainActor func realDaemonInputResizeNavigationAndReconnect() async throws {
    let binary = try #require(ProcessInfo.processInfo.environment["ZZ_NATIVE_TEST_FIXTURE"])
    let socket = "/tmp/zzn-\(UUID().uuidString.prefix(8)).sock"
    let ready = URL(fileURLWithPath: socket).deletingPathExtension().appendingPathExtension("ready").path
    func startFixture() throws -> Process {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: binary)
        process.arguments = [socket]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        return process
    }
    var process = try startFixture()
    let client = NativeClient(endpoint: socket, session: "native-fixture")
    defer {
        client.execute("kill-server")
        client.disconnect()
        if process.isRunning { process.terminate() }
        try? FileManager.default.removeItem(atPath: socket)
        try? FileManager.default.removeItem(atPath: ready)
    }
    try await eventually { FileManager.default.fileExists(atPath: ready) }
    client.connect()
    try await eventually { client.activePane != nil }
    let first = try #require(client.activePane?.id)
    let originalSession = try #require(client.attachedSession)
    let slot = client.slot(for: first)
    try await eventually { slot.frame?.text.contains("zz-native-ready") == true }
    #expect(slot.frame?.text.contains("café 界 👩‍💻") == true)
    #expect(slot.frame?.styles.contains { $0.foreground != slot.frame?.foreground } == true)
    let retained = try #require(slot.frame)
    client.text("native-typed", pane: first)
    client.key(first, code: ZZ_KEY_ENTER.rawValue)
    try await eventually { slot.frame?.text.contains("native-typed") == true }
    client.text("pasted-é-界\n", pane: first, paste: true)
    try await eventually { slot.frame?.text.contains("pasted-é-界") == true }
    client.resize(first, columns: 91, rows: 29, cell: CGSize(width: 16, height: 32))
    try await eventually { slot.frame?.columns == 91 && slot.frame?.rows == 29 }

    client.execute("split-window", ["-h", "-t", "%\(first)", "exec /bin/cat"])
    try await eventually { client.currentWindow?.panes.count == 2 }
    let split = try #require(client.currentWindow)
    #expect(split.panes.allSatisfy { $0.rect.width > 0 && $0.rect.width < 1 })
    client.selectPane(first)
    try await eventually { client.activePane?.id == first }
    let firstWindow = split.id
    client.execute("new-window", ["-n", "second", "printf 'native-second\\r\\n'; exec /bin/cat"])
    try await eventually { client.currentWindow?.name == "second" }
    let second = try #require(client.activePane?.id)
    try await eventually { client.slot(for: second).frame?.text.contains("native-second") == true }
    client.selectWindow(firstWindow)
    try await eventually { client.currentWindow?.id == firstWindow }

    let generation = client.connectionGeneration
    client.disconnect()
    #expect(retained.text.contains("zz-native-ready"))
    client.connect()
    try await eventually { client.connectionGeneration > generation && client.currentWindow?.id == firstWindow }
    try await eventually { client.slot(for: first).frame?.text.contains("native-typed") == true }
    client.selectPane(first)
    try await eventually { client.activePane?.id == first }
    client.execute("resize-pane", ["-Z", "-t", "%\(first)"])
    try await eventually { client.currentWindow?.panes.count == 1 }
    client.resize(first, columns: 88, rows: 26, cell: CGSize(width: 16, height: 32))
    try await eventually { client.slot(for: first).frame?.columns == 88 && client.slot(for: first).frame?.rows == 26 }

    client.execute("new-session", ["-s", "second-session", "exec /bin/cat"])
    try await eventually { client.attachedSession?.name == "second-session" && client.activePane != nil }
    client.focusTerminal(first, focused: false)
    let sessionPane = try #require(client.activePane?.id)
    client.text("new-session-input\n", pane: sessionPane, paste: true)
    try await eventually { client.slot(for: sessionPane).frame?.text.contains("new-session-input") == true }
    #expect(client.commandError == nil)
    client.execute("zz-native-invalid-command")
    try await eventually { client.commandError != nil }
    client.clearError()
    client.attach(originalSession)
    try await eventually { client.attachedSession?.id == originalSession.id && client.activePane?.id == first }

    let reconnectGeneration = client.connectionGeneration
    client.execute("kill-server")
    try await eventually { !client.connected && !process.isRunning }
    try FileManager.default.removeItem(atPath: ready)
    process = try startFixture()
    try await eventually {
        client.connected && client.connectionGeneration > reconnectGeneration && client.activePane != nil
    }
    let restarted = try #require(client.activePane?.id)
    try await eventually { client.slot(for: restarted).frame?.text.contains("zz-native-ready") == true }
    #expect(client.attachedSession?.name == "native-fixture")
}
