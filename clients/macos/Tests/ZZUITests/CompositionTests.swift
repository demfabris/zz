import AppKit
import SwiftUI
import Testing

@testable import ZZUI

@MainActor
struct CompositionTests {
    @Test func sidebarUsesItsDefaultWidthAndReplacesTheStatusBar() throws {
        _ = NSApplication.shared
        var visible = true
        func root() -> some View {
            ZZThemeContainer {
                ZZAppShell(sidebarVisible: Binding(get: { visible }, set: { visible = $0 })) {
                    ZZWorkspaceSidebar {
                        EmptyView()
                    } content: {
                        EmptyView()
                    }
                    .background(LayoutProbe("sidebar"))
                } content: {
                    LayoutProbe("content")
                } status: {
                    LayoutProbe("status").frame(height: 35)
                }
                .frame(width: 1280, height: 840)
            }
        }
        let hosting = NSHostingView(rootView: root())
        hosting.frame = NSRect(x: 0, y: 0, width: 1280, height: 840)
        hosting.layoutSubtreeIfNeeded()
        #expect(try #require(probe("sidebar", in: hosting)).frame.width == 256)
        #expect(probe("status", in: hosting) == nil)
        #expect(try #require(probe("content", in: hosting)).frame.width == 1018)

        visible = false
        hosting.rootView = root()
        hosting.layoutSubtreeIfNeeded()
        #expect(probe("sidebar", in: hosting) == nil)
        #expect(try #require(probe("status", in: hosting)).frame.height == 35)
        #expect(try #require(probe("content", in: hosting)).frame.width == 1280)
    }

    @Test func sidebarSplitRespectsItsWidthLimits() throws {
        for (fraction, expectedWidth) in [(CGFloat(0), CGFloat(160)), (CGFloat(1), CGFloat(640))] {
            let hosting = NSHostingView(
                rootView: ZZWorkspaceSplit(
                    axis: .horizontal, fraction: .constant(fraction), fractionRange: (160.0 / 1274)...(640.0 / 1274)
                ) {
                    LayoutProbe("sidebar")
                } second: {
                    Color.clear
                }
                .frame(width: 1280, height: 840))
            hosting.frame = NSRect(x: 0, y: 0, width: 1280, height: 840)
            hosting.layoutSubtreeIfNeeded()
            #expect(abs(try #require(probe("sidebar", in: hosting)).frame.width - expectedWidth) < 1)
        }
    }

    private func probe(_ name: String, in view: NSView) -> NSView? {
        if view.identifier?.rawValue == name { return view }
        return view.subviews.lazy.compactMap { probe(name, in: $0) }.first
    }

    @Test func providerSuggestionsCapAtSixRows() {
        for count in [0, 3, 100] {
            let size = measured(width: 600, theme: .dark) {
                ZZAgentSuggestionList(rowCount: count) {
                    ForEach(0..<count, id: \.self) { index in
                        ZZAgentSuggestionRow("command\(index)", description: "Provider supplied description") {}
                    }
                }
            }
            #expect(abs(size.height - CGFloat(8 + min(count, 6) * 52)) < 1)
            #expect(abs(size.width - 600) < 1)
        }
    }

    @Test func workspaceRowsKeepTheirDensityWithLongLabelsAndIndentation() {
        for theme in [ZZTheme.light, .dark] {
            let size = measured(width: 256, theme: theme) {
                VStack(spacing: 0) {
                    ZZWorkspaceTreeRow("This Mac", icon: "laptopcomputer", expanded: .constant(true)) {}
                    ZZWorkspaceTreeRow("A session with a long label", icon: "square.stack", depth: 1) {}
                    ZZWorkspaceTreeRow(
                        "A deeply nested terminal pane with a long title", icon: "terminal", depth: 2, selected: true
                    ) {}
                }
            }
            #expect(abs(size.height - 96) < 1)
            #expect(abs(size.width - 256) < 1)
        }
    }

    @Test func chooserCapsVisibleRowsWithoutGrowingToItsFullCollection() {
        for count in [0, 3, 100] {
            let size = measured(width: 600, theme: .dark) {
                ZZChooserModal("Choose a window", subtitle: "\(count) windows", rowCount: count, close: {}) {
                    ForEach(0..<count, id: \.self) { index in
                        ZZTreeChooserRow("Window \(index)", target: "@\(index)") {}
                    }
                } footer: {
                    ZZChooserFooter()
                }
            }
            #expect(abs(size.height - CGFloat(98 + min(count, 10) * 40)) < 1)
            #expect(abs(size.width - 600) < 1)
        }
    }

    @Test func emptyComposerRetainsItsSourceHeightWithNativeEditing() {
        let size = measured(width: 720, theme: .light) {
            ZZAgentComposer(text: .constant(""), canSend: false, send: {}, stop: {}) {
                Text("Default model").font(.system(size: 11))
            } attachments: {
                EmptyView()
            } footer: {
                Text("~/dev/zz").font(.system(size: 11))
            }
        }
        #expect(abs(size.height - 146) < 1)
        #expect(abs(size.width - 720) < 1)
    }

    @Test func paneCornersPreserveFlushEdgesAndInsetBorders() {
        let shape = ZZPaneShape(.init(topLeft: 20, topRight: 0, bottomLeft: 0, bottomRight: 20))
        let bounds = CGRect(x: 0, y: 0, width: 100, height: 100)
        let path = shape.path(in: bounds)
        #expect(!path.contains(CGPoint(x: 1, y: 1)))
        #expect(path.contains(CGPoint(x: 99, y: 1)))
        #expect(path.contains(CGPoint(x: 1, y: 99)))
        #expect(!path.contains(CGPoint(x: 99, y: 99)))
        #expect(shape.inset(by: 2).path(in: bounds).boundingRect == bounds.insetBy(dx: 2, dy: 2))
    }

    @Test func longDiffLinesKeepTheirIntrinsicWidthInsideNativeScrolling() throws {
        _ = NSApplication.shared
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 600, height: 200),
            styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.close() }
        let hosting = NSHostingView(
            rootView: ZZThemeContainer {
                ZZAgentDiff(
                    "long.swift",
                    lines: [
                        .init("long", text: String(repeating: "let value = 42; ", count: 30), newLine: 1, kind: .added)
                    ]
                ) {}.frame(width: 600)
            })
        window.contentView = hosting
        hosting.layoutSubtreeIfNeeded()
        func scrollViews(in view: NSView) -> [NSScrollView] {
            (view as? NSScrollView).map { [$0] } ?? view.subviews.flatMap { scrollViews(in: $0) }
        }
        let scroll = try #require(scrollViews(in: hosting).first)
        let document = try #require(scroll.documentView)
        #expect(document.frame.width > scroll.contentView.bounds.width + 500)
    }

    @Test func nativeTimelineScrollHandlesWheelAndRestickWithoutTreatingLayoutAsUserInput() {
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        let document = FlippedDocument(frame: NSRect(x: 0, y: 0, width: 400, height: 1200))
        scroll.documentView = document
        let observer = ZZAgentScrollObserverView()
        var distances: [CGFloat] = []
        observer.onUserScroll = { distances.append($0) }
        document.addSubview(observer)

        scroll.contentView.scroll(to: NSPoint(x: 0, y: 500))
        scroll.reflectScrolledClipView(scroll.contentView)
        #expect(distances.isEmpty)
        NotificationCenter.default.post(name: NSScrollView.didLiveScrollNotification, object: scroll)
        #expect(distances.last == 400)

        scroll.contentView.scroll(to: NSPoint(x: 0, y: 850))
        NotificationCenter.default.post(name: NSScrollView.didLiveScrollNotification, object: scroll)
        #expect(distances.last == 50)

        let otherScroll = NSScrollView()
        NotificationCenter.default.post(name: NSScrollView.didLiveScrollNotification, object: otherScroll)
        #expect(distances.count == 2)
        observer.removeFromSuperview()
        NotificationCenter.default.post(name: NSScrollView.didLiveScrollNotification, object: scroll)
        #expect(distances.count == 2)
    }

    @Test func nativeTimelineBottomDistanceSupportsUnflippedDocumentsAndShortContent() {
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        scroll.documentView = NSView(frame: NSRect(x: 0, y: 0, width: 400, height: 1200))
        scroll.contentView.scroll(to: NSPoint(x: 0, y: 230))
        #expect(ZZAgentScrollObserverView.distanceToBottom(in: scroll) == 230)
        scroll.contentView.scroll(to: .zero)
        #expect(ZZAgentScrollObserverView.distanceToBottom(in: scroll) == 0)
        scroll.documentView = FlippedDocument(frame: NSRect(x: 0, y: 0, width: 400, height: 100))
        #expect(ZZAgentScrollObserverView.distanceToBottom(in: scroll) == 0)
    }

    private func measured<Content: View>(
        width: CGFloat, theme: ZZTheme,
        @ViewBuilder content: () -> Content
    ) -> CGSize {
        _ = NSApplication.shared
        let view = NSHostingView(
            rootView: ZZThemeContainer(theme: theme) {
                content().frame(width: width).fixedSize(horizontal: false, vertical: true)
            })
        view.layoutSubtreeIfNeeded()
        return view.fittingSize
    }
}

@MainActor private final class FlippedDocument: NSView {
    override var isFlipped: Bool { true }
}

private struct LayoutProbe: NSViewRepresentable {
    let name: String
    init(_ name: String) { self.name = name }
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        view.identifier = NSUserInterfaceItemIdentifier(name)
        return view
    }
    func updateNSView(_ view: NSView, context: Context) {}
}
