//! The harness's floor: a client that does nothing a window can do without. It connects, binds, makes a
//! toplevel, fills one buffer with one colour, and commits it with the configure acknowledgement; then it
//! answers pings and configures until the window is closed. Whatever a benchmark harness reports as start-up
//! for this program is not the framework's: process creation, the compositor's two round trips, the kernel's
//! first touch of the buffer's pages, and the wait for the latch.
//!
//! `floor [WIDTH HEIGHT]` (default 1280 800). `FLOOR_NOFILL=1` commits the buffer untouched (zero pages).
#[cfg(target_os = "linux")]
fn main() {
    use bough_host::wl::{Conn, Shm, Wl};
    let a: Vec<String> = std::env::args().collect();
    let (mut w, mut h) = (a.get(1).and_then(|s| s.parse().ok()).unwrap_or(1280), a.get(2).and_then(|s| s.parse().ok()).unwrap_or(800));
    let mut run = || -> Result<(), bough_host::wl::Error> {
        let mut c = Conn::connect()?;
        c.flush()?;
        let mut shm = Shm::new((w * h * 4) as usize)?;
        let mut evs = Vec::new();
        while !c.ready() {
            c.read(true)?;
            while c.next(&mut evs)? {}
        }
        c.create("floor", "gui-bench.floor")?;
        c.flush()?;
        let fill = |shm: &mut Shm, w: i32, h: i32| {
            if std::env::var_os("FLOOR_NOFILL").is_none() {
                // SAFETY: the mapping holds `w * h` pixels.
                unsafe { core::slice::from_raw_parts_mut(shm.map as *mut u32, (w * h) as usize) }.fill(0xffff_ffff);
            }
        };
        fill(&mut shm, w, h);
        let mut configured = false;
        while !configured {
            if evs.is_empty() {
                c.read(true)?;
                while c.next(&mut evs)? {}
            }
            for e in evs.drain(..) {
                if let Wl::Configure { w: cw, h: ch, .. } = e {
                    configured = true;
                    if cw > 0 && ch > 0 && (cw, ch) != (w, h) {
                        (w, h) = (cw, ch);
                        shm.grow((w * h * 4) as usize)?;
                        fill(&mut shm, w, h);
                    }
                }
            }
        }
        let pool = c.create_pool(shm.fd, shm.len as i32);
        let buf = c.create_buffer(pool, 0, w, h, 0, 0);
        c.commit(buf, 120, (w, h), false, |f| f(0, 0, w, h));
        c.flush()?;
        loop {
            c.read(true)?;
            while c.next(&mut evs)? {}
            for e in evs.drain(..) {
                if let Wl::Input(bough_host::event::Event::Close) = e {
                    return Ok(());
                }
            }
            c.ack_only();
            c.flush()?;
        }
    };
    if let Err(e) = run() {
        eprintln!("floor: {e}");
        std::process::exit(1);
    }
}
#[cfg(not(target_os = "linux"))]
fn main() {}
