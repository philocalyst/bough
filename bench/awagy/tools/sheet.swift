// contact sheet: sheet <out.png> <cols> <cell_w> <png>...
import Foundation
import CoreGraphics
import CoreText
import ImageIO
import UniformTypeIdentifiers
let a = CommandLine.arguments
let cols = Int(a[2])!, cw = Int(a[3])!, ch = cw * 7 / 10
let files = Array(a[4...]); let rows = (files.count + cols - 1) / cols
let ctx = CGContext(data: nil, width: cols * cw, height: rows * (ch + 24), bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
ctx.setFillColor(CGColor(red: 0.15, green: 0.15, blue: 0.15, alpha: 1)); ctx.fill(CGRect(x: 0, y: 0, width: cols * cw, height: rows * (ch + 24)))
for (i, f) in files.enumerated() {
    guard let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: f) as CFURL, nil), let img = CGImageSourceCreateImageAtIndex(src, 0, nil) else { continue }
    let (c, r) = (i % cols, i / cols)
    let y = (rows - 1 - r) * (ch + 24)
    ctx.draw(img, in: CGRect(x: c * cw, y: y, width: cw, height: ch))
    let label = (f as NSString).lastPathComponent.replacingOccurrences(of: ".png", with: "") as CFString
    let line = CTLineCreateWithAttributedString(CFAttributedStringCreate(nil, label, [kCTForegroundColorAttributeName: CGColor(red: 1, green: 1, blue: 1, alpha: 1), kCTFontAttributeName: CTFontCreateWithName("Helvetica" as CFString, 15, nil)] as CFDictionary))
    ctx.textPosition = CGPoint(x: c * cw + 6, y: y + ch + 6); CTLineDraw(line, ctx)
}
let dst = CGImageDestinationCreateWithURL(URL(fileURLWithPath: a[1]) as CFURL, UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(dst, ctx.makeImage()!, nil); CGImageDestinationFinalize(dst)
