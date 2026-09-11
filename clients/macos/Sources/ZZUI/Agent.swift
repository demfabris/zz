import AppKit
import SwiftUI

public enum ZZAgentMetrics {
    public static let contentMaxWidth: CGFloat = 680
    public static let headerHeight: CGFloat = 40
    public static let activityHeight: CGFloat = 28
    public static let composerMinHeight: CGFloat = 86
    public static let composerFooterHeight: CGFloat = 28
    public static let composerOuterPadding: CGFloat = 12
    public static let composerSectionGap: CGFloat = 8
    public static let composerMaxWidth: CGFloat = 682
    public static let transcriptAttachment: CGFloat = 140
    public static let composerAttachment: CGFloat = 56
}

public struct ZZAgentHeader<Leading: View, Trailing: View>: View {
    @Environment(\.zzTheme) private var theme
    private let leading: Leading
    private let trailing: Trailing

    public init(@ViewBuilder leading: () -> Leading, @ViewBuilder trailing: () -> Trailing) {
        self.leading = leading()
        self.trailing = trailing()
    }

    public var body: some View {
        HStack(spacing: 6) {
            leading
            Spacer(minLength: 0)
            trailing
        }
        .padding(.horizontal, 6).frame(height: ZZAgentMetrics.headerHeight)
        .overlay(alignment: .bottom) { theme.border.color.frame(height: 1) }
    }
}

public struct ZZAgentTimeline<Content: View>: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Binding private var followsTail: Bool
    @State private var lastScrollDistance: CGFloat = 0
    private let revision: Int
    private let content: Content

    public init(revision: Int = 0, followsTail: Binding<Bool>, @ViewBuilder content: () -> Content) {
        self.revision = revision
        _followsTail = followsTail
        self.content = content()
    }

    public var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    content
                    Color.clear.frame(height: 1).id("zz-agent-tail")
                }
                .padding(.top, 16).padding(.horizontal, 16).padding(.bottom, 12)
                .frame(maxWidth: ZZAgentMetrics.contentMaxWidth).frame(maxWidth: .infinity)
                .background {
                    ZZAgentScrollObserver { distance in
                        let previous = lastScrollDistance
                        lastScrollDistance = distance
                        if distance > previous + 1 && distance > 2 {
                            followsTail = false
                        } else if !followsTail && (distance <= 2 || distance <= 70 && distance < previous) {
                            followsTail = true
                            lastScrollDistance = 0
                            proxy.scrollTo("zz-agent-tail", anchor: .bottom)
                        }
                    }
                }
            }
            .defaultScrollAnchor(.bottom)
            .onChange(of: revision) {
                if followsTail {
                    lastScrollDistance = 0
                    proxy.scrollTo("zz-agent-tail", anchor: .bottom)
                }
            }
            .overlay(alignment: .bottom) {
                if !followsTail {
                    ZZButton("Jump to latest", icon: "arrow.down", size: .small) {
                        withAnimation(reduceMotion ? nil : .easeOut(duration: 0.2)) {
                            followsTail = true
                            lastScrollDistance = 0
                            proxy.scrollTo("zz-agent-tail", anchor: .bottom)
                        }
                    }.padding(.bottom, 8)
                }
            }
        }
    }
}

private struct ZZAgentScrollObserver: NSViewRepresentable {
    let onUserScroll: (CGFloat) -> Void

    func makeNSView(context: Context) -> ZZAgentScrollObserverView {
        let view = ZZAgentScrollObserverView()
        view.onUserScroll = onUserScroll
        return view
    }

    func updateNSView(_ view: ZZAgentScrollObserverView, context: Context) {
        view.onUserScroll = onUserScroll
        view.observeEnclosingScrollView()
    }
}

final class ZZAgentScrollObserverView: NSView {
    var onUserScroll: (CGFloat) -> Void = { _ in }
    private weak var observedScrollView: NSScrollView?

    override func viewDidMoveToSuperview() {
        super.viewDidMoveToSuperview()
        observeEnclosingScrollView()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        observeEnclosingScrollView()
    }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    func observeEnclosingScrollView() {
        let scrollView = enclosingScrollView
        guard scrollView !== observedScrollView else { return }
        NotificationCenter.default.removeObserver(
            self, name: NSScrollView.didLiveScrollNotification, object: observedScrollView)
        observedScrollView = scrollView
        if let scrollView {
            NotificationCenter.default.addObserver(
                self, selector: #selector(didScroll(_:)),
                name: NSScrollView.didLiveScrollNotification, object: scrollView)
        }
    }

    static func distanceToBottom(in scrollView: NSScrollView) -> CGFloat {
        guard let document = scrollView.documentView else { return 0 }
        let visible = scrollView.documentVisibleRect
        return max(0, document.isFlipped ? document.bounds.maxY - visible.maxY : visible.minY - document.bounds.minY)
    }

    @objc private func didScroll(_ notification: Notification) {
        guard let scrollView = observedScrollView else { return }
        onUserScroll(Self.distanceToBottom(in: scrollView))
    }
}

public struct ZZAgentUserMessage<Attachments: View>: View {
    @Environment(\.zzTheme) private var theme
    private let markdown: String
    private let attachments: Attachments

    public init(_ markdown: String, @ViewBuilder attachments: () -> Attachments) {
        self.markdown = markdown
        self.attachments = attachments()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            attachments
            if !markdown.isEmpty { ZZMarkdown(markdown) }
        }
        .padding(.horizontal, 12).padding(.vertical, 8)
        .background(theme.background.raised(1).color, in: ZZRoundedRectangle(radius: theme.radius))
        .overlay { ZZRoundedRectangle(radius: theme.radius).stroke(theme.border.color, lineWidth: 1) }
        .frame(maxWidth: .infinity, alignment: .trailing)
    }
}

extension ZZAgentUserMessage where Attachments == EmptyView {
    public init(_ markdown: String) { self.init(markdown) { EmptyView() } }
}

public struct ZZAgentAssistantMessage: View {
    private let markdown: String
    private let streaming: Bool
    private let copy: (() -> Void)?

    public init(_ markdown: String, streaming: Bool = false, copy: (() -> Void)? = nil) {
        self.markdown = markdown
        self.streaming = streaming
        self.copy = copy
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            ZZMarkdown(markdown)
            if streaming { ZZSpinner(size: 10).accessibilityLabel("Agent is responding") }
            if let copy {
                ZZIconButton("Copy message", systemName: "doc.on.doc", action: copy).frame(height: 28)
            }
        }.frame(maxWidth: .infinity, alignment: .leading)
    }
}

public struct ZZAgentActivity<Content: View>: View {
    @Environment(\.zzTheme) private var theme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var hovered = false
    @Binding private var expanded: Bool
    private let title: String
    private let icon: String
    private let running: Bool
    private let content: Content

    public init(
        _ title: String, icon: String = "terminal", expanded: Binding<Bool>, running: Bool = false,
        @ViewBuilder content: () -> Content
    ) {
        self.title = title
        self.icon = icon
        _expanded = expanded
        self.running = running
        self.content = content()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Button {
                withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.16)) { expanded.toggle() }
            } label: {
                HStack(spacing: 8) {
                    if running {
                        ZZSpinner(size: 13)
                    } else {
                        Image(systemName: icon).font(.system(size: 13)).offset(y: 0.5)
                    }
                    Text(title.replacingOccurrences(of: "\n", with: " · ")).font(theme.font(size: 13)).lineLimit(1)
                    Image(systemName: expanded ? "chevron.up" : "chevron.right").font(.system(size: 12)).offset(y: 0.5)
                    Spacer(minLength: 0)
                }.frame(height: 28).contentShape(Rectangle())
            }
            .buttonStyle(.plain).foregroundStyle((hovered ? theme.foreground : theme.foreground.muted()).color)
            .onHover { hovered = $0 }.accessibilityValue(expanded ? "Expanded" : "Collapsed")
            if expanded {
                content.padding(.leading, 16).padding(.vertical, 4)
                    .overlay(alignment: .leading) { theme.border.color.frame(width: 1) }
                    .padding(.leading, 4)
            }
        }
    }
}

public struct ZZAgentPlan<Content: View>: View {
    @Environment(\.zzTheme) private var theme
    private let content: Content
    public init(@ViewBuilder content: () -> Content) { self.content = content() }
    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label("Plan", systemImage: "checkmark.circle").font(theme.font(size: 12, weight: .medium))
                .foregroundStyle(theme.foreground.muted().color)
            content
        }
        .padding(.horizontal, 12).padding(.vertical, 8).frame(maxWidth: .infinity, alignment: .leading)
        .overlay { ZZRoundedRectangle(radius: theme.radius).stroke(theme.border.color, lineWidth: 1) }
    }
}

public struct ZZAgentToolOutput: View {
    @Environment(\.zzTheme) private var theme
    private let output: String
    private let title: String?

    public init(_ output: String, title: String? = nil) {
        self.output = output
        self.title = title
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let title {
                Text(title).font(theme.font(size: 11, monospaced: true)).foregroundStyle(theme.foreground.muted().color)
                    .padding(8).frame(maxWidth: .infinity, alignment: .leading)
                ZZSeparator()
            }
            ScrollView([.horizontal, .vertical]) {
                Text(output).font(theme.font(size: 11, monospaced: true)).lineSpacing(5)
                    .textSelection(.enabled).padding(8).frame(maxWidth: .infinity, alignment: .leading)
            }.frame(maxHeight: 360)
        }.zzSurface()
    }
}

public struct ZZAgentPermission<Actions: View>: View {
    @Environment(\.zzTheme) private var theme
    private let title: String
    private let detail: String
    private let actions: Actions

    public init(_ title: String, detail: String, @ViewBuilder actions: () -> Actions) {
        self.title = title
        self.detail = detail
        self.actions = actions()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Label(title, systemImage: "hand.raised").font(theme.font(size: 13, weight: .medium))
                .foregroundStyle(theme.warning.color)
            Text(detail).font(theme.font(size: 12)).textSelection(.enabled)
            HStack(spacing: 8) {
                Spacer()
                actions
            }
        }.padding(12)
            .background(theme.warning.fill().color, in: ZZRoundedRectangle(radius: theme.radius))
            .overlay {
                ZZRoundedRectangle(radius: theme.radius).strokeBorder(theme.warning.outline().color, lineWidth: 1)
            }
    }
}

public enum ZZAgentDiffKind: Sendable {
    case context, added, removed
}

public struct ZZAgentDiffLine: Identifiable, Sendable {
    public let id: String
    public let text: String
    public let oldLine: Int?
    public let newLine: Int?
    public let kind: ZZAgentDiffKind

    public init(_ id: String, text: String, oldLine: Int? = nil, newLine: Int? = nil, kind: ZZAgentDiffKind = .context)
    {
        self.id = id
        self.text = text
        self.oldLine = oldLine
        self.newLine = newLine
        self.kind = kind
    }
}

public struct ZZAgentDiff: View {
    @Environment(\.zzTheme) private var theme
    @State private var viewportWidth: CGFloat = 0
    private let path: String
    private let lines: [ZZAgentDiffLine]
    private let open: () -> Void

    public init(_ path: String, lines: [ZZAgentDiffLine], open: @escaping () -> Void) {
        self.path = path
        self.lines = lines
        self.open = open
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button(action: open) {
                Label(path, systemImage: "doc.text").font(theme.font(size: 11, monospaced: true))
                    .padding(8).frame(maxWidth: .infinity, alignment: .leading)
            }.buttonStyle(.plain)
            ZZSeparator()
            ScrollView([.horizontal, .vertical]) {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(lines) { line in
                        HStack(spacing: 8) {
                            Text(line.oldLine.map(String.init) ?? "").frame(width: 28, alignment: .trailing)
                            Text(line.newLine.map(String.init) ?? "").frame(width: 28, alignment: .trailing)
                            Text(line.kind == .added ? "+" : line.kind == .removed ? "−" : " ")
                            Text(line.text).textSelection(.enabled).fixedSize(horizontal: true, vertical: false)
                            Spacer(minLength: 0)
                        }
                        .font(theme.font(size: 11, monospaced: true)).padding(.horizontal, 8).frame(height: 20)
                        .foregroundStyle(
                            (line.kind == .added
                                ? theme.success : line.kind == .removed ? theme.danger : theme.foreground).color
                        )
                        .background(
                            line.kind == .added
                                ? theme.success.fill().color
                                : line.kind == .removed ? theme.danger.fill().color : .clear)
                    }
                }.padding(.vertical, 4).frame(minWidth: viewportWidth, alignment: .leading)
            }
            .frame(height: min(360, CGFloat(lines.count) * 20 + 8))
            .onGeometryChange(for: CGFloat.self) {
                $0.size.width
            } action: {
                viewportWidth = $0
            }
        }.zzSurface()
    }
}

public struct ZZAgentComposer<Settings: View, Attachments: View, Footer: View>: View {
    @Environment(\.zzTheme) private var theme
    @Binding private var text: String
    @FocusState private var focused: Bool
    private let focusRequest: Int
    private let canSend: Bool
    private let running: Bool
    private let placeholder: String
    private let hint: String?
    private let send: () -> Void
    private let stop: () -> Void
    private let settings: Settings
    private let attachments: Attachments
    private let footer: Footer
    private var stops: Bool { running && !canSend }
    private var actionTitle: String { running ? (canSend ? "Queue" : "Stop") : "Send" }

    public init(
        text: Binding<String>, canSend: Bool, running: Bool = false, placeholder: String = "Ask the agent…",
        hint: String? = nil, focusRequest: Int = 0, send: @escaping () -> Void,
        stop: @escaping () -> Void, @ViewBuilder settings: () -> Settings,
        @ViewBuilder attachments: () -> Attachments, @ViewBuilder footer: () -> Footer
    ) {
        _text = text
        self.canSend = canSend
        self.running = running
        self.placeholder = placeholder
        self.hint = hint
        self.focusRequest = focusRequest
        self.send = send
        self.stop = stop
        self.settings = settings()
        self.attachments = attachments()
        self.footer = footer()
    }

    public var body: some View {
        VStack(spacing: ZZAgentMetrics.composerSectionGap) {
            VStack(alignment: .leading, spacing: 0) {
                attachments
                TextField(placeholder, text: $text, axis: .vertical)
                    .focused($focused)
                    .onChange(of: focusRequest) { focused = true }
                    .textFieldStyle(.plain).lineLimit(2...8).font(theme.font(size: 13))
                    .padding(.top, 12).padding(.horizontal, 14)
                    .onSubmit { if canSend { send() } }
                if let hint {
                    Text(hint).font(theme.font(size: 10)).foregroundStyle(theme.foreground.muted().color)
                        .padding(.horizontal, 12).padding(.bottom, 4)
                }
                HStack(alignment: .bottom, spacing: 6) {
                    HStack(spacing: 6) { settings }
                    Spacer(minLength: 0)
                    ZZButton(
                        actionTitle, icon: stops ? "xmark" : (running ? "plus" : "arrow.up"),
                        variant: running ? .default : .accent,
                        size: .xSmall, iconOnly: true, action: stops ? stop : send
                    )
                    .compactChromeIcon()
                    .transformEnvironment(\.zzTheme) { $0.radius = .infinity }
                    .disabled(!running && !canSend)
                    .keyboardShortcut(.return, modifiers: .command)
                    .help(stops ? "Stop the current turn" : (running ? "Queue this as the next turn" : "Send message"))
                }.padding(6).frame(minHeight: 40, alignment: .bottom)
            }
            .frame(minHeight: ZZAgentMetrics.composerMinHeight, alignment: .bottom).background(
                theme.background.raised(1).color,
                in: ZZRoundedRectangle(radius: theme.radius)
            )
            .overlay { ZZRoundedRectangle(radius: theme.radius).stroke(theme.border.color, lineWidth: 1) }
            footer.frame(height: ZZAgentMetrics.composerFooterHeight).padding(.horizontal, 4)
        }
        .frame(maxWidth: ZZAgentMetrics.composerMaxWidth)
        .padding(ZZAgentMetrics.composerOuterPadding).frame(maxWidth: .infinity).background(theme.background.color)
    }
}
