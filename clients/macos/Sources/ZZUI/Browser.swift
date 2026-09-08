import SwiftUI

public struct ZZBrowserToolbar<Address: View, Actions: View>: View {
    @Environment(\.zzTheme) private var theme
    private let canGoBack: Bool
    private let canGoForward: Bool
    private let loading: Bool
    private let back: () -> Void
    private let forward: () -> Void
    private let reload: () -> Void
    private let address: Address
    private let actions: Actions

    public init(
        canGoBack: Bool = false, canGoForward: Bool = false, loading: Bool = false,
        back: @escaping () -> Void, forward: @escaping () -> Void, reload: @escaping () -> Void,
        @ViewBuilder address: () -> Address, @ViewBuilder actions: () -> Actions
    ) {
        self.canGoBack = canGoBack
        self.canGoForward = canGoForward
        self.loading = loading
        self.back = back
        self.forward = forward
        self.reload = reload
        self.address = address()
        self.actions = actions()
    }

    public var body: some View {
        HStack(spacing: 4) {
            ZZIconButton("Back", systemName: "chevron.left", action: back).disabled(!canGoBack)
            ZZIconButton("Forward", systemName: "chevron.right", action: forward).disabled(!canGoForward)
            ZZIconButton(
                loading ? "Stop loading" : "Reload", systemName: loading ? "xmark" : "arrow.clockwise", action: reload)
            address.frame(maxWidth: .infinity)
            actions
        }
        .padding(.horizontal, 8).frame(height: 40)
        .overlay(alignment: .bottom) { theme.border.color.frame(height: 1) }
    }
}

public struct ZZBrowserAddress: View {
    @Environment(\.zzTheme) private var theme
    @Binding private var address: String
    @FocusState private var focused: Bool
    private let submit: () -> Void

    public init(_ address: Binding<String>, submit: @escaping () -> Void) {
        _address = address
        self.submit = submit
    }

    public var body: some View {
        TextField("Search or enter address", text: $address).textFieldStyle(.plain)
            .font(theme.font(size: 12)).focused($focused).autocorrectionDisabled()
            .onSubmit(submit).padding(.horizontal, 8).frame(height: 24)
            .background(theme.background.washed(1).color, in: ZZRoundedRectangle(radius: theme.radius))
            .zzControlSurface(focused: focused).accessibilityLabel("Address")
    }
}

public struct ZZBrowserTab: Identifiable, Equatable, Sendable {
    public let id: String
    public var title: String
    public var detail: String

    public init(_ id: String, title: String, detail: String = "") {
        self.id = id
        self.title = title
        self.detail = detail
    }
}

public struct ZZBrowserTabStrip: View {
    @Binding private var selection: String
    @Binding private var address: String
    private let tabs: [ZZBrowserTab]
    private let submit: () -> Void
    private let close: (String) -> Void
    private let newTab: () -> Void
    private let focusRequest: Int
    private let blurRequest: Int
    private let focusChanged: (Bool) -> Void
    private let addressChanged: (String) -> Void
    private let moveSelection: (Int) -> Bool
    private let removeSelection: () -> Bool
    private let cancel: () -> Void

    public init(
        tabs: [ZZBrowserTab], selection: Binding<String>, address: Binding<String>, focusRequest: Int = 0,
        blurRequest: Int = 0,
        focusChanged: @escaping (Bool) -> Void = { _ in }, addressChanged: @escaping (String) -> Void = { _ in },
        moveSelection: @escaping (Int) -> Bool = { _ in false }, removeSelection: @escaping () -> Bool = { false },
        cancel: @escaping () -> Void = {},
        submit: @escaping () -> Void, close: @escaping (String) -> Void, newTab: @escaping () -> Void
    ) {
        self.tabs = tabs
        _selection = selection
        _address = address
        self.submit = submit
        self.close = close
        self.newTab = newTab
        self.focusRequest = focusRequest
        self.blurRequest = blurRequest
        self.focusChanged = focusChanged
        self.addressChanged = addressChanged
        self.moveSelection = moveSelection
        self.removeSelection = removeSelection
        self.cancel = cancel
    }

    public var body: some View {
        HStack(spacing: 4) {
            ForEach(tabs) { tab in
                ZZBrowserTabItem(
                    tab: tab, selected: tab.id == selection, closable: tabs.count > 1,
                    focusRequest: focusRequest, blurRequest: blurRequest,
                    address: $address, submit: submit, activate: { selection = tab.id },
                    close: { close(tab.id) }, focusChanged: focusChanged, addressChanged: addressChanged,
                    moveSelection: moveSelection, removeSelection: removeSelection, cancel: cancel)
            }
            if tabs.isEmpty { ZZBrowserAddress($address, submit: submit) }
            ZZIconButton("New tab", systemName: "plus", action: newTab)
        }.padding(.horizontal, 2)
    }
}

private struct ZZBrowserTabItem: View {
    @Environment(\.zzTheme) private var theme
    @State private var hovered = false
    @FocusState private var addressFocused: Bool
    let tab: ZZBrowserTab
    let selected: Bool
    let closable: Bool
    let focusRequest: Int
    let blurRequest: Int
    @Binding var address: String
    let submit: () -> Void
    let activate: () -> Void
    let close: () -> Void
    let focusChanged: (Bool) -> Void
    let addressChanged: (String) -> Void
    let moveSelection: (Int) -> Bool
    let removeSelection: () -> Bool
    let cancel: () -> Void

    var body: some View {
        HStack(spacing: 2) {
            if selected {
                TextField("Search or enter address", text: $address).textFieldStyle(.plain)
                    .focused($addressFocused)
                    .font(theme.font(size: 12)).onSubmit {
                        submit(); addressFocused = false
                    }.autocorrectionDisabled()
                    .onKeyPress(.downArrow) { moveSelection(1) ? .handled : .ignored }
                    .onKeyPress(.upArrow) { moveSelection(-1) ? .handled : .ignored }
                    .onKeyPress(keys: [.delete]) { key in
                        key.modifiers.contains(.shift) && removeSelection() ? .handled : .ignored
                    }
                    .onKeyPress(.escape) {
                        cancel(); addressFocused = false; return .handled
                    }
                    .accessibilityLabel("Address")
            } else {
                Button(action: activate) {
                    Text(tab.title).font(theme.font(size: 12)).lineLimit(1)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                }.buttonStyle(.plain)
            }
            if closable {
                ZZButton("", icon: "xmark", variant: .text, size: .xSmall, flat: true, action: close)
                    .frame(width: 24, height: 24).opacity(hovered ? 1 : 0)
                    .accessibilityLabel("Close \(tab.title)")
            }
        }
        .padding(.leading, 10).frame(minWidth: 40, maxWidth: .infinity).frame(height: 24)
        .background(
            theme.background.washed(hovered ? 2 : 1).color,
            in: ZZRoundedRectangle(radius: theme.radius)
        )
        .zzControlSurface().onHover { hovered = $0 }.help(tab.detail.isEmpty ? tab.title : tab.detail)
        .accessibilityAddTraits(selected ? [.isSelected] : [])
        .onChange(of: focusRequest) { if selected { addressFocused = true } }
        .onChange(of: blurRequest) { if selected { addressFocused = false } }
        .onChange(of: addressFocused) { if selected { focusChanged(addressFocused) } }
        .onChange(of: address) { if selected && addressFocused { addressChanged(address) } }
    }
}

public struct ZZBrowserRecentRow: View {
    @Environment(\.zzTheme) private var theme
    @State private var hovered = false
    private let url: String
    private let action: () -> Void

    public init(_ url: String, action: @escaping () -> Void) {
        self.url = url
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Image(systemName: "globe").foregroundStyle(theme.foreground.muted().color)
                Text(url).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
            }
            .font(theme.font(size: 12, weight: .medium)).padding(.horizontal, 12).frame(height: 32)
            .background(
                theme.background.washed(hovered ? 2 : 1).color,
                in: ZZRoundedRectangle(radius: theme.radius))
        }.buttonStyle(.plain).onHover { hovered = $0 }
    }
}

public struct ZZBrowserStart<Content: View>: View {
    @Environment(\.zzTheme) private var theme
    private let showsHint: Bool
    private let content: Content

    public init(showsHint: Bool = true, @ViewBuilder content: () -> Content) {
        self.showsHint = showsHint
        self.content = content()
    }

    public var body: some View {
        VStack(spacing: 0) {
            if showsHint {
                VStack(spacing: 10) {
                    Image(systemName: "globe").font(.system(size: 14)).foregroundStyle(theme.foreground.muted().color)
                        .frame(width: 32, height: 32).background(theme.background.washed(2).color, in: Circle())
                    Text("Where to?").font(theme.font(size: 13, weight: .semibold))
                    Text("Type a URL to get started. Pages you visit will show up here.")
                        .font(theme.font(size: 11)).foregroundStyle(theme.foreground.muted().color)
                        .multilineTextAlignment(.center)
                }.padding(20).frame(maxWidth: 440)
            }
            VStack(spacing: 4) { content }.frame(maxWidth: 360)
        }
        .padding(.horizontal, 12).frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

extension ZZBrowserStart where Content == EmptyView {
    public init(showsHint: Bool = true) { self.init(showsHint: showsHint) { EmptyView() } }
}

public struct ZZBrowserOmniboxRow: View {
    @Environment(\.zzTheme) private var theme
    @State private var hovered = false
    private let title: String
    private let url: String
    private let selected: Bool
    private let action: () -> Void

    public init(_ title: String, url: String, selected: Bool = false, action: @escaping () -> Void) {
        self.title = title
        self.url = url
        self.selected = selected
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Image(systemName: "globe").foregroundStyle(theme.foreground.muted().color)
                VStack(alignment: .leading, spacing: 1) {
                    if !title.isEmpty { Text(title).font(theme.font(size: 12, weight: .medium)).lineLimit(1) }
                    Text(url).font(theme.font(size: 11)).foregroundStyle(theme.foreground.muted().color).lineLimit(1)
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(.horizontal, 12).frame(height: 44)
            .background(selected || hovered ? theme.background.washed(2).color : .clear)
        }
        .buttonStyle(.plain).onHover { hovered = $0 }.accessibilityAddTraits(selected ? [.isSelected] : [])
    }
}

public struct ZZBrowserOmnibox<Content: View>: View {
    private let content: Content
    public init(@ViewBuilder content: () -> Content) { self.content = content() }
    public var body: some View { VStack(spacing: 0) { content }.padding(.vertical, 4).zzSurface(elevation: 2) }
}

public struct ZZBrowserError: View {
    @Environment(\.zzTheme) private var theme
    private let message: String
    private let retry: () -> Void

    public init(_ message: String, retry: @escaping () -> Void) {
        self.message = message
        self.retry = retry
    }

    public var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "exclamationmark.triangle").font(.system(size: 24)).foregroundStyle(theme.warning.color)
            Text("Couldn’t load this page").font(theme.font(size: 13, weight: .semibold))
            Text(message).font(theme.font(size: 12)).foregroundStyle(theme.foreground.muted().color)
                .multilineTextAlignment(.center).textSelection(.enabled)
            ZZButton("Try again", icon: "arrow.clockwise", action: retry)
        }.padding(20).frame(maxWidth: 440).frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

public struct ZZBrowserPickStatus: View {
    @Environment(\.zzTheme) private var theme
    private let message: String
    private let cancel: () -> Void
    public init(_ message: String, cancel: @escaping () -> Void) {
        self.message = message
        self.cancel = cancel
    }
    public var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "cursorarrow.rays")
            Text(message).font(theme.font(size: 12))
            ZZIconButton("Cancel element selection", systemName: "xmark", action: cancel)
        }.padding(8).zzSurface()
    }
}

public struct ZZBrowserActionMenu<Content: View>: View {
    private let content: Content
    public init(@ViewBuilder content: () -> Content) { self.content = content() }
    public var body: some View {
        Menu {
            content
        } label: {
            Image(systemName: "ellipsis").frame(width: 24, height: 24)
        }
        .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize().help("Browser actions")
    }
}
