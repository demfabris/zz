import SwiftUI

public struct ZZAppShell<Sidebar: View, Content: View, Status: View>: View {
    @Environment(\.zzTheme) private var theme
    @Binding private var sidebarVisible: Bool
    private let sidebar: Sidebar
    private let content: Content
    private let status: Status

    public init(
        sidebarVisible: Binding<Bool>, @ViewBuilder sidebar: () -> Sidebar,
        @ViewBuilder content: () -> Content, @ViewBuilder status: () -> Status
    ) {
        _sidebarVisible = sidebarVisible
        self.sidebar = sidebar()
        self.content = content()
        self.status = status()
    }

    public var body: some View {
        VStack(spacing: 0) {
            HSplitView {
                if sidebarVisible { sidebar }
                content.frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            status
        }
        .foregroundStyle(theme.foreground.color).background(theme.background.color)
    }
}

public struct ZZWorkspaceStatusBar<Leading: View, Windows: View, Trailing: View>: View {
    private let leading: Leading
    private let windows: Windows
    private let trailing: Trailing

    public init(
        @ViewBuilder leading: () -> Leading, @ViewBuilder windows: () -> Windows,
        @ViewBuilder trailing: () -> Trailing
    ) {
        self.leading = leading()
        self.windows = windows()
        self.trailing = trailing()
    }

    public var body: some View {
        HStack(spacing: 6) {
            leading
            HStack(spacing: 2) { windows }.frame(maxWidth: .infinity)
            trailing
        }
        .padding(.horizontal, 6).frame(height: 35).background(.ultraThinMaterial)
    }
}

public struct ZZConnectionState<Actions: View>: View {
    @Environment(\.zzTheme) private var theme
    private let message: String
    private let connecting: Bool
    private let actions: Actions

    public init(_ message: String, connecting: Bool = false, @ViewBuilder actions: () -> Actions) {
        self.message = message
        self.connecting = connecting
        self.actions = actions()
    }

    public var body: some View {
        VStack(spacing: 12) {
            if connecting { ZZSpinner() }
            Text(message).font(theme.font(size: 12)).foregroundStyle(theme.foreground.muted().color)
            actions
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity).background(theme.background.color)
    }
}

extension ZZConnectionState where Actions == EmptyView {
    public init(_ message: String, connecting: Bool = false) {
        self.init(message, connecting: connecting) { EmptyView() }
    }
}
