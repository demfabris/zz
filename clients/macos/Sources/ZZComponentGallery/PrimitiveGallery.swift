import AppKit
import SwiftUI
import ZZUI

struct FoundationGallery: View {
    @Environment(\.zzTheme) private var theme

    var body: some View {
        VStack(spacing: 18) {
            GallerySection(
                "Seven colors, one visual language",
                detail: "All surfaces and interaction states derive from these roots."
            ) {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 94))], alignment: .leading, spacing: 14) {
                    swatch("Background", theme.background)
                    swatch("Foreground", theme.foreground)
                    swatch("Border", theme.border)
                    swatch("Success", theme.success)
                    swatch("Warning", theme.warning)
                    swatch("Danger", theme.danger)
                    swatch("Scrim", theme.scrim)
                }
            }
            GallerySection(
                "Surface elevation", detail: "The GPUI Oklab color derivations, with a shared half-point edge."
            ) {
                HStack(spacing: 12) {
                    ForEach(0..<4) { level in
                        VStack(alignment: .leading, spacing: 25) {
                            Image(systemName: "square.stack.3d.up").font(.system(size: 18))
                            Text(level == 0 ? "Base" : "Raised \(level)").font(.system(size: 12, weight: .medium))
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(16)
                        .zzSurface(elevation: level)
                    }
                }
            }
            GallerySection("Typography", detail: "The system UI face, with a 13-point default for compact controls.") {
                VStack(alignment: .leading, spacing: 16) {
                    HStack {
                        Text("Workspace title").font(.system(size: 20, weight: .semibold))
                        Spacer()
                        metric("20 pt")
                    }
                    HStack {
                        Text("Primary row label").font(.system(size: 13))
                        Spacer()
                        metric("13 pt")
                    }
                    HStack {
                        Text("Descriptions and menu items").font(.system(size: 12)).foregroundStyle(
                            theme.foreground.muted().color)
                        Spacer()
                        metric("12 pt")
                    }
                    HStack {
                        Text("let client = ClientCore::new();").font(.system(size: 13, design: .monospaced))
                        Spacer()
                        metric("13 pt mono")
                    }
                }
            }
            GallerySection("Control density", detail: "Named sizes keep inputs, selects, and buttons aligned.") {
                HStack(alignment: .bottom, spacing: 18) {
                    ForEach(ZZControlSize.allCases) { size in
                        VStack(spacing: 10) {
                            ZZButton(size.rawValue, icon: "plus", size: size) {}
                            metric("\(Int(size.height)) pt")
                        }
                    }
                }
            }
            GallerySection(
                "Native material",
                detail: "macOS supplies translucency, SF Symbols, text services, and window behavior."
            ) {
                HStack(spacing: 10) {
                    Image(systemName: "sidebar.left").font(.system(size: 18))
                    VStack(alignment: .leading, spacing: 3) {
                        Text("A native surface").font(.system(size: 13, weight: .medium))
                        Text("Follows system contrast and transparency preferences").font(.system(size: 12))
                            .foregroundStyle(theme.foreground.muted().color)
                    }
                    Spacer()
                    ZZIconButton("Add", systemName: "plus") {}
                }
                .padding(16)
                .background(.regularMaterial, in: ZZRoundedRectangle(radius: theme.radius))
            }
        }
    }

    private func swatch(_ title: String, _ color: ZZColor) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            ZZRoundedRectangle(radius: theme.radius).fill(color.color)
                .frame(height: 58)
                .overlay {
                    ZZRoundedRectangle(radius: theme.radius).stroke(theme.foreground.opacity(0.1).color, lineWidth: 0.5)
                }
            Text(title).font(.system(size: 11, weight: .medium))
            Text(color.hex).font(.system(size: 10, design: .monospaced)).foregroundStyle(theme.foreground.muted().color)
        }
        .textSelection(.enabled)
    }

    private func metric(_ text: String) -> some View {
        Text(text).font(.system(size: 11, design: .monospaced)).foregroundStyle(theme.foreground.muted().color)
    }
}

struct ButtonGallery: View {
    @Environment(\.zzTheme) private var theme
    @State private var lastAction = "Click any control to try it."
    @State private var selected = true
    @State private var loading = false
    @State private var loadTask: Task<Void, Never>?

    var body: some View {
        VStack(spacing: 18) {
            GallerySection(
                "Variants", detail: "Neutral surfaces preserve the shared background tint on hover and press."
            ) {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 112))], alignment: .leading, spacing: 16) {
                    ForEach(ZZButtonVariant.standard, id: \.title) { variant in
                        ZZButton(variant.title, variant: variant) { lastAction = "\(variant.title) clicked" }
                    }
                    ZZButton("Custom", icon: "paintpalette", variant: .custom(theme.success)) {
                        lastAction = "Custom clicked"
                    }
                }
            }
            GallerySection("Sizes") {
                HStack(alignment: .center, spacing: 16) {
                    ForEach(ZZControlSize.allCases) { size in
                        ZZButton("Size \(size.rawValue)", icon: "plus", size: size) {
                            lastAction = "Size \(size.rawValue) clicked"
                        }
                    }
                }
            }
            GallerySection("Outline and state") {
                HStack(spacing: 12) {
                    ZZButton("Outline", variant: .primary, outline: true) { lastAction = "Outline clicked" }
                    ZZButton("Success", variant: .success, outline: true) { lastAction = "Success clicked" }
                    ZZButton("Selected", icon: "checkmark", selected: selected) {
                        selected.toggle()
                        lastAction = "Selection \(selected ? "on" : "off")"
                    }
                    ZZButton("Disabled", icon: "lock") {}.disabled(true)
                    ZZButton("Working", isLoading: true) {}
                }
            }
            GallerySection(
                "Compact toolbar actions",
                detail: "24-point targets, 14-point SF Symbols, and tooltips with accessible names."
            ) {
                HStack(spacing: 6) {
                    ZZIconButton("New terminal", systemName: "terminal") { lastAction = "New terminal" }
                    ZZIconButton("New browser", systemName: "globe") { lastAction = "New browser" }
                    ZZIconButton("New agent", systemName: "sparkles") { lastAction = "New agent" }
                    ZZSeparator(axis: .vertical).frame(height: 16).padding(.horizontal, 6)
                    ZZIconButton("Split right", systemName: "rectangle.split.2x1") { lastAction = "Split right" }
                    ZZIconButton("Split below", systemName: "rectangle.split.1x2") { lastAction = "Split below" }
                    ZZIconButton("Close", systemName: "xmark", flat: true) { lastAction = "Close" }
                }
            }
            GallerySection("Embedded and dropdown actions") {
                HStack(spacing: 16) {
                    HStack {
                        Text("Font family").font(.system(size: 13))
                        Spacer()
                        ZZButton("Reset", icon: "arrow.counterclockwise", variant: .ghost, flat: true) {
                            lastAction = "Reset font"
                        }
                    }
                    .padding(.leading, 10).padding(.trailing, 3).padding(.vertical, 3)
                    .zzControlSurface()
                    .frame(width: 280)
                    Menu {
                        Button("Terminal") { lastAction = "Terminal selected" }
                        Button("Browser") { lastAction = "Browser selected" }
                        Button("Agent") { lastAction = "Agent selected" }
                    } label: {
                        Text("New pane")
                    }
                    .menuStyle(.borderlessButton)
                    .frame(width: 110)
                    .padding(8)
                    .zzControlSurface()
                }
            }
            GallerySection("Try an asynchronous action") {
                HStack(spacing: 14) {
                    ZZButton("Run action", icon: "play.fill", variant: .primary, isLoading: loading) {
                        loading = true
                        lastAction = "Action running…"
                        loadTask?.cancel()
                        loadTask = Task {
                            do { try await Task.sleep(for: .seconds(1.2)) } catch { return }
                            loading = false
                            lastAction = "Action completed"
                        }
                    }
                    Text(lastAction).font(.system(size: 12)).foregroundStyle(theme.foreground.muted().color)
                        .accessibilityIdentifier("button-action-result")
                }
            }
        }
        .onDisappear { loadTask?.cancel() }
    }
}

struct PrimitiveGallery: View {
    @Environment(\.zzTheme) private var theme
    @State private var selection = "Terminal"

    var body: some View {
        VStack(spacing: 18) {
            GallerySection("Tags", detail: "The three zz-ui variants, in filled, outline, and compact sizes.") {
                VStack(alignment: .leading, spacing: 16) {
                    HStack(spacing: 12) {
                        ForEach(ZZTagVariant.allCases, id: \.self) { variant in
                            ZZTag(variant.rawValue.capitalized, variant: variant)
                            ZZTag(variant.rawValue.capitalized, variant: variant, outline: true)
                        }
                    }
                    HStack(spacing: 12) {
                        ZZTag("Compact", variant: .secondary, size: .small)
                        ZZTag("Selected", variant: .primary, size: .small)
                        ZZTag("Complete", variant: .success, size: .small)
                    }
                }
            }
            GallerySection("Status and key hints") {
                HStack(spacing: 12) {
                    ZZTag("Default")
                    ZZTag("Healthy", tone: .success)
                    ZZTag("Needs attention", tone: .warning)
                    ZZTag("Failed", tone: .danger)
                    Spacer(minLength: 0)
                    ZZKbd("⌘K")
                    ZZKbd("⇧↩")
                    ZZKbd("⎋")
                    ZZKbd("t")
                }
            }
            GallerySection(
                "List selection",
                detail: "Lists keep zz-ui's outlined selection. Menu and navigation rows use their own flat highlight."
            ) {
                VStack(spacing: 5) {
                    ForEach(["Terminal", "Browser", "Agent"], id: \.self) { title in
                        ZZListItem(selected: selection == title, action: { selection = title }) {
                            HStack {
                                Text(title)
                                Spacer()
                                if selection == title { Image(systemName: "checkmark").accessibilityHidden(true) }
                            }
                        }
                    }
                    ZZListItem(action: {}) { Text("Unavailable") }.disabled(true)
                }
                .frame(maxWidth: 440)
            }
            GallerySection(
                "Icons and loading",
                detail: "SF Symbols and the native progress indicator follow system accessibility preferences."
            ) {
                HStack(spacing: 22) {
                    ForEach(ZZControlSize.allCases) { size in ZZIcon(systemName: "terminal", size: size) }
                    ZZSeparator(axis: .vertical).frame(height: 28)
                    ZZSpinner(size: 12)
                    ZZSpinner(size: 16)
                    ZZSpinner(size: 24)
                    Text("Connecting…").font(.system(size: 12)).foregroundStyle(theme.foreground.muted().color)
                }
            }
            GallerySection(
                "Native scrolling",
                detail: "Momentum, trackpad gestures, and scrollbar visibility follow macOS preferences."
            ) {
                ZZScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(1...100, id: \.self) { row in
                            HStack {
                                Text("Session \(row)")
                                Spacer()
                                Text("\(row)").monospacedDigit().foregroundStyle(theme.foreground.muted().color)
                            }
                            .padding(.horizontal, 12).padding(.vertical, 7)
                            ZZSeparator()
                        }
                    }
                }
                .frame(height: 180)
                .zzSurface()
            }
        }
    }
}
