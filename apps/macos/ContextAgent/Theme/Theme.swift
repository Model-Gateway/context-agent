import SwiftUI
import AppKit

/// Design tokens (ARCHITECTURE.md "Design system"). Every colour is built
/// from a light/dark pair, so the app follows the effective appearance
/// (system, or the Appearance setting applied through `NSApp.appearance`).
enum Theme {
    // MARK: Surfaces

    /// The light/dark hex pairs that other code needs as raw values (the
    /// Appearance swatches draw a fixed light or dark preview).
    /// The window, sidebar and cards share one background (`surface`);
    /// panes and cards are told apart by hairlines and shadows only.
    enum Hex {
        static let surfaceLight = "#FFFFFF", surfaceDark = "#1E1E20"
        static let canvasLight = surfaceLight, canvasDark = surfaceDark
        static let sidebarLight = surfaceLight, sidebarDark = surfaceDark
        static let surfaceRaisedLight = "#F2F2F4", surfaceRaisedDark = "#2B2B2E"
        static let borderLight = "#E7E7EA", borderDark = "#343438"
        static let textPrimaryLight = "#1A1A1C", textPrimaryDark = "#F3F3F5"
        static let textSecondaryLight = "#6B6F76", textSecondaryDark = "#A3A6AD"
        static let textTertiaryLight = "#9A9EA6", textTertiaryDark = "#6F7278"
        static let accentLight = "#2F7BF6", accentDark = "#5B9BFF"
        static let successLight = "#1A9E57", successDark = "#3DD68C"
        static let dangerLight = "#D93A3A", dangerDark = "#FF6B6B"
    }

    /// Window / content background (the same colour as `surface`).
    static let canvas = Color.adaptive(light: Hex.canvasLight, dark: Hex.canvasDark)
    /// Cards, composer, popovers.
    static let surface = Color.adaptive(light: Hex.surfaceLight, dark: Hex.surfaceDark)
    /// Hover, selected rows, chips.
    static let surfaceRaised = Color.adaptive(light: Hex.surfaceRaisedLight, dark: Hex.surfaceRaisedDark)
    /// Sidebar background (the same colour as `surface`).
    static let sidebar = Color.adaptive(light: Hex.sidebarLight, dark: Hex.sidebarDark)
    /// Hairlines.
    static let border = Color.adaptive(light: Hex.borderLight, dark: Hex.borderDark)
    /// Focused inputs.
    static let borderStrong = Color.adaptive(light: "#D8D8DC", dark: "#45454A")

    // MARK: Text

    static let textPrimary = Color.adaptive(light: Hex.textPrimaryLight, dark: Hex.textPrimaryDark)
    /// AppKit twin of `textPrimary` for `NSTextView`s (composer, full-output sheet).
    static var nsTextPrimary: NSColor { NSColor.adaptive(light: Hex.textPrimaryLight, dark: Hex.textPrimaryDark) }
    static let textSecondary = Color.adaptive(light: Hex.textSecondaryLight, dark: Hex.textSecondaryDark)
    /// Placeholders, timestamps.
    static let textTertiary = Color.adaptive(light: Hex.textTertiaryLight, dark: Hex.textTertiaryDark)

    /// AppKit twins for the AppKit-drawn views (diff text view, editor gutter).
    static var nsSurface: NSColor { NSColor.adaptive(light: Hex.surfaceLight, dark: Hex.surfaceDark) }
    static var nsSurfaceRaised: NSColor { NSColor.adaptive(light: Hex.surfaceRaisedLight, dark: Hex.surfaceRaisedDark) }
    static var nsBorder: NSColor { NSColor.adaptive(light: Hex.borderLight, dark: Hex.borderDark) }
    static var nsTextSecondary: NSColor { NSColor.adaptive(light: Hex.textSecondaryLight, dark: Hex.textSecondaryDark) }
    static var nsTextTertiary: NSColor { NSColor.adaptive(light: Hex.textTertiaryLight, dark: Hex.textTertiaryDark) }
    static var nsAccent: NSColor { NSColor.adaptive(light: Hex.accentLight, dark: Hex.accentDark) }
    static var nsSuccess: NSColor { NSColor.adaptive(light: Hex.successLight, dark: Hex.successDark) }
    static var nsDanger: NSColor { NSColor.adaptive(light: Hex.dangerLight, dark: Hex.dangerDark) }

    // MARK: Accent and semantic colours

    /// Send, links, selection ring.
    static let accent = Color.adaptive(light: Hex.accentLight, dark: Hex.accentDark)
    /// Send button background.
    static let accentSoft = Color.adaptive(light: "#DCEBFD", dark: "#1E3A5F")
    /// The Ultra effort tier (effort chip and popover).
    static let ultra = Color.adaptive(light: "#9B5DE5", dark: "#B98CFF")
    static let success = Color.adaptive(light: Hex.successLight, dark: Hex.successDark)
    static let warning = Color.adaptive(light: "#D98806", dark: "#F5B544")
    static let danger = Color.adaptive(light: Hex.dangerLight, dark: Hex.dangerDark)

    // MARK: Code

    /// Code blocks, tool output.
    static let codeBackground = Color.adaptive(light: "#F6F6F7", dark: "#1E1E21")
    static let codeString = Color.adaptive(light: "#1B8A3D", dark: "#7EE2A8")
    static let codeKeyword = Color.adaptive(light: "#6F42C1", dark: "#C3A6FF")
    static let codeNumber = Color.adaptive(light: "#B45309", dark: "#F0B35E")
    static let codeComment = Color.adaptive(light: "#8A8F98", dark: "#7A7F88")
    /// Types and functions, which only the editor's tree-sitter highlighting tells apart.
    static let codeType = Color.adaptive(light: "#0E7490", dark: "#7DD3E8")
    static let codeFunction = Color.adaptive(light: "#2457C5", dark: "#8AB4FF")

    // MARK: Badges

    static let badgeBackground = Color.adaptive(light: "#FDE7EC", dark: "#4A1F2B")
    static let badgeText = Color.adaptive(light: "#E0245E", dark: "#FF7A9A")

    // MARK: Typography

    enum Font {
        /// 22 semibold — hero titles ("Where should we begin?").
        static let title: SwiftUI.Font = .system(size: 22, weight: .semibold)
        /// 15 semibold — card and section headings.
        static let heading: SwiftUI.Font = .system(size: 15, weight: .semibold)
        /// 14 regular — transcript text, composer.
        static let body: SwiftUI.Font = .system(size: 14, weight: .regular)
        /// 13 — secondary lines, chips, meta text.
        static let secondary: SwiftUI.Font = .system(size: 13)
        /// 12 — captions, timestamps, section headers.
        static let caption: SwiftUI.Font = .system(size: 12)
        /// 12.5 monospaced — code, tool output, paths.
        static let mono: SwiftUI.Font = .system(size: 12.5, design: .monospaced)
        /// 13.5 — sidebar rows.
        static let sidebar: SwiftUI.Font = .system(size: 13.5)

        /// 11.5 monospaced — dense logs (Inspector Terminal tab).
        static let monoSmall: SwiftUI.Font = .system(size: 11.5, design: .monospaced)
        /// AppKit twin of `mono`, for `NSTextView`-based views.
        static var nsMono: NSFont { .monospacedSystemFont(ofSize: 12.5, weight: .regular) }
    }

    // MARK: Metrics

    enum Metrics {
        static let sidebarWidth: CGFloat = 248
        /// Default inspector width; dragging its leading edge resizes it
        /// between `inspectorMinWidth` and `inspectorMaxWidth`.
        static let inspectorWidth: CGFloat = 320
        static let inspectorMinWidth: CGFloat = 260
        static let inspectorMaxWidth: CGFloat = 720
        static let contentMaxWidth: CGFloat = 760
        static let radiusSmall: CGFloat = 8
        static let radius: CGFloat = 12
        static let radiusLarge: CGFloat = 16

        /// Spacing scale 4 / 8 / 12 / 16 / 24 / 32.
        static let spacingXS: CGFloat = 4
        static let spacingS: CGFloat = 8
        static let spacingM: CGFloat = 12
        static let spacingL: CGFloat = 16
        static let spacingXL: CGFloat = 24
        static let spacingXXL: CGFloat = 32
    }
}

// MARK: - Adaptive colours

extension Color {
    /// A colour that resolves to `light` under light appearances and `dark`
    /// under dark ones (including the high-contrast variants). Hex strings
    /// take the form `#RRGGBB` or `RRGGBB`; an unparsable string falls back
    /// to the label colour so a typo never produces an invisible element.
    static func adaptive(light: String, dark: String) -> Color {
        Color(nsColor: NSColor.adaptive(light: light, dark: dark))
    }
}

extension NSColor {
    /// The dynamic `NSColor` behind `Color.adaptive`.
    static func adaptive(light: String, dark: String) -> NSColor {
        let lightColor = NSColor(hexString: light) ?? NSColor.labelColor
        let darkColor = NSColor(hexString: dark) ?? lightColor
        return NSColor(name: nil) { appearance in
            let match = appearance.bestMatch(from: [.aqua, .darkAqua])
            return match == .darkAqua ? darkColor : lightColor
        }
    }

    /// Parses `#RRGGBB`, `RRGGBB`, `#RRGGBBAA` or `RRGGBBAA` (sRGB).
    convenience init?(hexString: String) {
        var text = hexString.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.hasPrefix("#") { text.removeFirst() }
        guard text.count == 6 || text.count == 8, let value = UInt64(text, radix: 16) else { return nil }
        let hasAlpha = text.count == 8
        let r = CGFloat((value >> (hasAlpha ? 24 : 16)) & 0xFF) / 255
        let g = CGFloat((value >> (hasAlpha ? 16 : 8)) & 0xFF) / 255
        let b = CGFloat((value >> (hasAlpha ? 8 : 0)) & 0xFF) / 255
        let a = hasAlpha ? CGFloat(value & 0xFF) / 255 : 1
        self.init(srgbRed: r, green: g, blue: b, alpha: a)
    }
}

#Preview("Theme tokens") {
    let swatches: [(String, Color)] = [
        ("canvas", Theme.canvas), ("surface", Theme.surface), ("surfaceRaised", Theme.surfaceRaised),
        ("sidebar", Theme.sidebar), ("border", Theme.border), ("borderStrong", Theme.borderStrong),
        ("textPrimary", Theme.textPrimary), ("textSecondary", Theme.textSecondary), ("textTertiary", Theme.textTertiary),
        ("accent", Theme.accent), ("accentSoft", Theme.accentSoft), ("success", Theme.success),
        ("warning", Theme.warning), ("danger", Theme.danger), ("codeBackground", Theme.codeBackground),
        ("codeString", Theme.codeString), ("codeKeyword", Theme.codeKeyword), ("codeNumber", Theme.codeNumber),
        ("codeComment", Theme.codeComment), ("badgeBackground", Theme.badgeBackground), ("badgeText", Theme.badgeText),
    ]
    return VStack(alignment: .leading, spacing: 6) {
        ForEach(swatches, id: \.0) { name, color in
            HStack(spacing: 10) {
                RoundedRectangle(cornerRadius: 4).fill(color).frame(width: 36, height: 18)
                    .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(Theme.border))
                Text(name).font(Theme.Font.mono).foregroundStyle(Theme.textPrimary)
            }
        }
        Divider()
        Text("Title 22 semibold").font(Theme.Font.title)
        Text("Heading 15 semibold").font(Theme.Font.heading)
        Text("Body 14 regular").font(Theme.Font.body)
        Text("Secondary 13").font(Theme.Font.secondary).foregroundStyle(Theme.textSecondary)
        Text("Caption 12").font(Theme.Font.caption).foregroundStyle(Theme.textTertiary)
        Text("mono 12.5 `let x = 1`").font(Theme.Font.mono)
    }
    .padding(24)
    .background(Theme.canvas)
}
