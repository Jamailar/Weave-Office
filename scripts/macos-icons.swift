// Approved original raster artwork for the app; code-drawn document icons.
// Each iconset size is rendered directly from its master; no fonts are used.
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

struct Ink {
    let r: CGFloat
    let g: CGFloat
    let b: CGFloat
    func color(_ alpha: CGFloat = 1) -> CGColor {
        CGColor(red: r, green: g, blue: b, alpha: alpha)
    }
}

let blue = Ink(r: 0.15, g: 0.41, b: 0.86)
let green = Ink(r: 0.12, g: 0.60, b: 0.43)
let orange = Ink(r: 0.96, g: 0.53, b: 0.23)
let red = Ink(r: 0.83, g: 0.28, b: 0.32)
let pale = Ink(r: 0.96, g: 0.97, b: 0.99)
let dark = Ink(r: 0.20, g: 0.27, b: 0.36)

func polygon(_ ctx: CGContext, _ points: [CGPoint], _ color: CGColor) {
    ctx.beginPath()
    ctx.move(to: points[0])
    for point in points.dropFirst() { ctx.addLine(to: point) }
    ctx.closePath()
    ctx.setFillColor(color)
    ctx.fillPath()
}

func rounded(_ ctx: CGContext, _ rect: CGRect, _ radius: CGFloat, _ color: CGColor) {
    ctx.addPath(CGPath(roundedRect: rect, cornerWidth: radius, cornerHeight: radius, transform: nil))
    ctx.setFillColor(color)
    ctx.fillPath()
}

func line(_ ctx: CGContext, _ a: CGPoint, _ b: CGPoint, _ width: CGFloat, _ color: CGColor) {
    ctx.setStrokeColor(color)
    ctx.setLineWidth(width)
    ctx.setLineCap(.round)
    ctx.move(to: a)
    ctx.addLine(to: b)
    ctx.strokePath()
}

func appIcon(_ ctx: CGContext, _ image: CGImage) {
    // Standard icon tile clipping removes the presentation backdrop while preserving
    // the approved raster artwork. The outside of the macOS tile is transparent.
    ctx.saveGState()
    ctx.addPath(CGPath(roundedRect: CGRect(x: 65, y: 65, width: 894, height: 894),
                       cornerWidth: 220, cornerHeight: 220, transform: nil))
    ctx.clip()
    ctx.interpolationQuality = .high
    ctx.draw(image, in: CGRect(x: 0, y: 0, width: 1024, height: 1024))
    ctx.restoreGState()
}

func documentIcon(_ ctx: CGContext, _ kind: String) {
    let accent: Ink = ["word": blue, "sheet": green, "slides": orange, "pdf": red][kind]!
    let shape = [CGPoint(x: 156, y: 88), CGPoint(x: 156, y: 928), CGPoint(x: 665, y: 928), CGPoint(x: 866, y: 727), CGPoint(x: 866, y: 88)]
    // Subtle border and white paper stay visible on light and dark Finder backgrounds.
    polygon(ctx, shape, dark.color(0.16))
    polygon(ctx, [CGPoint(x: 166, y: 98), CGPoint(x: 166, y: 918), CGPoint(x: 659, y: 918), CGPoint(x: 856, y: 721), CGPoint(x: 856, y: 98)], pale.color())
    polygon(ctx, [CGPoint(x: 659, y: 918), CGPoint(x: 856, y: 721), CGPoint(x: 659, y: 721)], accent.color(0.25))
    rounded(ctx, CGRect(x: 166, y: 98, width: 690, height: 115), 0, accent.color())
    switch kind {
    case "word":
        for (y, width) in [(603.0, 470.0), (526.0, 510.0), (449.0, 450.0), (372.0, 500.0)] {
            rounded(ctx, CGRect(x: 250, y: y, width: width, height: 30), 15, accent.color(y == 603 ? 0.90 : 0.40))
        }
    case "sheet":
        rounded(ctx, CGRect(x: 248, y: 327, width: 528, height: 349), 16, accent.color(0.12))
        for x in [248.0, 424.0, 600.0, 776.0] {
            line(ctx, CGPoint(x: x, y: 327), CGPoint(x: x, y: 676), 15, accent.color(0.7))
        }
        for y in [327.0, 414.0, 501.0, 588.0, 676.0] {
            line(ctx, CGPoint(x: 248, y: y), CGPoint(x: 776, y: y), 15, accent.color(0.7))
        }
        rounded(ctx, CGRect(x: 265, y: 604, width: 494, height: 56), 4, accent.color(0.74))
    case "slides":
        rounded(ctx, CGRect(x: 242, y: 340, width: 542, height: 339), 25, accent.color(0.17))
        rounded(ctx, CGRect(x: 270, y: 482, width: 245, height: 144), 15, accent.color(0.86))
        rounded(ctx, CGRect(x: 548, y: 589, width: 174, height: 24), 12, accent.color(0.73))
        rounded(ctx, CGRect(x: 548, y: 532, width: 198, height: 24), 12, accent.color(0.45))
        rounded(ctx, CGRect(x: 270, y: 389, width: 461, height: 26), 13, accent.color(0.43))
    default: // PDF: a continuous abstract ribbon, no licensed word mark.
        ctx.setStrokeColor(accent.color())
        ctx.setLineWidth(43)
        ctx.setLineCap(.round)
        ctx.setLineJoin(.round)
        ctx.move(to: CGPoint(x: 284, y: 378))
        ctx.addCurve(to: CGPoint(x: 565, y: 602), control1: CGPoint(x: 426, y: 409), control2: CGPoint(x: 515, y: 755))
        ctx.addCurve(to: CGPoint(x: 405, y: 319), control1: CGPoint(x: 615, y: 471), control2: CGPoint(x: 361, y: 241))
        ctx.addCurve(to: CGPoint(x: 726, y: 393), control1: CGPoint(x: 469, y: 361), control2: CGPoint(x: 612, y: 419))
        ctx.strokePath()
    }
}

func render(_ kind: String, _ pixels: Int, _ url: URL, _ appImage: CGImage?) throws {
    let space = CGColorSpaceCreateDeviceRGB()
    guard let ctx = CGContext(data: nil, width: pixels, height: pixels, bitsPerComponent: 8,
                              bytesPerRow: 0, space: space,
                              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
        throw NSError(domain: "WeaveIcons", code: 1)
    }
    ctx.scaleBy(x: CGFloat(pixels) / 1024, y: CGFloat(pixels) / 1024)
    if kind == "app" {
        guard let image = appImage else { throw NSError(domain: "WeaveIcons", code: 4) }
        appIcon(ctx, image)
    } else { documentIcon(ctx, kind) }
    guard let image = ctx.makeImage(), let dest = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil) else {
        throw NSError(domain: "WeaveIcons", code: 2)
    }
    CGImageDestinationAddImage(dest, image, nil)
    guard CGImageDestinationFinalize(dest) else { throw NSError(domain: "WeaveIcons", code: 3) }
}

guard (3...4).contains(CommandLine.arguments.count) else {
    fputs("usage: macos-icons <kind> <iconset-directory> [app-master.png]\n", stderr)
    exit(2)
}
let kind = CommandLine.arguments[1]
guard ["app", "word", "sheet", "slides", "pdf"].contains(kind) else { exit(2) }
var appImage: CGImage? = nil
if kind == "app" {
    guard CommandLine.arguments.count == 4,
          let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: CommandLine.arguments[3]) as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { exit(2) }
    appImage = image
}
let directory = URL(fileURLWithPath: CommandLine.arguments[2], isDirectory: true)
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
for size in [16, 32, 128, 256, 512] {
    try render(kind, size, directory.appendingPathComponent("icon_\(size)x\(size).png"), appImage)
    try render(kind, size * 2, directory.appendingPathComponent("icon_\(size)x\(size)@2x.png"), appImage)
}
