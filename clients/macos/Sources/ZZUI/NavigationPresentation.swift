import SwiftUI

public struct ZZWorkspaceMarker: View {
    @Environment(\.zzTheme) private var theme
    private let icon: String
    private let bell: Bool
    private let badge: ZZColor?

    public init(_ icon: String, bell: Bool = false, badge: ZZColor? = nil) {
        self.icon = icon
        self.bell = bell
        self.badge = badge
    }

    public var body: some View {
        Image(systemName: icon).font(.system(size: 14)).frame(width: 14, height: 14)
            .overlay(alignment: .topTrailing) {
                if bell { Circle().fill(theme.warning.color).frame(width: 5, height: 5).offset(x: 1, y: -1) }
            }
            .overlay(alignment: .bottomTrailing) {
                if let badge { Circle().fill(badge.color).frame(width: 5, height: 5).offset(x: 1, y: 1) }
            }.frame(width: 18).accessibilityHidden(true)
    }
}

public struct ZZHostIndicator: View {
    @Environment(\.zzTheme) private var theme
    private let connecting: Bool
    private let detail: String?
    private let action: () -> Void

    public init(connecting: Bool = false, detail: String? = nil, action: @escaping () -> Void = {}) {
        self.connecting = connecting
        self.detail = detail
        self.action = action
    }

    public var body: some View {
        Group {
            if connecting {
                ZZSpinner(size: 12).accessibilityLabel("Connecting")
            } else {
                Button(action: action) { Image(systemName: "xmark").font(.system(size: 12)) }
                    .buttonStyle(.plain).disabled(detail == nil)
                    .help(detail ?? "Disconnected").accessibilityLabel(detail ?? "Disconnected")
            }
        }.foregroundStyle(theme.foreground.muted().color).frame(width: 16, height: 24)
    }
}

public struct ZZWindowLayoutMenu: View {
    private let split: (ZZPaneSplitAxis) -> Void

    public init(split: @escaping (ZZPaneSplitAxis) -> Void) { self.split = split }

    public var body: some View {
        Menu {
            Button("Split right", systemImage: "rectangle.righthalf.inset.filled") { split(.horizontal) }
            Button("Split bottom", systemImage: "rectangle.bottomhalf.inset.filled") { split(.vertical) }
        } label: {
            Image(systemName: "rectangle.split.2x1").font(.system(size: 14)).frame(width: 24, height: 24)
        }.menuStyle(.button).buttonStyle(.plain).menuIndicator(.hidden).fixedSize()
            .help("Window layout").accessibilityLabel("Window layout")
    }
}

public struct ZZStatusSession: View {
    private let name: String
    private let action: () -> Void

    public init(_ name: String, action: @escaping () -> Void) {
        self.name = name
        self.action = action
    }

    public var body: some View {
        ZZButton(name, icon: "square.3.layers.3d", variant: .ghost, size: .xSmall, action: action)
    }
}

public struct ZZStatusWindowEntry: Identifiable {
    public let id: String
    public let label: String
    public let active: Bool
    public let select: () -> Void

    public init(_ id: String, label: String, active: Bool = false, select: @escaping () -> Void) {
        self.id = id
        self.label = label
        self.active = active
        self.select = select
    }
}

public struct ZZStatusWindowOverflow: View {
    private let windows: [ZZStatusWindowEntry]

    public init(_ windows: [ZZStatusWindowEntry]) { self.windows = windows }

    public var body: some View {
        Menu {
            ForEach(windows) { entry in
                Button(entry.label, systemImage: entry.active ? "checkmark" : "macwindow", action: entry.select)
            }
        } label: {
            Image(systemName: "ellipsis").font(.system(size: 14)).frame(width: 24, height: 24)
        }.menuStyle(.button).buttonStyle(.plain).menuIndicator(.hidden).fixedSize()
            .help("All windows").accessibilityLabel("All windows")
    }
}

public struct ZZStatusAgentCount: View {
    private let count: Int
    public init(_ count: Int) { self.count = count }
    public var body: some View { ZZWorkspaceStatusItem(count == 1 ? "1 agent" : "\(count) agents", icon: "cpu") }
}

public struct ZZStatusClock: View {
    private let label: String
    public init(_ label: String) { self.label = label }
    public var body: some View { ZZWorkspaceStatusItem(label, icon: "clock") }
}

extension View {
    public func zzRenameMenu(_ label: String, action: @escaping () -> Void) -> some View {
        contextMenu { Button(label, action: action) }
    }
}
