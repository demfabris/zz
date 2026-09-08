import SwiftUI
import ZZUI

struct InputGallery: View {
    @Environment(\.zzTheme) private var theme
    @State private var name = "zz native"
    @State private var query = ""
    @State private var password = "correct horse battery staple"
    @State private var url = "zzmux.sh"
    @State private var email = "fabrico@example.com"
    @State private var invalid = "not an email"
    @State private var notes = "Native editing, selection, undo, and dictation.\nTry emoji: 🦀 👨‍👩‍👧‍👦"
    @State private var smallText = "Compact field"
    @State private var number = "12"
    @State private var fraction = "0.5"
    @State private var emptyNumber = ""
    @State private var selected: String? = "rust"
    @State private var selectedFont: String? = "sf"
    @State private var optionalSelection: String?
    @State private var compactOn = true
    @State private var regularOn = false
    @State private var color: ZZColor? = ZZColor(hex: "#7aa2f7")
    @State private var inheritedColor: ZZColor?

    init() {}

    private let languages: [ZZSelectOption] = [
        .init("rust", title: "Rust", group: "Systems", detail: "Shared client core", systemImage: "gearshape.2"),
        .init("swift", title: "Swift", group: "Systems", detail: "Native macOS presentation", systemImage: "swift"),
        .init("c", title: "C", group: "Systems", detail: "Stable foreign interface"),
        .init("typescript", title: "TypeScript", group: "Web"),
        .init("javascript", title: "JavaScript", group: "Web"),
        .init("unavailable", title: "Unavailable option", group: "Web", disabled: true),
    ]

    var body: some View {
        VStack(spacing: 18) {
            GallerySection("Text fields", detail: "Native macOS text editing inside the zz control surface.") {
                Grid(alignment: .leading, horizontalSpacing: 14, verticalSpacing: 12) {
                    GridRow {
                        field("Text") { ZZTextField("Workspace name", text: $name, clearable: true) }
                        field("Search") {
                            ZZTextField("Search sessions", text: $query, contentType: .search, clearable: true)
                        }
                    }
                    GridRow {
                        field("Password") {
                            ZZTextField("Password", text: $password, contentType: .password, leadingIcon: "lock")
                        }
                        field("Address") {
                            ZZTextField(
                                "Address", text: $url, contentType: .url, trailingIcon: "arrow.up.right",
                                prefix: "https://")
                        }
                    }
                    GridRow {
                        field("Email") {
                            ZZTextField("Email", text: $email, contentType: .email, leadingIcon: "envelope")
                        }
                        field("Invalid") { ZZTextField("Email address", text: $invalid, invalid: true) }
                    }
                    GridRow {
                        field("Loading") {
                            ZZTextField("Resolving host", text: .constant("Connecting to workstation"), isLoading: true)
                        }
                        field("Disabled") {
                            ZZTextField(
                                "Unavailable", text: .constant("Read-only configuration"), leadingIcon: "lock.fill"
                            ).disabled(true)
                        }
                    }
                }
            }
            GallerySection("Control sizes and appearance") {
                HStack(alignment: .bottom, spacing: 12) {
                    ForEach(ZZControlSize.allCases) { size in
                        field("\(size.rawValue) · \(Int(size.height)) pt") {
                            ZZTextField("\(size.rawValue) field", text: $smallText, size: size)
                        }
                    }
                }
                HStack(spacing: 12) {
                    field("Bare") { ZZTextField("Bare input", text: $name, appearance: false) }
                    field("Borderless") { ZZTextField("Borderless input", text: $name, bordered: false) }
                    field("Centered") { ZZTextField("Centered input", text: $name, alignment: .center) }
                }
                ZZTextField("Remote host", text: $name)
                    .leadingAccessory { ZZTag("SSH") }
                    .trailingAccessory { ZZKbd("⌘K") }
            }
            GallerySection(
                "Multiline input",
                detail:
                    "Grows from three to eight lines, then scrolls. Native selection and marked text stay available."
            ) {
                ZZTextEditor("Notes", text: $notes, minRows: 3, maxRows: 8)
            }
            GallerySection(
                "Number inputs",
                detail: "Use the buttons or ↑ and ↓. Bounds and decimal precision follow the Rust controls."
            ) {
                HStack(spacing: 14) {
                    field("Integer · 0…24") { ZZNumberInput("Font size", text: $number, minimum: 0, maximum: 24) }
                    field("Fraction · 0…1") {
                        ZZNumberInput("Opacity", text: $fraction, step: 0.05, minimum: 0, maximum: 1)
                    }
                    field("Empty · minimum 10") {
                        ZZNumberInput("History limit", text: $emptyNumber, minimum: 10, maximum: 100)
                    }
                    field("Disabled") { ZZNumberInput("Disabled stepper", text: .constant("16")).disabled(true) }
                }
            }
            GallerySection(
                "Select",
                detail: "Selection stays stable while searching. Arrow keys move, Return selects, and Escape closes."
            ) {
                HStack(alignment: .top, spacing: 14) {
                    field("Searchable and grouped") {
                        ZZSelect(
                            "Language", selection: $selected, options: languages, searchable: true, clearable: true)
                    }
                    field("Single selection") {
                        ZZSelect(
                            "Font", selection: $selectedFont,
                            options: [
                                .init("sf", title: "SF Mono"), .init("jetbrains", title: "JetBrains Mono"),
                                .init("iosevka", title: "Iosevka"), .init("menlo", title: "Menlo"),
                            ])
                    }
                    field("Placeholder") {
                        ZZSelect(
                            "Choose a language", selection: $optionalSelection, options: languages,
                            placeholder: "Choose…")
                    }
                }
                HStack(spacing: 14) {
                    ZZSelect("Empty", selection: .constant(nil), options: [], placeholder: "No options available")
                    ZZSelect("Disabled", selection: .constant("rust"), options: languages).disabled(true)
                }
            }
            GallerySection("Switches", detail: "The zz track sizes with native keyboard and accessibility behavior.") {
                HStack(spacing: 30) {
                    ZZSwitch("Small · 28 × 16", isOn: $compactOn, size: .small)
                    ZZSwitch("Regular · 36 × 20", isOn: $regularOn)
                    ZZSwitch("Disabled", isOn: .constant(true), size: .small).disabled(true)
                    ZZSwitch("Icon-only switch", isOn: $compactOn, size: .small, showsLabel: false)
                }
            }
            GallerySection(
                "Color picker",
                detail:
                    "40 zz presets, hexadecimal overrides, and the macOS color panel. Clearing restores the inherited color."
            ) {
                HStack(spacing: 24) {
                    field("Accent override") {
                        ZZColorPicker("Accent", selection: $color, inherited: theme.foreground, showsValue: true)
                    }
                    field("Inherited background") {
                        ZZColorPicker(
                            "Background", selection: $inheritedColor, inherited: theme.background.raised(2),
                            showsValue: true)
                    }
                    field("Compact swatch") {
                        ZZColorPicker("Compact accent", selection: $color, inherited: theme.foreground)
                    }
                    field("Disabled") {
                        ZZColorPicker("Disabled color", selection: .constant(nil), inherited: theme.warning).disabled(
                            true)
                    }
                }
            }
        }
    }

    private func field<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(theme.font(size: 11)).foregroundStyle(theme.foreground.muted().color)
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
