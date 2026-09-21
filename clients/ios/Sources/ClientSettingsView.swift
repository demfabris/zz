import SwiftUI

struct ClientSettingsView: View {
    @Environment(ZZClientSettings.self) private var settings
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        @Bindable var settings = settings
        NavigationStack {
            Form {
                Section("Appearance") {
                    Picker("Appearance", selection: $settings.appearance) {
                        ForEach(ZZAppAppearance.allCases) { appearance in
                            Text(appearance.label)
                                .tag(appearance)
                                .accessibilityIdentifier("settings-appearance-\(appearance.rawValue)")
                        }
                    }
                    .pickerStyle(.segmented)
                }
                Section {
                    TerminalSettingsPreview()
                        .listRowInsets(EdgeInsets())
                        .listRowBackground(Color.clear)
                }
                Section("Terminal") {
                    Picker("Font", selection: $settings.terminalFont) {
                        ForEach(ZZTerminalFont.allCases) { font in
                            Text(font.label).font(font.swiftUIFont(size: 16)).tag(font)
                        }
                    }
                    .accessibilityIdentifier("settings-typeface")
                    Stepper(value: $settings.terminalFontSize, in: ZZClientSettings.terminalFontSizeRange) {
                        LabeledContent("Text Size", value: "\(settings.terminalFontSize) pt")
                    }
                    .accessibilityLabel("Terminal font size")
                    .accessibilityValue("\(settings.terminalFontSize) points")
                    if let shared = settings.shared {
                        NavigationLink {
                            TerminalThemePicker(shared: shared)
                        } label: {
                            LabeledContent("Color Theme", value: shared.text("theme").isEmpty ? "Default" : shared.text("theme"))
                        }
                        .accessibilityIdentifier("settings-color-theme")
                    }
                }
                if let error = settings.shared?.error {
                    Section("Could Not Save Settings") {
                        Text(error).foregroundStyle(.red).textSelection(.enabled)
                    }
                }
            }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("settings-done")
                }
            }
        }
        .preferredColorScheme(settings.appearance.colorScheme)
        .accessibilityIdentifier("ipad-settings")
    }
}

private struct TerminalThemePicker: View {
    let shared: ZZSharedSettings
    @State private var search = ""

    private var themes: [ZZTerminalTheme] {
        shared.themes.filter { search.isEmpty || $0.name.localizedStandardContains(search) }
    }

    var body: some View {
        List {
            Button {
                shared.action("set-appearance", ["key": "theme", "value": NSNull()])
            } label: {
                HStack {
                    Text("Default")
                    Spacer()
                    if shared.text("theme").isEmpty { Image(systemName: "checkmark") }
                }
            }
            ForEach(themes) { theme in
                Button {
                    shared.action("set-appearance", ["key": "theme", "value": theme.name])
                } label: {
                    HStack {
                        Text("Aa").font(.system(.body, design: .monospaced))
                            .foregroundStyle(Color(zzHex: theme.foreground))
                            .padding(8)
                            .background(Color(zzHex: theme.background), in: .rect(cornerRadius: 6))
                        Text(theme.name).foregroundStyle(.primary)
                        Spacer()
                        if shared.text("theme") == theme.name { Image(systemName: "checkmark") }
                    }
                    .padding(.vertical, 2)
                }
                .accessibilityAddTraits(shared.text("theme") == theme.name ? [.isSelected] : [])
                .accessibilityIdentifier("terminal-theme-\(theme.name)")
            }
        }
        .searchable(text: $search, prompt: "Search themes")
        .navigationTitle("Color Theme")
    }
}

private struct TerminalSettingsPreview: View {
    @Environment(ZZClientSettings.self) private var settings
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private var foreground: Color { Color(zzRGB: settings.shared?.mobileAppearance?.foreground ?? 0xD8DEE9) }
    private var background: Color { Color(zzRGB: settings.shared?.mobileAppearance?.background ?? 0x101014) }
    private var accent: Color { Color(zzRGB: settings.shared?.mobileAppearance?.palette.dropFirst(2).first ?? 0xA3BE8C) }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 0) {
                Text("you@host").foregroundStyle(accent)
                Text(" ~ % ").foregroundStyle(foreground.opacity(0.72))
                Text("zz attach").foregroundStyle(foreground)
            }
            HStack(spacing: 8) {
                Text("attached to zz").foregroundStyle(foreground.opacity(0.72))
                Rectangle().fill(foreground)
                    .frame(width: 8, height: CGFloat(settings.terminalFontSize))
                    .phaseAnimator(reduceMotion ? [true] : [true, false]) { content, visible in
                        content.opacity(visible ? 1 : 0.28)
                    } animation: { _ in .linear(duration: 0.55) }
                    .accessibilityHidden(true)
            }
        }
        .font(settings.terminalFont.swiftUIFont(size: CGFloat(settings.terminalFontSize)))
        .frame(maxWidth: .infinity, minHeight: 106, alignment: .leading)
        .padding(20)
        .background(background, in: .rect(cornerRadius: 16))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Terminal preview")
        .accessibilityValue("\(settings.terminalFont.label), \(settings.terminalFontSize) points")
    }
}

extension Color {
    var zzHex: String {
        let color = UIColor(self)
        var red: CGFloat = 0, green: CGFloat = 0, blue: CGFloat = 0, alpha: CGFloat = 0
        color.getRed(&red, green: &green, blue: &blue, alpha: &alpha)
        return String(format: "#%02x%02x%02x", Int(red * 255), Int(green * 255), Int(blue * 255))
    }
    init(zzRGB: UInt32) {
        self.init(red: Double((zzRGB >> 16) & 255) / 255,
                  green: Double((zzRGB >> 8) & 255) / 255,
                  blue: Double(zzRGB & 255) / 255)
    }
    init(zzHex: String) {
        var digits = zzHex.trimmingCharacters(in: .whitespacesAndNewlines)
        if digits.hasPrefix("#") { digits.removeFirst() }
        if digits.count == 3 { digits = digits.map { "\($0)\($0)" }.joined() }
        guard [6, 8].contains(digits.count), let value = UInt32(digits, radix: 16) else {
            self.init(zzRGB: 0)
            return
        }
        let alpha = digits.count == 8
        let rgb = alpha ? value >> 8 : value
        self.init(red: Double((rgb >> 16) & 255) / 255,
                  green: Double((rgb >> 8) & 255) / 255,
                  blue: Double(rgb & 255) / 255,
                  opacity: alpha ? Double(value & 255) / 255 : 1)
    }
}
