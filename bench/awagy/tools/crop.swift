// crop <in.png> <out.png> <y> <h>  (pixels, from the top), scaled to half width
import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
let a = CommandLine.arguments
let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: a[1]) as CFURL, nil)!
let img = CGImageSourceCreateImageAtIndex(src, 0, nil)!
let y = Int(a[3])!, h = min(Int(a[4])!, img.height - Int(a[3])!)
let c = img.cropping(to: CGRect(x: 0, y: y, width: img.width, height: h))!
let ctx = CGContext(data: nil, width: img.width / 2, height: h / 2, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
ctx.interpolationQuality = .high; ctx.draw(c, in: CGRect(x: 0, y: 0, width: img.width / 2, height: h / 2))
let d = CGImageDestinationCreateWithURL(URL(fileURLWithPath: a[2]) as CFURL, UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(d, ctx.makeImage()!, nil); CGImageDestinationFinalize(d)
