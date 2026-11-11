// The harness's input (apps/scroll.py, apps/dashboard.py) for macOS: OS events posted to one process
// (CGEventPostToPid: the real cursor never moves). Clock: CLOCK_UPTIME_RAW ns, as the present observer's.
//   awagy_input window <pid>                                   -> content rect "x y w h" (global points, top-left)
//   awagy_input scroll <pid> <x> <y> <rate> <seconds>         -> wheel lines, reversing every 0.5 s
//   awagy_input dashboard <pid> <x0> <y0> <w> <h> <rate> <phase_s> <warmup_s>
import Foundation
import CoreGraphics
import AppKit

func now() -> UInt64 { clock_gettime_nsec_np(CLOCK_UPTIME_RAW) }
func sleepUntil(_ t: UInt64) { let n = now(); if t > n { usleep(useconds_t(min(10_000, (t - n) / 1000))) } }
let a = CommandLine.arguments
let pid = pid_t(Int32(a[2])!)

var windowNumber: Int64 = 0
func contentRect(_ pid: pid_t) -> CGRect? {
    guard let info = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] else { return nil }
    for w in info where (w[kCGWindowOwnerPID as String] as? pid_t) == pid && (w[kCGWindowLayer as String] as? Int) == 0 {
        if let b = w[kCGWindowBounds as String] as? NSDictionary, let r = CGRect(dictionaryRepresentation: b), r.width > 200 {
            windowNumber = Int64(w[kCGWindowNumber as String] as? Int ?? 0)
            return r
        }
    }
    return nil
}
// The target window is uncovered: no other on-screen window (except the menu bar and Dock layers' own
// strips outside it) intersects it above it in the front-to-back list.
func uncovered(_ pid: pid_t) -> Bool {
    guard let info = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]], let mine = contentRect(pid) else { return false }
    for w in info {
        if (w[kCGWindowOwnerPID as String] as? pid_t) == pid && (w[kCGWindowLayer as String] as? Int) == 0 { return true }
        guard let b = w[kCGWindowBounds as String] as? NSDictionary, let r = CGRect(dictionaryRepresentation: b) else { continue }
        let alpha = w[kCGWindowAlpha as String] as? Double ?? 1
        if alpha > 0.01 && r.intersects(mine.insetBy(dx: 1, dy: 1)) && r.width > 2 && r.height > 2 { return false }
    }
    return false
}
// AppKit routes an event posted to a process by the window number it carries.
func target(_ e: CGEvent) -> CGEvent {
    e.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: windowNumber)
    e.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent, value: windowNumber)
    return e
}
// Wheel events are real HID input too, routed by the window server to the window under the pointer (which the
// driver has placed over the app's window): a wheel event posted to a process carries no window, and AppKit
// does not dispatch it to any view (toolkits that read scroll from their view never see it).
func wheel(_ x: Double, _ y: Double, _ lines: Int32) {
    let e = CGEvent(scrollWheelEvent2Source: nil, units: .line, wheelCount: 1, wheel1: lines, wheel2: 0, wheel3: 0)!
    e.location = CGPoint(x: x, y: y)
    e.post(tap: .cghidEventTap)
}
// Clicks are real HID input (the cursor moves), as XTest's on Linux: AppKit dispatches a click to a view only
// when the window server routes it. Keys and wheel stay targeted at the app's process.
func mouse(_ x: Double, _ y: Double, _ t: CGEventType) {
    let e = CGEvent(mouseEventSource: nil, mouseType: t, mouseCursorPosition: CGPoint(x: x, y: y), mouseButton: .left)!
    e.post(tap: .cghidEventTap)
}
func click(_ x: Double, _ y: Double) { mouse(x, y, .mouseMoved); mouse(x, y, .leftMouseDown); mouse(x, y, .leftMouseUp) }
func key(_ code: CGKeyCode, _ flags: CGEventFlags = []) {
    for down in [true, false] { let e = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down)!; e.flags = flags; e.postToPid(pid) }
}

if a[1] != "window" { for _ in 0..<100 { if contentRect(pid) != nil { break }; usleep(50_000) } }
switch a[1] {
case "activate":
    // as the harness's xdo_activate_window, then verified: the app is active and its window is uncovered
    // (macOS throttles occluded windows, so a covered window's capture would be invalid)
    let app = NSRunningApplication(processIdentifier: pid)
    var state = "covered"
    for _ in 0..<20 {
        _ = app?.activate(options: [.activateAllWindows, .activateIgnoringOtherApps])
        usleep(150_000)
        if uncovered(pid) { state = "front"; break }
    }
    print(state)
case "front":
    print(uncovered(pid) ? "front" : "covered")
case "click":
    click(Double(a[3])!, Double(a[4])!)
case "type":
    for _ in 0..<Int(a[3])! { key(0); usleep(20_000) }
case "window":
    var r: CGRect? = nil
    for _ in 0..<200 { r = contentRect(pid); if r != nil { break }; usleep(50_000) }
    guard let r = r else { print("none"); exit(1) }
    print("\(r.origin.x) \(r.origin.y) \(r.width) \(r.height)")
case "scroll":
    let (x, y, rate, secs) = (Double(a[3])!, Double(a[4])!, Double(a[5])!, Double(a[6])!)
    // as the harness: the pointer goes to the window's centre first (toolkits scroll what is under it)
    mouse(x - 1, y, .mouseMoved); usleep(50_000); mouse(x, y, .mouseMoved); usleep(50_000)
    let start = now(); var next = start; var n = 0
    let end = start + UInt64(secs * 1e9)
    while now() < end {
        let t = now()
        if t >= next {
            let half = Int(Double(t - start) / 0.5e9) % 2
            wheel(x, y, half == 0 ? -1 : 1)   // button 5 (down) first, as the harness
            n += 1; next += UInt64(1e9 / rate); if next < t { next = t + UInt64(1e9 / rate) }
        }
        sleepUntil(min(next, end))
    }
    print("{\"start_ns\": \(start), \"end_ns\": \(now()), \"events\": \(n)}")
case "dashboard":
    let (x0, y0, w, h) = (Double(a[3])!, Double(a[4])!, Double(a[5])!, Double(a[6])!)
    let (rate, phase, warm) = (Double(a[7])!, Double(a[8])!, Double(a[9])!)
    let at = { (px: Double, py: Double) in (x0 + px * w / 1000, y0 + py * h / 700) }
    // prepare: focus and clear the input, then warm up idle
    let (tx, ty) = at(860, 24); click(tx, ty); usleep(150_000)   // activate: a click on the window's empty top right
    let (fx, fy) = at(200, 90); click(fx, fy); usleep(50_000); key(0, .maskCommand); key(51); usleep(UInt32(warm * 1e6))
    var phases: [String] = []
    let started = now()
    for (i, name) in ["typing", "refresh", "toggle", "scroll"].enumerated() {
        let ps = started + UInt64(Double(i) * phase * 1e9), pe = ps + UInt64(phase * 1e9)
        var next = ps; var n = 0
        while now() < pe {
            let t = now()
            if t >= next {
                switch name {
                case "typing": n % 2 == 0 ? key(0) : key(51)
                case "refresh": let (x, y) = at(496, 90); click(x, y)
                case "toggle": let (x, y) = at(704, 90); click(x, y)
                default: let (x, y) = at(500, 400); mouse(x, y, .mouseMoved); wheel(x, y, t < ps + UInt64(phase * 0.5e9) ? -1 : 1)
                }
                n += 1; next = max(next + UInt64(1e9 / rate), now())
            }
            sleepUntil(min(next, pe))
        }
        phases.append("{\"phase\": \"\(name)\", \"events_sent\": \(n)}")
    }
    print("{\"start_ns\": \(started), \"end_ns\": \(now()), \"phases\": [\(phases.joined(separator: ", "))]}")
default: exit(2)
}
