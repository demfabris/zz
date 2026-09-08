import Observation
import SwiftUI

public struct ZZDialog<Content: View, Actions: View>: View {
    @Environment(\.zzTheme) private var theme
    private let title: String
    private let description: String?
    private let content: Content
    private let actions: Actions

    public init(
        _ title: String,
        description: String? = nil,
        @ViewBuilder content: () -> Content,
        @ViewBuilder actions: () -> Actions
    ) {
        self.title = title
        self.description = description
        self.content = content()
        self.actions = actions()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title).font(theme.font(size: 13, weight: .semibold))
                .accessibilityAddTraits(.isHeader)
            if let description {
                Text(description).font(theme.font(size: 12))
                    .foregroundStyle(theme.foreground.muted().color)
                    .fixedSize(horizontal: false, vertical: true)
            }
            content
            HStack(spacing: 8) {
                Spacer(minLength: 0)
                actions
            }
        }
        .padding(12)
        .frame(width: 400, alignment: .leading)
        .frame(minHeight: 80)
        .foregroundStyle(theme.foreground.color)
        .background(theme.background.raised(1).color)
    }
}

extension ZZDialog where Content == EmptyView {
    public init(_ title: String, description: String? = nil, @ViewBuilder actions: () -> Actions) {
        self.init(title, description: description, content: { EmptyView() }, actions: actions)
    }
}

public struct ZZPopover<Label: View, Content: View>: View {
    @Binding private var isPresented: Bool
    private let edge: Edge
    private let label: Label
    private let content: Content

    public init(
        isPresented: Binding<Bool>,
        edge: Edge = .bottom,
        @ViewBuilder label: () -> Label,
        @ViewBuilder content: () -> Content
    ) {
        _isPresented = isPresented
        self.edge = edge
        self.label = label()
        self.content = content()
    }

    public var body: some View {
        Button {
            isPresented.toggle()
        } label: {
            label
        }
        .buttonStyle(.plain)
        .popover(isPresented: $isPresented, arrowEdge: edge) {
            content.padding(12)
        }
    }
}

public struct ZZMenu<Content: View>: View {
    @Environment(\.zzTheme) private var theme
    @State private var hovered = false
    private let title: String
    private let icon: String?
    private let flat: Bool
    private let content: Content

    public init(_ title: String, icon: String? = nil, flat: Bool = false, @ViewBuilder content: () -> Content) {
        self.title = title
        self.icon = icon
        self.flat = flat
        self.content = content()
    }

    public var body: some View {
        Menu {
            content
        } label: {
            if flat {
                label.background(
                    hovered ? theme.background.washed(2).color : .clear,
                    in: ZZRoundedRectangle(radius: theme.radius))
            } else {
                label.zzControlSurface()
            }
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .menuIndicator(.hidden)
        .fixedSize()
        .onHover { hovered = $0 }
    }

    private var label: some View {
        HStack(spacing: flat ? 4 : 6) {
            if let icon {
                Image(systemName: icon).font(.system(size: 14))
                    .foregroundStyle(theme.foreground.muted().color)
                    .frame(width: 14, height: 14).offset(y: 0.5).accessibilityHidden(true)
            }
            Text(title).font(theme.font(size: flat ? 12 : 13)).lineLimit(1)
            Image(systemName: "chevron.down").font(.system(size: 10))
                .foregroundStyle(theme.foreground.muted().color).accessibilityHidden(true)
        }
        .foregroundStyle(theme.foreground.color)
        .padding(.horizontal, 8)
        .frame(height: flat ? 24 : 28)
    }
}

extension View {
    public func zzTooltip(_ text: String, shortcut: String? = nil) -> some View {
        help(shortcut.map { "\(text) (\($0))" } ?? text)
    }

    public func zzContextMenu<Items: View>(@ViewBuilder items: () -> Items) -> some View {
        contextMenu(menuItems: items)
    }

    public func zzToastOverlay(_ center: ZZToastCenter) -> some View {
        overlay(alignment: .top) {
            if !center.toasts.isEmpty {
                GeometryReader { geometry in
                    ZZToastStack(center: center)
                        .frame(width: max(0, min(400, geometry.size.width - 24)))
                        .frame(maxHeight: max(0, geometry.size.height - 24), alignment: .top)
                        .frame(maxWidth: .infinity, alignment: .top)
                        .padding(.top, 12)
                }
            }
        }
    }
}

public enum ZZToastKind: String, CaseIterable, Sendable {
    case info, success, warning, error

    var icon: String {
        switch self {
        case .info: "info.circle"
        case .success: "checkmark.circle"
        case .warning: "exclamationmark.triangle"
        case .error: "xmark.circle"
        }
    }
}

public struct ZZToast: Identifiable, Equatable, Sendable {
    public let id: UUID
    public let key: String?
    public let title: String?
    public let message: String
    public let kind: ZZToastKind?

    public init(
        id: UUID = UUID(), key: String? = nil, title: String? = nil,
        message: String, kind: ZZToastKind? = nil
    ) {
        self.id = id
        self.key = key
        self.title = title
        self.message = message
        self.kind = kind
    }
}

@MainActor @Observable public final class ZZToastCenter {
    public private(set) var toasts: [ZZToast] = []
    @ObservationIgnored private var dismissals: [UUID: Task<Void, Never>] = [:]
    @ObservationIgnored private var contents: [UUID: AnyView] = [:]

    var visibleToasts: [ZZToast] { Array(toasts.suffix(10)) }

    public init() {}

    deinit {
        for task in dismissals.values { task.cancel() }
    }

    @discardableResult public func show(
        _ message: String, title: String? = nil, kind: ZZToastKind? = nil,
        key: String? = nil, duration: Duration? = .seconds(5)
    ) -> UUID {
        if let key { dismiss(key: key) }
        let toast = ZZToast(key: key, title: title, message: message, kind: kind)
        toasts.append(toast)
        if let duration {
            dismissals[toast.id] = Task { [weak self] in
                do { try await Task.sleep(for: duration) } catch { return }
                self?.dismiss(id: toast.id)
            }
        }
        return toast.id
    }

    @discardableResult public func show<Content: View>(
        _ message: String = "", title: String? = nil, kind: ZZToastKind? = nil,
        key: String? = nil, duration: Duration? = .seconds(5),
        @ViewBuilder content: (@escaping () -> Void) -> Content
    ) -> UUID {
        let id = show(message, title: title, kind: kind, key: key, duration: duration)
        contents[id] = AnyView(content { [weak self] in self?.dismiss(id: id) })
        return id
    }

    func content(for id: UUID) -> AnyView? { contents[id] }

    public func dismiss(id: UUID) {
        dismissals.removeValue(forKey: id)?.cancel()
        contents.removeValue(forKey: id)
        toasts.removeAll { $0.id == id }
    }

    public func dismiss(key: String) {
        for toast in toasts where toast.key == key { dismiss(id: toast.id) }
    }

    public func dismissAll() {
        for task in dismissals.values { task.cancel() }
        dismissals.removeAll()
        contents.removeAll()
        toasts.removeAll()
    }
}

public struct ZZToastView<Content: View>: View {
    @Environment(\.zzTheme) private var theme
    private let toast: ZZToast
    private let dismiss: () -> Void
    private let content: Content

    public init(_ toast: ZZToast, dismiss: @escaping () -> Void, @ViewBuilder content: () -> Content) {
        self.toast = toast
        self.dismiss = dismiss
        self.content = content()
    }

    private var accent: ZZColor {
        switch toast.kind {
        case .success: theme.success
        case .warning: theme.warning
        case .error: theme.danger
        default: theme.foreground
        }
    }

    public var body: some View {
        HStack(spacing: 8) {
            HStack(alignment: .top, spacing: 8) {
                if let kind = toast.kind {
                    Image(systemName: kind.icon)
                        .font(.system(size: 14))
                        .foregroundStyle(accent.color)
                        .frame(height: 18)
                        .accessibilityLabel(kind.rawValue)
                }
                VStack(alignment: .leading, spacing: 2) {
                    if let title = toast.title {
                        Text(title).font(theme.font(size: 13, weight: .semibold))
                            .frame(minHeight: 18)
                    }
                    if !toast.message.isEmpty {
                        Text(toast.message).font(theme.font(size: 12))
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(minHeight: 18)
                    }
                    content.font(theme.font(size: 12))
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            ZZIconButton("Dismiss notification", systemName: "xmark", action: dismiss)
        }
        .padding(12)
        .frame(maxWidth: 400)
        .foregroundStyle(theme.foreground.color)
        .zzSurface(elevation: 1)
        .accessibilityElement(children: .contain)
    }
}

extension ZZToastView where Content == EmptyView {
    public init(_ toast: ZZToast, dismiss: @escaping () -> Void) {
        self.init(toast, dismiss: dismiss) { EmptyView() }
    }
}

private struct ZZToastStack: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let center: ZZToastCenter

    var body: some View {
        ViewThatFits(in: .vertical) {
            stack
            ScrollView {
                stack.padding(.horizontal, 1)
            }
        }
        .animation(reduceMotion ? nil : .easeOut(duration: 0.25), value: center.visibleToasts)
    }

    private var stack: some View {
        VStack(spacing: 12) {
            ForEach(center.visibleToasts) { toast in
                ZZToastView(toast, dismiss: { center.dismiss(id: toast.id) }) {
                    if let content = center.content(for: toast.id) { content }
                }
                .transition(reduceMotion ? .opacity : .move(edge: .top).combined(with: .opacity))
            }
        }
    }
}
