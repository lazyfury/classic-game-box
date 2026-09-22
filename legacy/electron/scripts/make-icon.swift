// ---------------------------------------------------------------------------
// make-icon.swift -- draw a square PNG as a macOS application icon.
//
// Called by scripts/make-icon.mjs with the source picture and an `.iconset`
// directory to fill. It is Swift rather than JavaScript for one reason: the
// macOS icon is not a resize. Big Sur and later draw every app inside a
// transparent canvas as a rounded square (a "squircle") with a soft shadow,
// and CoreGraphics is the one image engine that is always present on a Mac.
//
// The grid, in fractions of the 1024pt canvas (Apple's own template):
//
//     1024pt canvas
//     +----------------------------------+
//     |                                  |
//     |      +------------------+        |   body: 824pt across, centred
//     |      |                  |        |   margin: 100pt
//     |      |      artwork     |        |   corner radius: 185.4pt
//     |      +------------------+        |
//     |                                  |
//     +----------------------------------+
//
// Everything below is those three numbers scaled to the size being drawn, so
// the 16px icon is the same shape as the 1024px one rather than a shrink of it.
//
//   swift scripts/make-icon.swift build/icon-source.png build/icon.iconset
// ---------------------------------------------------------------------------

import AppKit
import CoreGraphics
import Foundation
import ImageIO

func die(_ message: String) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(1)
}

let arguments = CommandLine.arguments
guard arguments.count >= 3 else {
    die("usage: make-icon.swift <source.png> <iconset-dir>")
}
let sourcePath = arguments[1]
let iconsetPath = arguments[2]

guard let sourceImage = NSImage(contentsOfFile: sourcePath),
      let source = sourceImage.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
    die("make-icon: cannot read \(sourcePath)")
}

/// Apple's icon grid, as fractions of the canvas.
let bodyFraction: CGFloat = 824.0 / 1024.0
let radiusFraction: CGFloat = 185.4 / 824.0

/// Draw one icon at `size` pixels and return it.
func render(size: Int) -> CGImage {
    let canvas = CGFloat(size)
    let body = canvas * bodyFraction
    let margin = (canvas - body) / 2
    let rect = CGRect(x: margin, y: margin, width: body, height: body)

    let colorSpace = CGColorSpaceCreateDeviceRGB()
    guard let context = CGContext(
        data: nil,
        width: size,
        height: size,
        bitsPerComponent: 8,
        bytesPerRow: 0,
        space: colorSpace,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else {
        die("make-icon: cannot create a \(size)x\(size) context")
    }
    context.interpolationQuality = .high
    context.clear(CGRect(x: 0, y: 0, width: canvas, height: canvas))

    let rounded = CGPath(
        roundedRect: rect,
        cornerWidth: body * radiusFraction,
        cornerHeight: body * radiusFraction,
        transform: nil
    )

    // The soft drop shadow of the template. It is a filled body drawn under
    // the artwork, so only the shadow survives after the artwork covers it.
    context.saveGState()
    context.setShadow(
        offset: CGSize(width: 0, height: -canvas * 0.008),
        blur: canvas * 0.025,
        color: CGColor(red: 0, green: 0, blue: 0, alpha: 0.30)
    )
    context.addPath(rounded)
    context.setFillColor(CGColor(gray: 0, alpha: 1))
    context.fillPath()
    context.restoreGState()

    // Clip to the squircle and draw the picture aspect-filled inside it.
    context.saveGState()
    context.addPath(rounded)
    context.clip()
    let width = CGFloat(source.width)
    let height = CGFloat(source.height)
    let scale = max(body / width, body / height)
    let drawn = CGRect(
        x: margin + (body - width * scale) / 2,
        y: margin + (body - height * scale) / 2,
        width: width * scale,
        height: height * scale
    )
    context.draw(source, in: drawn)
    context.restoreGState()

    guard let image = context.makeImage() else {
        die("make-icon: cannot snapshot the \(size)x\(size) context")
    }
    return image
}

func write(_ image: CGImage, to path: String) {
    let url = URL(fileURLWithPath: path) as CFURL
    guard let destination = CGImageDestinationCreateWithURL(
        url, "public.png" as CFString, 1, nil
    ) else {
        die("make-icon: cannot write \(path)")
    }
    CGImageDestinationAddImage(destination, image, nil)
    guard CGImageDestinationFinalize(destination) else {
        die("make-icon: cannot finish \(path)")
    }
}

// The sizes iconutil expects, and the file names it expects them under.
let entries: [(pixels: Int, names: [String])] = [
    (16,   ["icon_16x16.png"]),
    (32,   ["icon_16x16@2x.png", "icon_32x32.png"]),
    (64,   ["icon_32x32@2x.png"]),
    (128,  ["icon_128x128.png"]),
    (256,  ["icon_128x128@2x.png", "icon_256x256.png"]),
    (512,  ["icon_256x256@2x.png", "icon_512x512.png"]),
    (1024, ["icon_512x512@2x.png"]),
]

try? FileManager.default.removeItem(atPath: iconsetPath)
try! FileManager.default.createDirectory(
    atPath: iconsetPath, withIntermediateDirectories: true
)

for entry in entries {
    let image = render(size: entry.pixels)
    for name in entry.names {
        write(image, to: (iconsetPath as NSString).appendingPathComponent(name))
    }
    print("[icon] \(entry.pixels)x\(entry.pixels)")
}

// A plain 1024 PNG beside the iconset, for tools that want a PNG.
write(
    render(size: 1024),
    to: ((iconsetPath as NSString).deletingLastPathComponent as NSString)
        .appendingPathComponent("icon.png")
)
print("[icon] icon.png (1024)")
