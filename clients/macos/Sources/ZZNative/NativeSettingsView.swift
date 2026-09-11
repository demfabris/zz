import AppKit
import SwiftUI
import ZZNativeCore
import ZZUI

struct NativeSettingsView: View {
    let client: NativeClient
    @Bindable var model: NativeSettings
    @Environment(\.zzTheme) private var theme
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.dismiss) private var dismiss
    @State private var hostName = ""
    @State private var hostEndpoint = ""
    @State private var confirmReload = false
    @State private var confirmImport = false
    private let pages: [ZZSettingsNavigationItem] = [
        .init("interface", title: "Interface", icon: "paintbrush", group: "Appearance"),
        .init("status", title: "Status bar", icon: "rectangle.topthird.inset.filled", group: "Appearance"),
        .init("panes", title: "Panes", icon: "rectangle.split.2x1", group: "Appearance"),
        .init("editor", title: "Editor", icon: "doc.text", group: "Tools"),
        .init("browser", title: "Browser", icon: "globe", group: "Tools"),
        .init("hosts", title: "Hosts", icon: "network", group: "Tools"),
        .init("system", title: "System", icon: "gearshape", group: "Application"),
        .init("terminal", title: "Terminal", icon: "terminal", group: "Configuration"),
        .init("multiplexer", title: "Multiplexer", icon: "square.3.layers.3d", group: "Configuration"),
        .init("about", title: "About", icon: "info.circle", group: "Application"),
    ]

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Settings").font(theme.font(size: 14, weight: .medium))
                Spacer()
                ZZButton("Done") { dismiss() }.keyboardShortcut(.cancelAction)
            }.padding(12)
            Divider()
            HStack(spacing: 0) {
                ZZSettingsNavigation(selection: $model.section, items: pages)
                Divider()
                VStack(spacing: 0) {
                    if let error = model.error {
                        Text(error).foregroundStyle(theme.danger.color).textSelection(.enabled).padding(12)
                    }
                    ZZSettingsPage(pages.first { $0.id == model.section }?.title ?? "Settings") {
                        if model.section == "terminal" || model.section == "multiplexer" {
                            editor
                        } else if model.section == "hosts" {
                            hosts
                        } else {
                            if model.section == "interface" {
                                palettes
                                ZZSettingEntry(
                                    "UI zoom", description: "Applies to this run of the app.",
                                    reset: model.uiZoom == 100 ? nil : { model.setUIZoom(100) }
                                ) {
                                    Text("\(Int(model.uiZoom))%")
                                    Stepper(
                                        "UI zoom", value: Binding(get: { model.uiZoom }, set: { model.setUIZoom($0) }),
                                        in: 50...300, step: 10
                                    ).labelsHidden()
                                }
                            }
                            if model.section == "about" { about }
                            ZZSettingsStack {
                                ForEach(
                                    model.snapshot?.settings.filter {
                                        $0.section == model.section && !$0.key.hasPrefix("chrome-preset-") && $0.key != "chrome-contrast"
                                    } ?? []
                                ) { setting in
                                    NativeSettingRow(setting: setting, model: model)
                                    ZZSettingsDivider()
                                }
                            }
                        }
                        if let snapshot = model.snapshot, !snapshot.diagnostics.isEmpty {
                            ZZSettingsStack("Configuration errors") {
                                ForEach(Array(snapshot.diagnostics.enumerated()), id: \.offset) { _, diagnostic in
                                    Text("Line \(diagnostic.line): \(diagnostic.message)")
                                        .font(theme.font(size: 12)).textSelection(.enabled).padding(10)
                                }
                            }
                        }
                    }
                }.frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }.frame(width: 860, height: 680)
            .confirmationDialog("Discard unsaved configuration edits?", isPresented: $confirmReload) {
                Button("Discard edits", role: .destructive) { model.reloadDrafts() }
            }
            .confirmationDialog(
                "Import Ghostty appearance? This replaces matching terminal settings.", isPresented: $confirmImport
            ) {
                Button("Import") { model.action("import-ghostty", ["dark": (colorScheme == .dark)]) }
            }
    }

    private var palettes: some View {
        ForEach([false, true], id: \.self) { dark in
            let key = dark ? "chrome-preset-dark" : "chrome-preset-light"
            ZZSettingsStack(
                dark ? "Dark palette" : "Light palette",
                description: dark ? "Used while the interface is dark." : "Used while the interface is light."
            ) {
                ZZPickerStrip {
                    ZZPaletteTile(
                        "Default", palette: dark ? .dark : .light, selected: model.text(key).isEmpty
                    ) { model.action("reset", ["key": key]) }
                    ForEach(model.snapshot?.presets.filter { $0.dark == dark } ?? []) { preset in
                        ZZPaletteTile(
                            preset.name, palette: palette(preset, dark: dark), selected: model.text(key) == preset.id
                        ) { model.action("preset", ["value": preset.id]) }
                    }
                }
            }
        }
    }

    private func palette(_ preset: NativeSettingsSnapshot.Preset, dark: Bool) -> ZZTheme {
        var palette: ZZTheme = dark ? .dark : .light
        palette.background = ZZColor(hex: preset.background) ?? palette.background
        palette.foreground = ZZColor(hex: preset.foreground) ?? palette.foreground
        palette.accent = ZZColor(hex: preset.accent) ?? palette.accent
        palette.success = ZZColor(hex: preset.success) ?? palette.success
        palette.warning = ZZColor(hex: preset.warning) ?? palette.warning
        palette.danger = ZZColor(hex: preset.danger) ?? palette.danger
        return palette
    }

    private var hosts: some View {
        VStack(spacing: 16) {
            ZZSettingsStack("Saved hosts") {
                ForEach(model.snapshot?.hosts ?? []) { host in
                    ZZSettingEntry(host.name, description: host.endpoint) {
                        ZZButton("Connect") {
                            client.endpoint = host.endpoint
                            client.sessionTarget = ""
                            client.connect()
                            dismiss()
                        }
                        ZZIconButton("Remove \(host.name)", systemName: "trash") {
                            model.action("remove-host", ["name": host.name])
                        }
                    }
                }
                if model.snapshot?.hosts.isEmpty != false {
                    Text("No saved hosts").padding(16).foregroundStyle(.secondary)
                }
            }
            ZZSettingsStack("Add host", description: "Use a local socket or ssh://user@host.") {
                VStack(spacing: 12) {
                    ZZTextField("Name", text: $hostName, placeholder: "Workstation")
                    ZZTextField("Endpoint", text: $hostEndpoint, placeholder: "ssh://user@host")
                    ZZButton("Add host", variant: .primary) {
                        if model.action("add-host", ["name": hostName, "endpoint": hostEndpoint]) {
                            hostName = ""; hostEndpoint = ""
                        }
                    }.disabled(hostName.isEmpty || hostEndpoint.isEmpty)
                }.padding(12)
            }
        }
    }

    private var editor: some View {
        let mux = model.section == "multiplexer"
        return VStack(alignment: .leading, spacing: 12) {
            if mux {
                ZZSettingsStack("Split shortcuts") {
                    NativeSplitSetting(horizontal: true, model: model)
                    ZZSettingsDivider()
                    NativeSplitSetting(horizontal: false, model: model)
                }
                DisclosureGroup("Active prefix bindings") {
                    ForEach(Array((model.snapshot?.prefix_bindings ?? []).enumerated()), id: \.offset) { _, binding in
                        HStack(alignment: .top) {
                            Text(binding.key).frame(width: 80, alignment: .leading)
                            Text(binding.command).textSelection(.enabled)
                            Spacer()
                        }.font(.system(size: 11, design: .monospaced)).padding(.vertical, 2)
                    }
                }
            }
            HStack {
                Text((mux ? model.snapshot?.mux_path : model.snapshot?.config_path) ?? "No configuration path")
                    .font(.system(size: 11, design: .monospaced)).textSelection(.enabled)
                Spacer()
                if let path = mux ? model.snapshot?.mux_path : model.snapshot?.config_path {
                    ZZIconButton("Reveal configuration", systemName: "folder") {
                        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
                    }
                }
            }
            ZZCodeEditor(
                text: mux ? $model.muxDraft : $model.terminalDraft,
                fontSize: model.number("editor-font-size", fallback: 13),
                showsLineNumbers: model.bool("editor-line-numbers"), softWrap: model.bool("editor-soft-wrap"),
                relativeLineNumbers: model.bool("editor-relative-line-numbers")
            )
            .frame(minHeight: 330).clipShape(RoundedRectangle(cornerRadius: 8))
            HStack {
                ZZButton("Reload") {
                    if model.muxDirty || model.terminalDirty { confirmReload = true } else { model.reloadDrafts() }
                }
                if !mux {
                    ZZButton("Import Ghostty") { confirmImport = true }
                        .disabled(model.snapshot?.ghostty_path == nil)
                }
                Spacer()
                if mux ? model.muxDirty : model.terminalDirty { Text("Unsaved changes").font(theme.font(size: 11)) }
                ZZButton("Save", variant: .primary) {
                    model.action(
                        mux ? "save-mux" : "save-terminal", ["source": mux ? model.muxDraft : model.terminalDraft])
                }.keyboardShortcut("s", modifiers: .command)
            }
            if mux {
                Text(
                    "Multiplexer changes reload on the connected daemon. Terminal appearance is applied to this client’s connection."
                )
                .font(theme.font(size: 11)).foregroundStyle(.secondary)
                DisclosureGroup("Configuration search paths") {
                    ForEach((model.snapshot?.mux_sources ?? []), id: \.self) {
                        Text($0).font(.system(size: 11, design: .monospaced)).textSelection(.enabled)
                    }
                }
            }
        }
    }

    private var about: some View {
        ZZSettingsStack("zz Native") {
            VStack(alignment: .leading, spacing: 12) {
                Text(
                    "Version \(model.snapshot?.version ?? "") · \(model.snapshot?.platform ?? "") \(model.snapshot?.architecture ?? "")"
                )
                HStack {
                    ZZButton("Copy version") {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(
                            "zz Native \(model.snapshot?.version ?? "") · \(model.snapshot?.platform ?? "") \(model.snapshot?.architecture ?? "")",
                            forType: .string)
                    }
                    ZZButton(model.updates.checking ? "Checking…" : "Check for updates") { model.updates.check() }
                        .disabled(model.updates.checking)
                    Link("Releases", destination: URL(string: "https://github.com/demfabris/zz/releases")!)
                }
                if let result = model.updates.result {
                    if result.state == "available" {
                        Text("Version \(result.version ?? "") is available.")
                        if let asset = result.asset, let url = URL(string: asset.url) {
                            Link("Download native macOS app", destination: url)
                        } else {
                            Text(
                                "This release has no native macOS download. Build the native client from source to update."
                            )
                            .font(theme.font(size: 11)).foregroundStyle(.secondary)
                        }
                    } else if result.state == "up_to_date" {
                        Text("You’re up to date.")
                    }
                    if let error = result.error {
                        Text(error).foregroundStyle(theme.danger.color).textSelection(.enabled)
                    }
                }
                if let error = model.updates.error { Text(error).foregroundStyle(theme.danger.color) }
            }.padding(12)
        }
    }
}

private struct NativeSettingRow: View {
    let setting: NativeSetting
    let model: NativeSettings
    @State private var draft = ""

    var body: some View {
        ZZSettingEntry(
            setting.title,
            description: ["editor-vim-mode", "experimental-editor-pane"].contains(setting.key)
                ? "Applies to editor panes in the GPUI desktop app." : "",
            provenance: setting.overridden ? "Override" : "Default",
            enabled: setting.enabled, reset: setting.overridden ? { model.reset(setting.key) } : nil
        ) {
            control
        }
        .onAppear { draft = setting.value.text }
        .onChange(of: setting.value) { _, value in draft = value.text }
    }

    @ViewBuilder private var control: some View {
        switch setting.control {
        case "boolean":
            Toggle(
                setting.title,
                isOn: Binding(get: { setting.value.bool ?? false }, set: { model.set(setting.key, .boolean($0)) })
            )
            .labelsHidden().toggleStyle(.switch).controlSize(.small)
        case "choice":
            Picker(
                setting.title,
                selection: Binding(get: { setting.value.text }, set: { model.set(setting.key, .string($0)) })
            ) {
                ForEach(setting.choices, id: \.value) { Text($0.title).tag($0.value) }
            }.labelsHidden().frame(width: 160)
        case "number":
            HStack(spacing: 6) {
                TextField(setting.title, text: $draft).frame(width: 60).onSubmit { commit() }
                Stepper(
                    setting.title,
                    value: Binding(get: { setting.value.number ?? 0 }, set: { model.set(setting.key, .number($0)) }),
                    in: (setting.range?.first ?? 0)...(setting.range?.last ?? 100),
                    step: (setting.range?.last ?? 100) <= 2 ? 0.05 : 1
                ).labelsHidden()
            }
        default:
            HStack {
                if setting.control == "color" {
                    ColorPicker(
                        setting.title,
                        selection: Binding(
                            get: { ZZColor(hex: draft)?.color ?? .clear },
                            set: { color in
                                if let value = NSColor(color).usingColorSpace(.sRGB) {
                                    model.set(
                                        setting.key,
                                        .string(
                                            String(
                                                format: "#%02x%02x%02x%02x", Int((value.redComponent * 255).rounded()),
                                                Int((value.greenComponent * 255).rounded()),
                                                Int((value.blueComponent * 255).rounded()),
                                                Int((value.alphaComponent * 255).rounded()))))
                                }
                            })
                    ).labelsHidden()
                }
                TextField(setting.title, text: $draft).frame(width: setting.control == "color" ? 96 : 160).onSubmit {
                    commit()
                }
                Button {
                    commit()
                } label: {
                    Image(systemName: "checkmark")
                }.help("Apply \(setting.title)")
                    .disabled(draft == setting.value.text)
            }
        }
    }
    private func commit() {
        if setting.control == "number", let number = Double(draft) {
            model.set(setting.key, .number(number))
        } else {
            model.set(setting.key, .string(draft))
        }
    }
}

private struct NativeSplitSetting: View {
    let horizontal: Bool
    let model: NativeSettings
    @State private var key = ""
    @State private var kind = "picker"
    private var binding: NativeSettingsSnapshot.SplitBinding? {
        horizontal ? model.snapshot?.horizontal : model.snapshot?.vertical
    }
    var body: some View {
        ZZSettingEntry(horizontal ? "Split right" : "Split down", enabled: binding?.editable ?? false) {
            TextField("Key", text: $key).frame(width: 65)
            Picker("Pane type", selection: $kind) {
                Text("Picker").tag("picker"); Text("Terminal").tag("terminal"); Text("Browser").tag("browser")
            }.labelsHidden().frame(width: 100)
            ZZButton("Apply") { model.action("split-binding", ["horizontal": horizontal, "key": key, "kind": kind]) }
        }
        .onAppear { load() }
        .onChange(of: binding?.key) { _, _ in load() }
        .onChange(of: binding?.kind) { _, _ in load() }
    }
    private func load() { key = binding?.key ?? (horizontal ? "%" : "\""); kind = binding?.kind ?? "picker" }
}
