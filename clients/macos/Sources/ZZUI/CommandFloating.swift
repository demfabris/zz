import SwiftUI

public struct ZZFloatingMenuRow: View {
    @Environment(\.zzTheme) private var theme
    @State private var hovered = false
    private let name: String
    private let annotation: String?
    private let enabled: Bool
    private let selected: Bool
    private let rowHeight: CGFloat
    private let selectedBackground: ZZColor?
    private let selectedForeground: ZZColor?
    private let action: () -> Void

    public init(
        _ name: String, annotation: String? = nil, enabled: Bool = true, selected: Bool = false,
        rowHeight: CGFloat = 24, selectedBackground: ZZColor? = nil, selectedForeground: ZZColor? = nil,
        action: @escaping () -> Void
    ) {
        self.name = name
        self.annotation = annotation
        self.enabled = enabled
        self.selected = selected
        self.rowHeight = rowHeight
        self.selectedBackground = selectedBackground
        self.selectedForeground = selectedForeground
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack {
                Text(name).lineLimit(1)
                Spacer(minLength: 8)
                if let annotation { Text("(\(annotation))") }
            }.font(theme.font(size: 13, monospaced: true)).padding(.horizontal, 12).frame(height: rowHeight)
                .foregroundStyle(
                    (selected
                        ? selectedForeground ?? theme.foreground.on()
                        : enabled ? theme.foreground : theme.foreground.muted()).color
                )
                .background(
                    selected
                        ? (selectedBackground ?? theme.foreground).color
                        : hovered && enabled ? theme.background.raised(1).opaque().color : .clear
                )
                .contentShape(Rectangle())
        }.buttonStyle(.plain).disabled(!enabled).onHover { hovered = $0 }
            .accessibilityAddTraits(selected ? [.isSelected] : [])
    }
}

public struct ZZFloatingMenuSeparator: View {
    @Environment(\.zzTheme) private var theme
    private let rowHeight: CGFloat

    public init(rowHeight: CGFloat = 24) { self.rowHeight = rowHeight }

    public var body: some View {
        theme.border.color.frame(height: 1).padding(.horizontal, 8).frame(height: rowHeight)
            .accessibilityHidden(true)
    }
}

public struct ZZConfirmPrompt: View {
    @Environment(\.zzTheme) private var theme
    private let prompt: String

    public init(_ prompt: String) { self.prompt = prompt }

    public var body: some View {
        Text(prompt).font(theme.font(size: 13, monospaced: true))
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading).padding(.horizontal, 12)
    }
}
