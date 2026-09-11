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
    let client = NativeClient(
        endpoint: socket, session: "native-fixture", config: socket + ".config", muxConfig: socket + ".mux")
    defer {
        client.execute("kill-server")
        client.disconnect()
        if process.isRunning { process.terminate() }
        try? FileManager.default.removeItem(atPath: socket)
        try? FileManager.default.removeItem(atPath: ready)
        try? FileManager.default.removeItem(atPath: socket + ".config")
        try? FileManager.default.removeItem(atPath: socket + ".mux")
        try? FileManager.default.removeItem(atPath: socket + ".config.agent-preferences")
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
    client.settings.terminalDraft = "font-size = 19\nwindow-padding-x = 7\nwindow-padding-y = 9\n"
    #expect(client.settings.action("save-terminal", ["source": client.settings.terminalDraft]))
    try await eventually { client.appearance.font_size == 19 && client.appearance.padding == [9, 7, 9, 7] }
    client.settings.terminalDraft = ""
    #expect(client.settings.action("save-terminal", ["source": ""]))
    try await eventually { client.appearance.font_size != 19 }

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

    client.execute("split-browser", ["-h", "-t", "%\(first)", "about:blank"])
    try await eventually { client.activePane?.browser != nil }
    let browserID = try #require(client.activePane?.id)
    let browser = try #require(client.browsers.panes[browserID])
    browser.pending = NativeBrowserDescriptor(tabs: ["about:blank#lost"], active_tab: 0, profile: browser.profile)
    browser.pendingRequest = UInt64.max
    let browserGeneration = client.connectionGeneration
    client.disconnect()
    client.connect()
    try await eventually {
        client.connectionGeneration > browserGeneration && client.activePane?.id == browserID
            && browser.pending == nil && browser.pendingRequest == 0
    }
    let reconnectedBrowser = try #require(client.browsers.panes[browserID])
    client.browsers.newTab(reconnectedBrowser, url: "about:blank#recovered")
    try await eventually {
        client.activePane?.browser?.tabs.last == "about:blank#recovered" && reconnectedBrowser.pending == nil
    }

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

@Test(.enabled(if: ProcessInfo.processInfo.environment["ZZ_NATIVE_TEST_FIXTURE"] != nil))
@MainActor func realAgentTranscriptPermissionsQueuesSettingsAndReplay() async throws {
    let binary = try #require(ProcessInfo.processInfo.environment["ZZ_NATIVE_TEST_FIXTURE"])
    let socket = "/tmp/zza-\(UUID().uuidString.prefix(8)).sock"
    let ready = URL(fileURLWithPath: socket).deletingPathExtension().appendingPathExtension("ready").path
    let process = Process()
    process.executableURL = URL(fileURLWithPath: binary)
    process.arguments = [socket, "--agent"]
    process.standardOutput = FileHandle.nullDevice
    process.standardError = FileHandle.nullDevice
    try process.run()
    let client = NativeClient(
        endpoint: socket, session: "native-fixture", config: socket + ".config", muxConfig: socket + ".mux")
    defer {
        client.execute("kill-server")
        client.disconnect()
        if process.isRunning { process.terminate() }
        try? FileManager.default.removeItem(atPath: socket)
        try? FileManager.default.removeItem(atPath: ready)
        try? FileManager.default.removeItem(atPath: socket + ".config")
        try? FileManager.default.removeItem(atPath: socket + ".mux")
        try? FileManager.default.removeItem(atPath: socket + ".config.agent-preferences")
    }
    try await eventually { FileManager.default.fileExists(atPath: ready) }
    client.connect()
    try await eventually { client.agents.values.contains { $0.snapshot?.phase == "ready" } }
    let agent = try #require(client.agents.values.first)
    try await eventually { agent.snapshot?.agent_name != nil }
    #expect(agent.snapshot?.agent_name == "Native fixture")
    #expect(agent.snapshot?.options.first?.current_value == "small")
    let pasteboard = NSPasteboard(name: NSPasteboard.Name("zz-agent-test-\(UUID().uuidString)"))
    defer { pasteboard.releaseGlobally() }
    agent.draft = "Keep this draft"
    pasteboard.setString("ordinary text", forType: .string)
    #expect(!agent.pasteImages(from: pasteboard))
    pasteboard.clearContents()
    pasteboard.setData(Data("invalid image".utf8), forType: .png)
    #expect(!agent.pasteImages(from: pasteboard))
    #expect(agent.draft == "Keep this draft" && agent.attachments.isEmpty)
    let bitmap = try #require(
        NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: 1, pixelsHigh: 1, bitsPerSample: 8, samplesPerPixel: 4,
            hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 4, bitsPerPixel: 32))
    bitmap.setColor(.red, atX: 0, y: 0)
    let png = try #require(bitmap.representation(using: .png, properties: [:]))
    pasteboard.clearContents()
    pasteboard.setData(png, forType: .png)
    #expect(agent.pasteImages(from: pasteboard))
    #expect(agent.draft == "Keep this draft")
    #expect(agent.attachments == [NativeAgentImage(format: "image/png", data: png)])
    agent.attachments = []
    agent.draft = "/rev"
    try await eventually {
        agent.updateCompletions(); return !agent.completions.isEmpty
    }
    let completion = try #require(agent.completions.first)
    agent.complete(completion)
    #expect(agent.draft == "/review ")
    #expect(agent.commandHint == "Argument · path")
    let commandEnter = try #require(
        NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: .command, timestamp: 0, windowNumber: 0, context: nil,
            characters: "\r", charactersIgnoringModifiers: "\r", isARepeat: false, keyCode: 36))
    agent.draft = "hello 界"
    #expect(agent.canSend)
    #expect(agent.handleKey(commandEnter, editingText: true))
    try await eventually {
        agent.entries.contains { $0.markdown == "Reply: hello 界" } && agent.snapshot?.phase == "ready"
    }
    #expect(agent.draft.isEmpty)
    #expect(agent.entries.contains { $0.type == "user" && $0.markdown == "hello 界" })
    #expect(agent.action("configure", ["option": "model", "value": "large"]))
    try await eventually { agent.snapshot?.options.first?.current_value == "large" && agent.snapshot?.busy == false }
    agent.draft = "permission"
    agent.send()
    try await eventually { agent.snapshot?.permissions.isEmpty == false }
    let permission = try #require(agent.snapshot?.permissions.first)
    #expect(permission.title == "Read fixture file")
    agent.draft = "Follow up with 1 example"
    for (keyCode, characters): (UInt16, String) in [(18, "1"), (36, "\r"), (53, "\u{1b}"), (125, "\u{f701}")] {
        let event = try #require(
            NSEvent.keyEvent(
                with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: 0, context: nil,
                characters: characters, charactersIgnoringModifiers: characters, isARepeat: false, keyCode: keyCode))
        #expect(!agent.handleKey(event, editingText: true))
        #expect(agent.snapshot?.permissions.first?.request_id == permission.request_id)
    }
    agent.draft = ""
    let approve = try #require(
        NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: 0, context: nil,
            characters: "1", charactersIgnoringModifiers: "1", isARepeat: false, keyCode: 18))
    #expect(agent.handleKey(approve, editingText: true))
    try await eventually { agent.snapshot?.phase == "ready" && agent.snapshot?.permissions.isEmpty == true }
    agent.draft = "hang"
    #expect(agent.handleKey(commandEnter, editingText: true))
    try await eventually { agent.snapshot?.phase == "running" }
    agent.draft = "queued text"
    #expect(agent.running && agent.canSend)
    #expect(agent.handleKey(commandEnter, editingText: true))
    try await eventually { agent.snapshot?.queued_prompts == 1 }
    #expect(agent.action("unqueue"))
    try await eventually { agent.draft == "queued text" && agent.snapshot?.queued_prompts == 0 }
    let restoredDraft = agent.draft
    agent.draft = ""
    #expect(agent.running && !agent.canSend)
    #expect(agent.handleKey(commandEnter, editingText: true))
    try await eventually { agent.snapshot?.phase == "ready" }
    agent.draft = restoredDraft
    let messages = agent.entries.compactMap(\.markdown)
    let generation = client.connectionGeneration
    client.disconnect()
    client.connect()
    do {
        try await eventually {
            client.connectionGeneration > generation && agent.entries.compactMap(\.markdown) == messages
                && agent.snapshot?.agent_name != nil && agent.snapshot?.phase == "ready"
        }
    } catch {
        Issue.record(
            "Replay: generation \(client.connectionGeneration), expected > \(generation), messages \(agent.entries.compactMap(\.markdown))/\(messages), phase \(agent.snapshot?.phase ?? "nil"), name \(agent.snapshot?.agent_name ?? "nil"), error \(agent.snapshot?.error ?? "none"), same model \(client.agents[agent.id] === agent)"
        )
        throw error
    }
    #expect(agent.draft == "queued text")
    #expect(agent.entries.filter { $0.markdown == "Reply: hello 界" }.count == 1)
    #expect(agent.action("list-sessions", ["cwd": NSNull(), "next": false]))
    try await eventually {
        agent.snapshot?.history.sessions.contains { $0.title == "Saved fixture" } == true
            && agent.snapshot?.busy == false
    }
    let saved = try #require(agent.snapshot?.history.sessions.first(where: { $0.title == "Saved fixture" }))
    let loaded = agent.action(
        "load-session", ["session_id": saved.sessionId, "cwd": "/tmp", "directories": [String]()])
    try #require(
        loaded,
        "Load error: \(agent.snapshot?.error ?? "none"), phase \(agent.snapshot?.phase ?? "nil"), busy \(agent.snapshot?.busy == true)"
    )
    try await eventually { agent.snapshot?.session_id == saved.sessionId && agent.snapshot?.busy == false }
    #expect(agent.entries.contains { $0.markdown == "Restored conversation" })
    #expect(!agent.entries.contains { $0.markdown == "Reply: hello 界" })
    #expect(agent.action("new-session", ["cwd": "/tmp"]))
    try await eventually { agent.snapshot?.session_id != saved.sessionId && agent.snapshot?.busy == false }
    #expect(agent.entries.isEmpty)
    try await eventually { agent.snapshot?.options.first?.current_value == "large" && agent.snapshot?.busy == false }
    let preferences = try String(contentsOfFile: socket + ".config.agent-preferences/preferences.json", encoding: .utf8)
    #expect(preferences.contains("large"))
}

@Test @MainActor func nativeSettingsPersistenceValidationAndDraftProtection() throws {
    let directory = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent(
        "zz-settings-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }
    let config = directory.appendingPathComponent("config")
    let model = NativeSettings(config: config.path, mux: directory.appendingPathComponent("mux").path)
    #expect(model.error == nil)
    #expect(model.snapshot?.presets.count == 34)
    #expect(model.snapshot?.settings.contains { $0.key == "browser-search-provider" } == true)
    model.set("theme-mode", .string("light"))
    #expect(model.text("theme-mode") == "light")
    #expect(try String(contentsOf: config, encoding: .utf8).contains("theme-mode = light"))
    #expect(!model.action("set", ["key": "pane-margin", "value": -1]))
    #expect(model.error != nil)
    model.terminalDraft = "font-size = 21\n"
    model.set("theme-mode", .string("dark"))
    #expect(model.terminalDraft == "font-size = 21\n")
    #expect(model.terminalDirty)
    #expect(model.action("save-terminal", ["source": model.terminalDraft]))
    #expect(!model.terminalDirty)
    #expect(model.text("theme-mode") == "dark")
    #expect(model.action("add-host", ["name": "builder", "endpoint": "ssh://builder.example"]))
    #expect(model.snapshot?.hosts.first?.name == "builder")
    model.reset("theme-mode")
    #expect(model.text("theme-mode") == "system")
    let reopened = NativeSettings(config: config.path, mux: directory.appendingPathComponent("mux").path)
    #expect(reopened.error == nil)
    #expect(reopened.terminalDraft.contains("font-size = 21"))
    #expect(reopened.snapshot?.hosts.first?.name == "builder")
}
