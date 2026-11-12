//! Keep a benchmark's window from disturbing the user (bench/tools/space/README.md).
//!
//! [`show`] orders a window we created in so that it never activates the app, never becomes key, ignores the mouse
//! and is never on the user's screen, while its view's display link keeps ticking at the display's rate:
//!
//! * `other` (the default): a Space this process creates (`CGSSpaceCreate`, SkyLight, no permission needed) and
//!   never shows; the window keeps an on-display position inside it, so its screen, backing scale and display
//!   link are the main display's. The Space goes away with the process's window-server connection.
//! * `offscreen`: a position outside every display. Measured on macOS 15: the view's display link does not tick
//!   there at all (the window has no screen), so display-link-paced benchmarks stall. Use it only for timers.
//! * `active`: the old behaviour (on the user's current Space, ordered in at its frame).
//!
//! `BOUGH_BENCH_SPACE` picks the mode ([`mode_from_env`]). Nothing here activates the app, posts input, uses UI
//! scripting or asks for a permission.
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSScreen, NSWindow, NSWindowCollectionBehavior};
#[cfg(target_os = "macos")]
use objc2_foundation::{MainThreadMarker, NSArray, NSNumber, NSPoint};

/// Where a benchmark window goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A private Space this process created: invisible, display link at the display's rate.
    Other,
    /// Outside every display: invisible, no display link.
    Offscreen,
    /// The user's current Space, at the window's own frame (disturbs the user; for debugging only).
    Active,
}

/// `BOUGH_BENCH_SPACE` = `other` | `offscreen` | `active` (also `0`/`off` = active); unset: `default`.
pub fn mode_from_env(default: Mode) -> Mode {
    match std::env::var("BOUGH_BENCH_SPACE").as_deref() {
        Ok("other") | Ok("space") | Ok("1") | Ok("on") => Mode::Other,
        Ok("offscreen") => Mode::Offscreen,
        Ok("active") | Ok("0") | Ok("off") => Mode::Active,
        _ => default,
    }
}

/// What [`show`] did.
#[derive(Clone, Debug)]
pub struct Placed {
    /// The mode that took effect (`Other` falls back to `Offscreen` when no Space could be created).
    pub mode: Mode,
    /// The private Space's id (`Other`), the user's active Space's id.
    pub space: u64,
    pub active_space: u64,
    /// The window server's own answer after placement: is any part of the window on screen?
    pub on_screen: bool,
    pub note: &'static str,
}

#[cfg(target_os = "macos")]
mod sl {
    //! SkyLight's window-server calls, looked up at run time (no link-time dependency on a private framework).
    use std::ffi::c_void;
    pub type Cid = i32;
    pub struct Sky {
        pub main_connection: unsafe extern "C" fn() -> Cid,
        pub active_space: unsafe extern "C" fn(Cid) -> u64,
        pub space_create: unsafe extern "C" fn(Cid, i32, *const c_void) -> u64,
        pub add_windows: unsafe extern "C" fn(Cid, *const c_void, *const c_void),
        pub remove_windows: unsafe extern "C" fn(Cid, *const c_void, *const c_void),
    }
    pub fn load() -> Option<Sky> {
        // SAFETY: dlopen/dlsym with NUL-terminated names; each symbol is transmuted to the signature SkyLight has
        // exported since macOS 10.x (the same ones yabai and Hammerspoon call), checked non-null first.
        unsafe {
            let h = libc::dlopen(
                c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight".as_ptr(),
                libc::RTLD_LAZY,
            );
            if h.is_null() {
                return None;
            }
            let f = |n: &std::ffi::CStr| {
                let p = libc::dlsym(h, n.as_ptr());
                (!p.is_null()).then_some(p)
            };
            Some(Sky {
                main_connection: std::mem::transmute(f(c"CGSMainConnectionID")?),
                active_space: std::mem::transmute(f(c"CGSGetActiveSpace")?),
                space_create: std::mem::transmute(f(c"CGSSpaceCreate")?),
                add_windows: std::mem::transmute(f(c"CGSAddWindowsToSpaces")?),
                remove_windows: std::mem::transmute(f(c"CGSRemoveWindowsFromSpaces")?),
            })
        }
    }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *const std::ffi::c_void;
}

/// Does the window server report any part of window `wid` on screen (`kCGWindowIsOnscreen`)?
#[cfg(target_os = "macos")]
pub fn on_screen(wid: isize) -> bool {
    use objc2_foundation::{NSDictionary, NSString};
    // kCGWindowListOptionIncludingWindow = 1 << 3
    // SAFETY: a Create-rule CFArray of CFDictionary (toll-free bridged to NSArray<NSDictionary>), or null.
    let p = unsafe { CGWindowListCopyWindowInfo(1 << 3, wid as u32) };
    if p.is_null() {
        return false;
    }
    let arr: Retained<NSArray<NSDictionary<NSString, objc2::runtime::AnyObject>>> =
        // SAFETY: the Create rule hands us one reference.
        unsafe { Retained::from_raw(p as *mut _) }.expect("window list");
    let key = NSString::from_str("kCGWindowIsOnscreen");
    arr.firstObject()
        .and_then(|d| d.objectForKey(&key))
        .and_then(|v| v.downcast::<NSNumber>().ok())
        .is_some_and(|n| n.boolValue())
}

/// A point outside every display (to the right of the rightmost and above the highest).
#[cfg(target_os = "macos")]
fn far_point(mtm: MainThreadMarker) -> NSPoint {
    let (mut x, mut y) = (0.0f64, 0.0f64);
    for s in NSScreen::screens(mtm).iter() {
        let f = s.frame();
        x = x.max(f.origin.x + f.size.width);
        y = y.max(f.origin.y + f.size.height);
    }
    NSPoint::new(x + 20000.0, y + 20000.0)
}

/// Order `window` in under `mode` instead of `orderFrontRegardless`/`makeKeyAndOrderFront`. Call it on the main
/// thread, once, with the window at the frame it should have; the app should be an accessory (no Dock icon) and
/// must never call `activate`. The window ignores the mouse from here on (so a stray click cannot activate the
/// app) and leaves the window cycle.
#[cfg(target_os = "macos")]
pub fn show(window: &NSWindow, mode: Mode) -> Placed {
    let mtm = MainThreadMarker::new().expect("bough-space: main thread");
    window.setIgnoresMouseEvents(true);
    window.setCollectionBehavior(NSWindowCollectionBehavior::IgnoresCycle);
    let wid = window.windowNumber();
    if mode == Mode::Active {
        window.orderFrontRegardless();
        return Placed {
            mode,
            space: 0,
            active_space: 0,
            on_screen: on_screen(wid),
            note: "on the user's Space",
        };
    }
    // ordered in far outside every display first: nothing is ever drawn where the user can see it
    let home = window.frame().origin;
    window.setFrameOrigin(far_point(mtm));
    window.orderFrontRegardless();
    if mode == Mode::Other {
        if let Some(sky) = sl::load() {
            // SAFETY: SkyLight calls on our own connection with our own window number and Space ids; the arrays
            // are NSArray<NSNumber> passed as their toll-free-bridged CFArrayRef and outlive the calls.
            unsafe {
                let cid = (sky.main_connection)();
                let active = (sky.active_space)(cid);
                let sid = (sky.space_create)(cid, 1, std::ptr::null());
                if sid != 0 {
                    let w = NSArray::from_retained_slice(&[NSNumber::new_isize(wid)]);
                    let to = NSArray::from_retained_slice(&[NSNumber::new_u64(sid)]);
                    let from = NSArray::from_retained_slice(&[NSNumber::new_u64(active)]);
                    let ptr = |a: &Retained<NSArray<NSNumber>>| Retained::as_ptr(a) as *const std::ffi::c_void;
                    (sky.add_windows)(cid, ptr(&w), ptr(&to));
                    (sky.remove_windows)(cid, ptr(&w), ptr(&from));
                    // back at its own frame on the main display, inside the private Space: a screen, a scale, a link
                    window.setFrameOrigin(home);
                    return Placed {
                        mode,
                        space: sid,
                        active_space: active,
                        on_screen: on_screen(wid),
                        note: "a private Space (CGSSpaceCreate), never shown",
                    };
                }
            }
        }
        return Placed {
            mode: Mode::Offscreen,
            space: 0,
            active_space: 0,
            on_screen: on_screen(wid),
            note: "no Space could be created: outside every display (no display link there)",
        };
    }
    Placed {
        mode,
        space: 0,
        active_space: 0,
        on_screen: on_screen(wid),
        note: "outside every display (no display link there)",
    }
}
