import SwiftUI
import ZZUI

struct GallerySection<Content: View>: View {
    @Environment(\.zzTheme) private var theme
    let title: String
    let detail: String?
    let content: Content

    init(_ title: String, detail: String? = nil, @ViewBuilder content: () -> Content) {
        self.title = title
        self.detail = detail
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 4) {
                Text(title).font(.system(size: 13, weight: .semibold))
                if let detail {
                    Text(detail).font(.system(size: 12)).foregroundStyle(theme.foreground.muted().color)
                }
            }
            content.frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(18)
        .zzSurface(elevation: 0)
    }
}
