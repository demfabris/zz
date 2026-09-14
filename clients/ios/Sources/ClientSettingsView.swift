import SwiftUI

private enum ZZSettingsSection: String, CaseIterable, Identifiable {
    case appearance, terminal, panes, status, mux
    var id: Self { self }
    var title: String {
        switch self {
        case .appearance: "Appearance"
        case .terminal: "Terminal"
        case .panes: "Panes"
        case .status: "Status Bar"
        case .mux: "Multiplexer"
        }
    }
    var symbol: String {
        switch self {
        case .appearance: "paintpalette"
        case .terminal: "terminal"
        case .panes: "rectangle.split.2x2"
        case .status: "rectangle.bottomthird.inset.filled"
        case .mux: "command"
        }
    }
}

struct ClientSettingsView: View {
    @Environment(ZZClientSettings.self) private var settings
    @Environment(\.dismiss) private var dismiss
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var section: ZZSettingsSection? = .appearance
    @State private var restoring = false

    var body: some View {
        Group {
            if sizeClass == .regular {
                NavigationSplitView {
                    List(ZZSettingsSection.allCases, selection: $section) { item in
                        NavigationLink(value: item) {
                            Label(item.title, systemImage: item.symbol)
                        }
                        .accessibilityIdentifier("settings-section-\(item.rawValue)")
                    }
                    .navigationTitle("Settings")
                    .navigationSplitViewColumnWidth(min: 180, ideal: 210, max: 260)
                } detail: {
                    NavigationStack {
                        SettingsSectionPage(section: section ?? .appearance)
                            .toolbar { doneButton }
                    }
                }
            } else {
                NavigationStack {
                    List {
                        ForEach(ZZSettingsSection.allCases) { item in
                            NavigationLink {
                                SettingsSectionPage(section: item)
                            } label: {
                                Label(item.title, systemImage: item.symbol)
                            }
                            .accessibilityIdentifier("settings-section-\(item.rawValue)")
                        }
                        Section {
                            Button("Restore Defaults", systemImage: "arrow.counterclockwise") {
                                restoring = true
                            }
                        }
                    }
                    .navigationTitle("Settings")
                    .toolbar { doneButton }
                }
            }
        }
        .confirmationDialog("Restore device preferences?", isPresented: $restoring) {
            Button("Restore Defaults", role: .destructive) { settings.restoreDefaults() }
        }
    }

    @ToolbarContentBuilder private var doneButton: some ToolbarContent {
        ToolbarItem(placement: .confirmationAction) {
            Button("Done") { dismiss() }
                .accessibilityIdentifier("settings-done")
        }
    }
}

private struct SettingsSectionPage: View {
    let section: ZZSettingsSection
    @Environment(ZZClientSettings.self) private var settings
    @State private var restoring = false

    var body: some View {
        @Bindable var settings = settings
        Form {
            if section == .appearance {
                Section("App Appearance") {
                    Picker("Appearance", selection: $settings.appearance) {
                        ForEach(ZZAppAppearance.allCases) { appearance in
                            Text(appearance.label).tag(appearance)
                        }
                    }
                    .pickerStyle(.segmented)
                }
            }
            if section == .terminal {
                Section("Preview") {
                    TerminalSettingsPreview()
                        .listRowInsets(EdgeInsets())
                        .listRowBackground(Color.clear)
                }
                Section("Font") {
                    Picker("Typeface", selection: $settings.terminalFont) {
                        ForEach(ZZTerminalFont.allCases) { font in
                            Text(font.label).font(font.swiftUIFont(size: 16)).tag(font)
                        }
                    }
                    .accessibilityIdentifier("settings-typeface")
                    Stepper(value: $settings.terminalFontSize, in: ZZClientSettings.terminalFontSizeRange) {
                        LabeledContent("Size", value: "\(settings.terminalFontSize) pt")
                    }
                    .accessibilityLabel("Terminal font size")
                    .accessibilityValue("\(settings.terminalFontSize) points")
                    if settings.shared == nil {
                        Toggle("Blink cursor", isOn: $settings.cursorBlinking)
                    }
                }
                if let shared = settings.shared {
                    Section("Colors") {
                        NavigationLink {
                            TerminalThemePicker(shared: shared)
                        } label: {
                            LabeledContent("Color Theme", value: shared.text("theme").isEmpty ? "Default" : shared.text("theme"))
                        }
                        .accessibilityIdentifier("settings-color-theme")
                    }
                }
            }
            if let shared = settings.shared {
                SharedSettingsSection(shared: shared, section: section)
                if section == .terminal || section == .mux {
                    Section {
                        NavigationLink(section == .mux ? "Edit mux.conf" : "Edit terminal config") {
                            SettingsSourceEditor(shared: shared, mux: section == .mux)
                        }
                        if section == .mux {
                            NavigationLink("Split Key Bindings") { MuxSplitBindingsView(shared: shared) }
                            NavigationLink("Key Bindings") { MuxKeyBindingsView(shared: shared) }
                        }
                    } footer: {
                        Text(section == .mux
                             ? "Preferences are saved on this device and applied to the connected multiplexer. Session settings affect other clients attached to the same session."
                             : "Terminal appearance is saved on this device.")
                    }
                }
                if let error = shared.error {
                    Section("Could Not Save Settings") {
                        Text(error).foregroundStyle(.red).textSelection(.enabled)
                    }
                }
            }
            if section == .panes {
                Section("iPad Layout") {
                    Toggle("Draw Behind Home Indicator", isOn: $settings.extendPanesUnderHomeIndicator)
                }
            }
            if section == .appearance {
                Section {
                    Button("Restore Defaults", systemImage: "arrow.counterclockwise") { restoring = true }
                }
            }
        }
        .contrast(settings.chromeContrast)
        .navigationTitle(section.title)
        .navigationBarTitleDisplayMode(.inline)
        .confirmationDialog("Restore device preferences?", isPresented: $restoring) {
            Button("Restore Defaults", role: .destructive) { settings.restoreDefaults() }
        }
    }
}

private struct SharedSettingsSection: View {
    let shared: ZZSharedSettings
    let section: ZZSettingsSection

    private var rows: [ZZSetting] {
        (shared.snapshot?.settings ?? []).filter { row in
            if section == .appearance {
                return ["animations", "ui-font-family", "chrome-preset-dark", "chrome-preset-light",
                        "chrome-background", "chrome-foreground", "chrome-accent",
                        "chrome-contrast", "widget-corner-radius", "shadow-strength"].contains(row.key)
            }
            guard row.section == section.rawValue || (section == .mux && row.section == "multiplexer") else { return false }
            return !["theme", "font-family", "font-size", "status-update"].contains(row.key)
        }
    }

    var body: some View {
        if !rows.isEmpty {
            Section(section.title) {
                ForEach(rows) { row in
                    SharedSettingRow(shared: shared, setting: row)
                }
            }
        }
    }
}

private struct SharedSettingRow: View {
    let shared: ZZSharedSettings
    let setting: ZZSetting
    @Environment(ZZClientSettings.self) private var settings

    private var resolvedColor: Color {
        switch setting.key {
        case "chrome-accent": settings.chromeTint
        case "chrome-background": settings.chromeBackground
        case "chrome-foreground": settings.chromeForeground
        case "background": Color(zzRGB: shared.mobileAppearance?.background ?? 0)
        case "foreground": Color(zzRGB: shared.mobileAppearance?.foreground ?? 0xFFFFFF)
        case "cursor-color": Color(zzRGB: shared.mobileAppearance?.cursor_color ?? 0xFFFFFF)
        default: Color(zzHex: setting.value.text)
        }
    }

    var body: some View {
        control
            .disabled(!setting.enabled)
            .contextMenu {
                if setting.overridden {
                    Button("Restore Default", systemImage: "arrow.counterclockwise") { shared.set(setting, .null) }
                }
            }
    }

    @ViewBuilder private var control: some View {
        if setting.key == "ui-font-family" {
            Picker(setting.title, selection: Binding(
                get: { setting.value.text.isEmpty ? "System" : setting.value.text },
                set: { shared.set(setting, .string($0)) }
            )) {
                Text("System").tag("System")
                ForEach(UIFont.familyNames.sorted(), id: \.self) { family in
                    Text(family).font(.custom(family, size: 17)).tag(family)
                }
            }
        } else if setting.control == "color" {
            ColorPicker(setting.title, selection: Binding(
                get: { resolvedColor },
                set: { shared.set(setting, .string($0.zzHex)) }
            ), supportsOpacity: false)
        } else if setting.control == "boolean" {
            Toggle(setting.title, isOn: Binding(
                get: { setting.value.bool ?? ["on", "true", "1"].contains(setting.value.text) },
                set: { shared.set(setting, .boolean($0)) }
            ))
        } else if !setting.choices.isEmpty {
            Picker(setting.title, selection: Binding(
                get: { setting.value.text },
                set: { shared.set(setting, $0.isEmpty ? .null : .string($0)) }
            )) {
                Text("Default").tag("")
                ForEach(setting.choices) { choice in Text(choice.title).tag(choice.value) }
            }
        } else if setting.control == "number", let bounds = setting.range, bounds.count == 2, bounds[1] <= 100 {
            let fractional = bounds[1] <= 1
            VStack(alignment: .leading) {
                LabeledContent(setting.title, value: setting.value.number?.formatted(.number.precision(.fractionLength(0...2))) ?? setting.value.text)
                Slider(value: Binding(
                    get: { setting.value.number ?? bounds[0] },
                    set: { shared.set(setting, .number($0)) }
                ), in: bounds[0]...bounds[1], step: fractional ? 0.05 : 1)
                .accessibilityLabel(setting.title)
            }
        } else {
            NavigationLink {
                SettingsValueEditor(shared: shared, setting: setting)
            } label: {
                LabeledContent(setting.title, value: setting.value.text.isEmpty ? "Default" : setting.value.text)
            }
        }
    }
}

private struct SettingsValueEditor: View {
    let shared: ZZSharedSettings
    let setting: ZZSetting
    @Environment(\.dismiss) private var dismiss
    @State private var draft = ""

    var body: some View {
        Form {
            TextField(setting.title, text: $draft)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .keyboardType(setting.control == "number" ? .numbersAndPunctuation : .default)
            Button("Save") {
                shared.set(setting, .string(draft))
                if shared.error == nil { dismiss() }
            }
            Button("Restore Default") { shared.set(setting, .null); dismiss() }
            if let error = shared.error { Text(error).foregroundStyle(.red) }
        }
        .navigationTitle(setting.title)
        .onAppear { draft = setting.value.text }
    }
}

private struct SettingsSourceEditor: View {
    let shared: ZZSharedSettings
    let mux: Bool
    @State private var draft = ""
    @State private var saved = ""

    var body: some View {
        VStack(alignment: .leading) {
            TextEditor(text: $draft)
                .font(.system(.body, design: .monospaced))
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .accessibilityLabel(mux ? "Multiplexer configuration" : "Terminal configuration")
            if let error = shared.error { Text(error).foregroundStyle(.red).padding() }
        }
        .navigationTitle(mux ? "mux.conf" : "Terminal Config")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button("Save") {
                    if shared.action(mux ? "save-mux" : "save-terminal", ["source": draft]) { saved = draft }
                }
                .disabled(draft == saved)
            }
        }
        .onAppear {
            draft = (mux ? shared.snapshot?.mux_source : shared.snapshot?.terminal_source) ?? ""
            saved = draft
        }
    }
}

private struct MuxSplitBindingsView: View {
    let shared: ZZSharedSettings
    @EnvironmentObject private var store: ZZStore
    @State private var horizontalKey = ""
    @State private var verticalKey = ""
    @State private var horizontalKind = "picker"
    @State private var verticalKind = "picker"

    var body: some View {
        Form {
            Section("Side by Side") {
                TextField("Prefix key", text: $horizontalKey)
                    .textInputAutocapitalization(.never).autocorrectionDisabled()
                kindPicker(selection: $horizontalKind)
                Button("Save Side by Side Binding") { save(horizontal: true) }
                    .disabled(store.client == nil || shared.horizontalBinding?.editable == false)
            }
            Section("Top and Bottom") {
                TextField("Prefix key", text: $verticalKey)
                    .textInputAutocapitalization(.never).autocorrectionDisabled()
                kindPicker(selection: $verticalKind)
                Button("Save Top and Bottom Binding") { save(horizontal: false) }
                    .disabled(store.client == nil || shared.verticalBinding?.editable == false)
            }
            Section {
                Text("Keys follow the multiplexer prefix. Choose Picker to select the new pane type, or Terminal to split immediately.")
                if shared.horizontalBinding?.editable == false || shared.verticalBinding?.editable == false {
                    Text("A custom command uses one of these bindings. Edit mux.conf to change it.")
                }
                if let error = shared.error { Text(error).foregroundStyle(.red) }
            }
        }
        .navigationTitle("Split Key Bindings")
        .onAppear {
            shared.refresh(client: store.client)
            horizontalKey = shared.horizontalBinding?.key ?? "%"
            verticalKey = shared.verticalBinding?.key ?? "\""
            horizontalKind = shared.horizontalBinding?.kind ?? "picker"
            verticalKind = shared.verticalBinding?.kind ?? "picker"
        }
    }

    private func kindPicker(selection: Binding<String>) -> some View {
        Picker("New Pane", selection: selection) {
            Text("Picker").tag("picker")
            Text("Terminal").tag("terminal")
            if selection.wrappedValue == "browser" { Text("Browser (host binding)").tag("browser") }
        }
    }

    private func save(horizontal: Bool) {
        shared.action("split-binding", [
            "horizontal": horizontal,
            "key": horizontal ? horizontalKey : verticalKey,
            "kind": horizontal ? horizontalKind : verticalKind,
        ], client: store.client)
    }
}

private struct MuxKeyBindingsView: View {
    let shared: ZZSharedSettings
    var body: some View {
        List {
            Section {
                Text("Use Prefix to enter the multiplexer key table. Hardware keyboards send the same keys as the desktop terminal.")
            }
            Section("Prefix Bindings") {
                if shared.prefixBindings.isEmpty { Text("Connect to a host to see its active bindings.").foregroundStyle(.secondary) }
                ForEach(shared.prefixBindings) { binding in
                    LabeledContent(binding.key, value: binding.command)
                        .font(.system(.body, design: .monospaced))
                }
            }
        }
        .navigationTitle("Key Bindings")
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
                    .phaseAnimator(settings.cursorBlinking ? [true, false] : [true]) { content, visible in
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
