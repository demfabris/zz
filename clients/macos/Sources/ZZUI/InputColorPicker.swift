import AppKit
import SwiftUI

public struct ZZColorPicker: View {
    @Environment(\.zzTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled
    @Binding private var selection: ZZColor?
    @State private var presented = false
    @State private var hex = ""
    @FocusState private var hexFocused: Bool
    private let title: String
    private let inherited: ZZColor
    private let size: ZZControlSize
    private let showsValue: Bool

    public init(
        _ title: String, selection: Binding<ZZColor?>, inherited: ZZColor, size: ZZControlSize = .small,
        showsValue: Bool = false
    ) {
        self.title = title
        self._selection = selection
        self.inherited = inherited
        self.size = size
        self.showsValue = showsValue
    }

    public static let presets: [ZZColor] = [
        "#000000", "#0a0a0a", "#141414", "#1e1e1e", "#2d2d2d", "#454545", "#6b6b6b", "#9a9a9a", "#cccccc", "#ffffff",
        "#1a1b26", "#16161e", "#1e1e2e", "#181825", "#282828", "#1d2021", "#2e3440", "#3b4252", "#191724", "#1f1d2e",
        "#7aa2f7", "#7dcfff", "#9ece6a", "#e0af68", "#f7768e", "#bb9af7", "#89b4fa", "#a6e3a1", "#f9e2af", "#f38ba8",
        "#89dceb", "#cba6f7", "#fab387", "#94e2d5", "#b4befe", "#83a598", "#fabd2f", "#fb4934", "#d3869b", "#8ec07c",
    ].compactMap(ZZColor.init(hex:))

    public static func committedColor(for text: String, preserving current: ZZColor?) -> ZZColor? {
        text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : ZZColor(hex: text) ?? current
    }

    private var shown: ZZColor { selection ?? inherited }
    private var invalid: Bool {
        !hex.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && ZZColor(hex: hex) == nil
    }

    public var body: some View {
        Button {
            hex = selection?.hex ?? ""
            presented = true
        } label: {
            HStack(spacing: 8) {
                swatch(shown, side: 16)
                if showsValue {
                    Text(selection?.hex ?? "Inherited")
                        .font(theme.font(size: size.fontSize, monospaced: true))
                        .foregroundStyle(theme.foreground.color)
                }
            }
            .padding(.horizontal, showsValue ? size.horizontalPadding : 6)
            .frame(height: size.height)
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .opacity(isEnabled ? 1 : 0.5)
        .accessibilityLabel(title)
        .accessibilityValue(selection?.hex ?? "Inherited, \(inherited.hex)")
        .help("Choose \(title)")
        .popover(isPresented: $presented, arrowEdge: .bottom) { picker }
        .onChange(of: selection) { hex = selection?.hex ?? "" }
    }

    private var picker: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title)
                .font(theme.font(size: 11, weight: .medium))
                .foregroundStyle(theme.foreground.muted().color)
            HStack(spacing: 6) {
                ZZTextField("Hex color", text: $hex, placeholder: "#rrggbb", invalid: invalid, focus: $hexFocused)
                    .onSubmit(commitHex)
                    .onChange(of: hexFocused) { if !hexFocused { commitHex() } }
                Button(action: clear) {
                    Image(systemName: "arrow.uturn.backward")
                        .font(.system(size: 13))
                        .frame(width: 24, height: 28)
                }
                .buttonStyle(.plain)
                .help("Clear the override")
                .accessibilityLabel("Use inherited color")
            }
            LazyVGrid(columns: Array(repeating: GridItem(.fixed(18), spacing: 4), count: 10), spacing: 4) {
                ForEach(Self.presets, id: \.hex) { color in
                    Button {
                        selection = color
                        hex = color.hex
                    } label: {
                        swatch(color, side: 18)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(color.hex)
                    .accessibilityAddTraits(color == selection ? .isSelected : [])
                    .help(color.hex)
                }
            }
            ZZSeparator()
            ColorPicker("Custom color", selection: nativeColor, supportsOpacity: true)
                .font(theme.font(size: 12))
            Text(selection == nil ? "Using inherited \(inherited.hex)" : "Override active")
                .font(theme.font(size: 11))
                .foregroundStyle(theme.foreground.muted().color)
        }
        .padding(12)
        .frame(width: 248)
        .foregroundStyle(theme.foreground.color)
        .background(theme.background.raised(1).color)
        .onDisappear(perform: commitHex)
    }

    private var nativeColor: Binding<Color> {
        Binding(
            get: { shown.color },
            set: { color in
                guard let rgb = NSColor(color).usingColorSpace(.sRGB) else { return }
                selection = ZZColor(
                    red: rgb.redComponent, green: rgb.greenComponent, blue: rgb.blueComponent, alpha: rgb.alphaComponent
                )
                hex = selection?.hex ?? ""
            })
    }

    private func swatch(_ color: ZZColor, side: CGFloat) -> some View {
        ZZRoundedRectangle(radius: theme.radius)
            .fill(color.color)
            .overlay { ZZRoundedRectangle(radius: theme.radius).stroke(theme.border.color, lineWidth: 1) }
            .frame(width: side, height: side)
    }

    private func commitHex() {
        selection = Self.committedColor(for: hex, preserving: selection)
    }

    private func clear() {
        selection = nil
        hex = ""
    }
}
