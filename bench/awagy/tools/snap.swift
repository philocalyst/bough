// snap <html> <out.png> <width> : render a page offscreen with WebKit and save a full-height snapshot
import AppKit
import WebKit
let a = CommandLine.arguments
let width = CGFloat(Double(a[3])!)
let app = NSApplication.shared
class D: NSObject, WKNavigationDelegate {
    func webView(_ w: WKWebView, didFinish _: WKNavigation!) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
            w.evaluateJavaScript("document.documentElement.scrollHeight") { h, _ in
                let hh = a.count > 4 ? Double(a[4])! : ((h as? Double) ?? 3000)
                w.frame = NSRect(x: 0, y: 0, width: width, height: CGFloat(hh))
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
                    let c = WKSnapshotConfiguration(); c.rect = w.bounds
                    w.takeSnapshot(with: c) { img, _ in
                        if let img = img, let t = img.tiffRepresentation, let r = NSBitmapImageRep(data: t), let png = r.representation(using: .png, properties: [:]) { try? png.write(to: URL(fileURLWithPath: a[2])) }
                        exit(0)
                    }
                }
            }
        }
    }
}
let d = D()
let w = WKWebView(frame: NSRect(x: 0, y: 0, width: width, height: 1200))
w.navigationDelegate = d
let url = URL(fileURLWithPath: a[1])
w.loadFileURL(url, allowingReadAccessTo: url.deletingLastPathComponent())
DispatchQueue.main.asyncAfter(deadline: .now() + 20) { exit(1) }
app.run()
