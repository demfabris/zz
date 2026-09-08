import AppKit
import SwiftUI
import ZZNativeCore
import ZZUI

struct NativeBrowserPaneView: View {
    let client: NativeClient
    let pane: NativePane
    @Bindable var model: NativeBrowserPane
    @Environment(\.zzTheme) private var theme
    @State private var profileVisible = false
    @State private var profile = ""
    @State private var suggestions: [NativeBrowserHistoryEntry] = []
    @State private var selectedSuggestion: String?
    @State private var historyVisible = false
    @State private var historySearch = ""
    @State private var historyEntries: [NativeBrowserHistoryEntry] = []
    @State private var recentEntries: [NativeBrowserHistoryEntry] = []
    @State private var chromeVisible = false
    @State private var sourceProfile = ""
    @State private var importError: String?
    private var engine: NativeBrowserEngine { client.browsers }

    var body: some View {
        VStack(spacing: 0) {
            ZZBrowserToolbar(
                canGoBack: model.activeTab?.canGoBack ?? false,
                canGoForward: model.activeTab?.canGoForward ?? false,
                loading: model.activeTab?.loading ?? false,
                back: { act("back") }, forward: { act("forward") },
                reload: { act(model.activeTab?.loading == true ? "stop" : "reload") }
            ) {
                ZZBrowserTabStrip(
                    tabs: model.tabs.map { ZZBrowserTab($0.id, title: $0.title, detail: $0.url) },
                    selection: Binding(
                        get: { model.selection },
                        set: { id in
                            if let tab = model.tabs.first(where: { $0.id == id }) { engine.select(model, tab: tab) }
                        }), address: $model.address, focusRequest: model.addressFocus, blurRequest: model.addressBlur,
                    focusChanged: { focused in
                        model.addressEditing = focused
                        if focused { refreshSuggestions() } else { suggestions = []; selectedSuggestion = nil }
                    }, addressChanged: { _ in refreshSuggestions() }, moveSelection: moveSuggestion,
                    removeSelection: removeSelectedSuggestion, cancel: cancelAddress,
                    submit: { submitSuggestion() }, close: { engine.closeTab(model, id: $0) },
                    newTab: { engine.newTab(model) })
            } actions: {
                ZZBrowserActionMenu {
                    Button("New tab") { engine.newTab(model) }
                    Button("Reload") { act("reload") }
                    Button("History…") {
                        historySearch = ""; refreshHistory(); historyVisible = true
                    }
                    Button("Import from Chrome…") {
                        importError = nil; chromeVisible = true; engine.discoverChromeProfiles()
                    }
                    Divider()
                    Button("Zoom in") { act("zoom-in") }
                    Button("Zoom out") { act("zoom-out") }
                    Button("Actual size") { act("zoom-reset") }
                    Divider()
                    Button("Select element") { pick() }
                    Button("Developer tools") { act("devtools") }
                    Button("Clear site data") { act("clear-site-data") }
                    Button("Browser profile…") {
                        profile = model.profile; profileVisible = true
                    }
                }
            }
            if let tab = model.activeTab {
                ZStack {
                    NativeBrowserSurface(client: client, pane: pane.id, tab: tab, active: pane.active, frame: tab.frame)
                    if let error = engine.error ?? tab.error {
                        ZZBrowserError(error) {
                            tab.error = nil; act("reload")
                        }
                    } else if tab.url == "about:blank" {
                        ZZBrowserStart {
                            ForEach(recentEntries) { entry in
                                ZZBrowserRecentRow(entry.display_url) { openHistory(entry) }
                            }
                        }
                    }
                }
                .overlay(alignment: .bottom) {
                    if tab.picking {
                        ZZBrowserPickStatus("Click an element to copy its context") {
                            tab.picking = false; act("cancel-pick")
                        }
                        .padding(12)
                    }
                }
            }
        }
        .overlay(alignment: .top) {
            if model.addressEditing && !suggestions.isEmpty {
                ZZBrowserOmnibox {
                    ForEach(suggestions) { entry in
                        HStack(spacing: 0) {
                            ZZBrowserOmniboxRow(
                                entry.title, url: entry.display_url, selected: selectedSuggestion == entry.id
                            ) {
                                submitSuggestion(entry)
                            }
                            ZZIconButton("Remove from history", systemName: "xmark") { removeHistory(entry) }
                                .help("Remove from history (Shift-Delete)").padding(.trailing, 4)
                        }
                    }
                }.padding(.top, 38).padding(.horizontal, 80)
            }
        }
        .onAppear {
            engine.setVisible(pane.id, true); refreshHistory()
        }
        .onDisappear { engine.setVisible(pane.id, false) }
        .onChange(of: model.pickRequest) { pick() }
        .onChange(of: model.selection) {
            suggestions = []; selectedSuggestion = nil
        }
        .onChange(of: model.profile) {
            suggestions = []; selectedSuggestion = nil; refreshHistory()
        }
        .onChange(of: engine.historyRevision) {
            refreshHistory(); if model.addressEditing { refreshSuggestions() }
        }
        .onChange(of: historySearch) { refreshHistory() }
        .onChange(of: engine.chromeProfilesLoading) {
            if !engine.chromeProfiles.contains(where: { $0.id == sourceProfile }) {
                sourceProfile = engine.chromeProfiles.first?.id ?? ""
            }
        }
        .sheet(isPresented: $historyVisible) { history }
        .sheet(isPresented: $chromeVisible) { chromeImport }
        .sheet(isPresented: $profileVisible) {
            ZZDialog("Browser profile", description: "Each profile keeps its own cookies and site data.") {
                ZZTextField("Profile", text: $profile)
            } actions: {
                ZZButton("Cancel") { profileVisible = false }
                ZZButton("Switch", variant: .primary) {
                    client.execute("set-browser-profile", ["-t", "%\(pane.id)", profile])
                    profileVisible = false
                }.disabled(profile.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
    }

    private var history: some View {
        ZZDialog("History", description: "Pages saved in the \(model.profile) browser profile.") {
            ZZTextField("Search history", text: $historySearch)
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(historyEntries) { entry in
                        HStack(spacing: 0) {
                            ZZBrowserOmniboxRow(entry.title, url: entry.display_url) {
                                historyVisible = false
                                openHistory(entry)
                            }
                            ZZIconButton("Remove from history", systemName: "xmark") { removeHistory(entry) }
                        }
                    }
                    if historyEntries.isEmpty {
                        Text(historySearch.isEmpty ? "No pages saved yet." : "No matching pages.")
                            .font(theme.font(size: 12)).foregroundStyle(theme.foreground.muted().color).padding(16)
                    }
                }
            }.frame(height: 320)
        } actions: {
            ZZButton("Done") { historyVisible = false }
        }
    }

    private var importing: Bool {
        guard let request = model.chromeImportRequest else { return false }
        return engine.pendingImports[request] == model.profile
    }

    private var importResult: NativeChromeImportResult? {
        guard let request = model.chromeImportRequest, let result = engine.chromeImports[request],
            result.profile == model.profile
        else { return nil }
        return result
    }

    private var chromeImport: some View {
        ZZDialog(
            "Import from Chrome", description: "Copy cookies and history into the \(model.profile) browser profile."
        ) {
            if engine.chromeProfilesLoading {
                HStack {
                    ProgressView().controlSize(.small); Text("Reading Chrome profiles…")
                }
            } else if engine.chromeProfiles.isEmpty {
                Text(engine.chromeProfilesError ?? "No Chrome profiles found.")
                    .font(theme.font(size: 12)).foregroundStyle(theme.foreground.muted().color)
            } else {
                Picker("Chrome profile", selection: $sourceProfile) {
                    ForEach(engine.chromeProfiles) { source in Text(source.label).tag(source.id) }
                }
            }
            if importing {
                HStack {
                    ProgressView().controlSize(.small); Text("Importing into \(model.profile)…")
                }
            }
            if let result = importResult {
                Text(
                    "Imported \(result.history_imported) history entries and \(result.cookies_imported) cookies into \(result.profile)."
                )
                .font(theme.font(size: 12)).fixedSize(horizontal: false, vertical: true)
                if result.history_skipped + result.cookies_skipped + result.cookies_rejected > 0 {
                    Text(
                        "Skipped \(result.history_skipped) history entries and \(result.cookies_skipped + result.cookies_rejected) cookies."
                    )
                    .font(theme.font(size: 11)).foregroundStyle(theme.foreground.muted().color)
                }
                if !result.errors.isEmpty {
                    Text(result.errors.joined(separator: "\n")).font(theme.font(size: 12))
                        .foregroundStyle(theme.warning.color).textSelection(.enabled)
                }
                if result.permission_denied {
                    ZZButton("Open Full Disk Access") {
                        if let url = URL(
                            string: "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
                        {
                            NSWorkspace.shared.open(url)
                        }
                    }
                }
            }
            if let importError { Text(importError).font(theme.font(size: 12)).foregroundStyle(theme.warning.color) }
        } actions: {
            ZZButton("Refresh") { engine.discoverChromeProfiles() }.disabled(engine.chromeProfilesLoading)
            ZZButton("Done") { chromeVisible = false }
            ZZButton("Import", variant: .primary) {
                guard let source = engine.chromeProfiles.first(where: { $0.id == sourceProfile }) else { return }
                importError =
                    engine.importChrome(model, source: source)
                    ? nil : "The import could not start. Try again when the page is ready."
            }.disabled(
                importing || engine.chromeProfilesLoading || sourceProfile.isEmpty
                    || (model.activeTab?.session ?? 0) == 0)
        }
    }

    private func refreshHistory() {
        recentEntries = engine.history(model, limit: 6)
        historyEntries = engine.history(model, input: historySearch.isEmpty ? nil : historySearch)
    }

    private func refreshSuggestions() {
        suggestions = engine.history(model, input: model.address, limit: 8)
        selectedSuggestion = nil
    }

    private func moveSuggestion(_ direction: Int) -> Bool {
        guard !suggestions.isEmpty else { return false }
        let current = selectedSuggestion.flatMap { id in suggestions.firstIndex(where: { $0.id == id }) }
        let next =
            current.map { ($0 + direction + suggestions.count) % suggestions.count }
            ?? (direction > 0 ? 0 : suggestions.count - 1)
        selectedSuggestion = suggestions[next].id
        return true
    }

    private func removeSelectedSuggestion() -> Bool {
        guard let entry = suggestions.first(where: { $0.id == selectedSuggestion }) else { return false }
        removeHistory(entry)
        return true
    }

    private func removeHistory(_ entry: NativeBrowserHistoryEntry) {
        engine.removeHistory(model, entry: entry)
        refreshHistory()
        refreshSuggestions()
    }

    private func submitSuggestion(_ entry: NativeBrowserHistoryEntry? = nil) {
        let entry = entry ?? suggestions.first(where: { $0.id == selectedSuggestion })
        engine.navigate(model, suggestion: entry)
        suggestions = []
        selectedSuggestion = nil
    }

    private func openHistory(_ entry: NativeBrowserHistoryEntry) {
        model.address = entry.url
        submitSuggestion(entry)
    }

    private func cancelAddress() {
        suggestions = []
        selectedSuggestion = nil
        model.address = model.activeTab?.url == "about:blank" ? "" : model.activeTab?.url ?? ""
        model.addressEditing = false
    }

    private func act(_ action: String) {
        if let tab = model.activeTab { engine.action(tab, action) }
    }

    private func pick() {
        guard let tab = model.activeTab else { return }
        tab.picking = true
        engine.action(
            tab, "pick",
            [
                "appearance": [
                    "outline": css(theme.foreground), "fill": css(theme.foreground.opacity(0.15)),
                    "contrast": css(theme.background), "background": css(theme.background),
                    "foreground": css(theme.foreground), "border": css(theme.border),
                    "radius": theme.radius, "font": theme.fontFamily ?? "-apple-system",
                ]
            ])
    }

    private func css(_ color: ZZColor) -> String {
        "rgba(\(Int(color.red * 255)),\(Int(color.green * 255)),\(Int(color.blue * 255)),\(color.alpha))"
    }
}
