import SwiftUI
import ZZUI

struct BrowserCompositionGallery: View {
    @Environment(\.zzTheme) private var theme
    @State private var address = "https://zzmux.sh"
    @State private var selected = "zz"
    @State private var tabs = [
        ZZBrowserTab("zz", title: "zzmux.sh", detail: "https://zzmux.sh"),
        ZZBrowserTab("github", title: "github.com", detail: "https://github.com/demfabris/zz"),
    ]
    @State private var loading = false
    @State private var suggestions = false
    @State private var picking = false
    @State private var failed = false
    @State private var profile = "Default"
    @State private var zoom = 100
    @State private var opened = ""
    @State private var nextID = 3

    var body: some View {
        VStack(spacing: 20) {
            GallerySection(
                "Browser pane",
                detail:
                    "The compact active tab doubles as the address field. Menus and keyboard editing use native macOS controls."
            ) {
                ZZPane(active: true, gap: 0) {
                    VStack(spacing: 0) {
                        toolbar
                        ZStack(alignment: .top) {
                            if failed {
                                ZZBrowserError("The server stopped responding. Check the address and try again.") {
                                    failed = false
                                    opened = address
                                }
                            } else {
                                ZZBrowserStart(showsHint: false) {
                                    VStack(spacing: 4) {
                                        ZZBrowserRecentRow("zzmux.sh") { navigate("https://zzmux.sh") }
                                        ZZBrowserRecentRow("github.com/demfabris/zz") {
                                            navigate("https://github.com/demfabris/zz")
                                        }
                                        if !opened.isEmpty {
                                            Text("Opened in fixture: \(opened)").font(.system(size: 10))
                                                .foregroundStyle(theme.foreground.muted().color).lineLimit(1)
                                        }
                                    }
                                }
                            }
                            if suggestions {
                                ZZBrowserOmnibox {
                                    ZZBrowserOmniboxRow(
                                        "zz · Terminal workspace", url: "https://zzmux.sh", selected: true
                                    ) { navigate("https://zzmux.sh") }
                                    ZZBrowserOmniboxRow("zz on GitHub", url: "https://github.com/demfabris/zz") {
                                        navigate("https://github.com/demfabris/zz")
                                    }
                                }.padding(8)
                            }
                            if picking {
                                ZZBrowserPickStatus("Choose an element to send to your agent") { picking = false }
                                    .padding(.top, 8)
                            }
                        }.frame(maxHeight: .infinity)
                    }
                }.frame(height: 380)
                HStack(spacing: 8) {
                    ZZButton("Suggestions", selected: suggestions) { suggestions.toggle() }
                    ZZButton("Error state", selected: failed) { failed.toggle() }
                    ZZButton("Element picker", selected: picking) { picking.toggle() }
                    Spacer()
                    ZZTag("\(profile) · \(zoom)%")
                }
            }
            GallerySection("Address and empty state") {
                ZZBrowserAddress($address) { navigate(address) }
                ZZBrowserStart().frame(height: 140)
            }
        }
        .onChange(of: selected) {
            if let tab = tabs.first(where: { $0.id == selected }) { address = tab.detail }
        }
    }

    private var toolbar: some View {
        ZZBrowserToolbar(
            canGoBack: !opened.isEmpty, canGoForward: false, loading: loading,
            back: { opened = "" }, forward: {}, reload: { loading.toggle() }
        ) {
            ZZBrowserTabStrip(
                tabs: tabs, selection: $selected, address: $address,
                submit: { navigate(address) }, close: closeTab
            ) {
                let id = "new-\(nextID)"
                nextID += 1
                tabs.append(.init(id, title: "New tab"))
                selected = id
                address = ""
                opened = ""
            }
        } actions: {
            ZZIconButton("Pick element", systemName: "cursorarrow.rays") { picking.toggle() }
            ZZBrowserActionMenu {
                Button("Copy address", systemImage: "doc.on.doc") { opened = "Address copied in fixture" }
                Menu("Profile") {
                    Button("Default") { profile = "Default" }
                    Button("Work") { profile = "Work" }
                }
                Divider()
                Button("Zoom in") { zoom = min(300, zoom + 10) }
                Button("Zoom out") { zoom = max(50, zoom - 10) }
                Button("Actual size") { zoom = 100 }
                Divider()
                Button("Reload") { loading.toggle() }
                Button("Inspect element") { picking = true }
                Button("Show error state") { failed = true }
            }
        }
    }

    private func navigate(_ url: String) {
        address = url
        opened = url
        suggestions = false
        failed = false
        if let index = tabs.firstIndex(where: { $0.id == selected }) {
            tabs[index].detail = url
            tabs[index].title = URL(string: url)?.host ?? url
        }
    }

    private func closeTab(_ id: String) {
        guard tabs.count > 1 else { return }
        tabs.removeAll { $0.id == id }
        if selected == id { selected = tabs.first?.id ?? "" }
    }
}
