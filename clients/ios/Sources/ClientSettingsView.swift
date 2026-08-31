import SwiftUI

struct ClientSettingsView: View {
    @Environment(ZZClientSettings.self) private var settings
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        @Bindable var settings = settings

        NavigationStack {
            Form {
                Section("Preview") {
                    TerminalSettingsPreview(
                        font: settings.terminalFont,
                        pointSize: CGFloat(settings.terminalFontSize),
                        cursorBlinking: settings.cursorBlinking
                    )
                    .listRowInsets(EdgeInsets())
                    .listRowBackground(Color.clear)
                }

                Section("Appearance") {
                    Picker("Appearance", selection: $settings.appearance) {
                        ForEach(ZZAppAppearance.allCases) { appearance in
                            Text(appearance.label)
                                .tag(appearance)
                        }
                    }
                    .pickerStyle(.segmented)
                }

                Section {
                    Picker("Typeface", selection: $settings.terminalFont) {
                        ForEach(ZZTerminalFont.allCases) { font in
                            Text(font.label)
                                .font(font.swiftUIFont(size: 16))
                                .tag(font)
                        }
                    }

                    Stepper(
                        value: $settings.terminalFontSize,
                        in: ZZClientSettings.terminalFontSizeRange
                    ) {
                        HStack {
                            Text("Size")
                            Spacer()
                            Text("\(settings.terminalFontSize) pt")
                                .foregroundStyle(.secondary)
                                .monospacedDigit()
                        }
                    }
                    .accessibilityLabel("Terminal font size")
                    .accessibilityValue("\(settings.terminalFontSize) points")

                    Toggle("Blink cursor", isOn: $settings.cursorBlinking)
                } header: {
                    Text("Terminal")
                } footer: {
                    Text("Terminal colors come from the connected zz host.")
                }

                Section {
                    Toggle(
                        "Draw Behind Home Indicator",
                        isOn: $settings.extendPanesUnderHomeIndicator
                    )
                } header: {
                    Text("iPad Layout")
                } footer: {
                    Text("Extends pane content into the bottom system inset.")
                }

                Section {
                    Button("Restore Defaults", systemImage: "arrow.counterclockwise") {
                        settings.restoreDefaults()
                    }
                }
            }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") {
                        dismiss()
                    }
                }
            }
        }
    }
}

private struct TerminalSettingsPreview: View {
    let font: ZZTerminalFont
    let pointSize: CGFloat
    let cursorBlinking: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 0) {
                Text("demfabris@macbook")
                    .foregroundStyle(Color.accentColor)
                Text(" ~ % ")
                    .foregroundStyle(.white.opacity(0.72))
                Text("zz attach")
                    .foregroundStyle(.white)
            }

            HStack(spacing: 8) {
                Text("attached to zz")
                    .foregroundStyle(.white.opacity(0.72))
                cursor
            }
        }
        .font(font.swiftUIFont(size: pointSize))
        .frame(maxWidth: .infinity, minHeight: 106, alignment: .leading)
        .padding(20)
        .background {
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .fill(Color.black)
        }
        .overlay {
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .strokeBorder(Color.white.opacity(0.12))
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Terminal preview")
        .accessibilityValue(
            "\(font.label), \(Int(pointSize)) points, \(cursorBlinking ? "blinking" : "steady") cursor"
        )
    }

    private var cursor: some View {
        RoundedRectangle(cornerRadius: 1.5, style: .continuous)
            .fill(Color.accentColor)
            .frame(
                width: max(6, pointSize * 0.52),
                height: max(12, pointSize * 1.18)
            )
            .phaseAnimator(cursorBlinking ? [true, false] : [true]) { content, visible in
                content.opacity(visible ? 1 : 0.28)
            } animation: { _ in
                .linear(duration: 0.55)
            }
            .accessibilityHidden(true)
    }
}
