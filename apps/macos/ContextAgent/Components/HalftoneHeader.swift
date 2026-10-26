import SwiftUI

/// Decorative halftone band for the top of the empty chat: clusters of tiny
/// dots that thin out towards the bottom and the sides (reference-home.png).
/// Deterministic (hash-based), so it never flickers on re-render.
struct HalftoneHeader: View {
    var height: CGFloat

    init(height: CGFloat = 120) {
        self.height = height
    }

    var body: some View {
        Canvas(opaque: false, rendersAsynchronously: false) { context, size in
            guard size.width > 0, size.height > 0 else { return }
            let spacing: CGFloat = 6
            let columns = Int(size.width / spacing) + 1
            let rows = Int(size.height / spacing) + 1
            let halfWidth = size.width / 2

            for row in 0..<rows {
                let y = CGFloat(row) * spacing + spacing / 2
                let vertical = max(0, 1 - y / size.height)
                let verticalFade = vertical * vertical
                if verticalFade < 0.015 { continue }

                for column in 0..<columns {
                    let x = CGFloat(column) * spacing + spacing / 2
                    let distance = abs(x - halfWidth) / halfWidth
                    let horizontalFade = max(0, 1 - distance * distance * 1.1)
                    if horizontalFade <= 0 { continue }

                    // Low-frequency noise shapes the clusters, high-frequency noise picks dots.
                    let cluster = Self.noise(column / 6, row / 3, seed: 7)
                    let pick = Self.noise(column, row, seed: 1)
                    let density = verticalFade * horizontalFade * (0.35 + 0.65 * cluster)
                    guard pick < density * 0.9 else { continue }

                    let radius = 0.7 + 1.3 * Self.noise(column, row, seed: 3)
                    let alpha = 0.05 + 0.17 * density
                    let rect = CGRect(x: x - radius, y: y - radius, width: radius * 2, height: radius * 2)
                    context.fill(Path(ellipseIn: rect), with: .color(Theme.textPrimary.opacity(alpha)))
                }
            }
        }
        .frame(height: height)
        .frame(maxWidth: .infinity)
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    /// Stable pseudo-random value in 0..<1 for a grid cell.
    private static func noise(_ x: Int, _ y: Int, seed: UInt32) -> Double {
        var h = UInt32(truncatingIfNeeded: x) &* 374_761_393
        h = h &+ UInt32(truncatingIfNeeded: y) &* 668_265_263
        h = h &+ seed &* 2_246_822_519
        h = (h ^ (h >> 13)) &* 1_274_126_177
        h ^= h >> 16
        return Double(h & 0xFFFF) / 65_535
    }
}

#Preview("Halftone header") {
    VStack(spacing: 0) {
        HalftoneHeader()
        Spacer()
        Icons.LogoView()
        Text("Where should we begin?").font(Theme.Font.title).foregroundStyle(Theme.textPrimary)
        Spacer()
    }
    .frame(width: 900, height: 400)
    .background(Theme.canvas)
}
