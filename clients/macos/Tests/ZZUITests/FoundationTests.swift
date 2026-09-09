import SwiftUI
import Testing

@testable import ZZUI

@Test func paletteMatchesGPUIRoots() {
    #expect(ZZTheme.light.background.hex == "#ffffff")
    #expect(ZZTheme.light.foreground.hex == "#0a0a0a")
    #expect(ZZTheme.dark.background.hex == "#0a0a0a")
    #expect(ZZTheme.dark.foreground.hex == "#fafafa")
    #expect(ZZTheme.light.scrim.alpha == 0.05)
    #expect(ZZTheme.dark.scrim.alpha == 0.2)
}

@Test func bordersSitBetweenBackgroundAndForeground() {
    for theme in [ZZTheme.light, ZZTheme.dark] {
        let background = theme.background.oklabLightness
        let foreground = theme.foreground.oklabLightness
        let border = theme.border.oklabLightness
        #expect(border > min(background, foreground))
        #expect(border < max(background, foreground))
        #expect(abs((border - background) / (foreground - background) - 0.14) < 0.0001)
        #expect(theme.border.alpha == 1)
        var translucent = theme
        translucent.background = theme.background.opacity(0.5)
        translucent.foreground = theme.foreground.opacity(0.5)
        #expect(translucent.border.alpha == 1)
    }
}

@Test func hexInputRoundTripsAndRejectsMalformedValues() {
    for value in ["#1a1b26", "#ff000080", "#aabbcc"] {
        #expect(ZZColor(hex: value)?.hex == value)
    }
    #expect(ZZColor(hex: " f00 ")?.hex == "#ff0000")
    #expect(ZZColor(hex: "#ABC")?.hex == "#aabbcc")
    for value in ["", "#", "#12", "#12345", "#gggggg", "#1234567", "red", "#１２３"] {
        #expect(ZZColor(hex: value) == nil)
    }
}

@Test func elevationPreservesDirectionAndAlpha() {
    for theme in [ZZTheme.light, ZZTheme.dark] {
        let a = theme.background.raised(1).oklabLightness
        let b = theme.background.raised(2).oklabLightness
        let base = theme.background.oklabLightness
        #expect(abs(a - base) < abs(b - base))
        #expect(abs(b - base) < abs(theme.foreground.oklabLightness - base))
        #expect(theme.background.opacity(0.6).raised(2).alpha == 0.6)
        #expect(theme.background.washed(2).alpha == 0.08)
    }
    #expect(ZZTheme.light.background.raised(1).lightness < ZZTheme.light.background.lightness)
    #expect(ZZTheme.dark.background.raised(1).lightness > ZZTheme.dark.background.lightness)
}

@Test func alphaPremultipliedMixAvoidsDarkFringes() {
    let red = ZZColor(red: 1, green: 0, blue: 0)
    let mixed = red.mix(.clear, weight: 0.2)
    #expect(abs(mixed.red - 1) < 0.0001)
    #expect(mixed.green < 0.0001)
    #expect(mixed.blue < 0.0001)
    #expect(mixed.alpha == 0.2)
    #expect(ZZColor.clear.mix(.clear, weight: 0.5) == .clear)
    #expect(red.opacity(0.5).opacity(0.5).alpha == 0.25)
}

@Test func oklabMixMatchesReferenceColor() {
    let red = ZZColor(red: 1, green: 0, blue: 0)
    let blue = ZZColor(red: 0, green: 0, blue: 1)
    #expect(red.mix(blue, weight: 0.5).hex == "#8c53a2")
}

@Test func compactControlsRetainGPUIDimensions() {
    #expect(ZZControlSize.allCases.map(\.height) == [24, 28, 36, 40])
    #expect(ZZControlSize.allCases.map(\.iconSize) == [12, 14, 16, 24])
    #expect(ZZControlSize.small.fontSize == 13)
    #expect(ZZTheme.light.radius == 6)
}

@Test func adaptiveCornersNeverBecomePills() {
    for height: CGFloat in [24, 28, 36, 40] {
        let size = CGSize(width: 120, height: height)
        let normal = ZZRoundedRectangle.resolvedRadius(6, in: size)
        let large = ZZRoundedRectangle.resolvedRadius(32, in: size)
        #expect(normal > 0 && normal < 6)
        #expect(large > normal && large < height / 2)
        #expect(ZZRoundedRectangle.resolvedRadius(0, in: size) == 0)
    }
    #expect(ZZRoundedRectangle.resolvedRadius(6, in: .zero) == 0)
}
