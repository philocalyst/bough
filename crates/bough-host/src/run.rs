//! The loop (Linux): one thread, one `poll`.
//!
//! Each pass: drain the socket (everything the compositor sent is decoded and applied to the application's
//! state before anything is drawn, so input is coalesced and the newest state wins); run the application's
//! clock if it is due; settle; if there is damage, draw and commit **now** (commit on input: a pending frame
//! callback delays nothing); then wait for the socket or the clock. Time advances only on a frame callback, and
//! a callback is asked for only while something is in continuous motion, so an idle window makes no system
//! call at all and an animation runs at the output's refresh.
//!
//! Start-up is the chain of `wl.rs`: the application mounts and draws its first frame into shared memory while
//! the compositor is answering the toplevel's first commit, and the first buffer is committed in the same write
//! as the configure acknowledgement.
use crate::pace::Ring;
use crate::wl::{Conn, Error, Prep, Shm, Wl};
use crate::{App, TilesRef};
use std::time::{Duration, Instant};

/// What the window asks for.
pub struct Window<'a> {
    pub title: &'a str,
    pub app_id: &'a str,
    /// Logical size asked for (the compositor may configure another).
    pub w: i32,
    pub h: i32,
    /// Stop after this long (a test harness's switch); `None`: run until closed.
    pub seconds: Option<f64>,
}

/// Start-up timestamps (nanoseconds since [`run`] was entered) and loop counters, printed at exit when
/// `BOUGH_HOST_STATS` is set.
#[derive(Default, Debug)]
pub struct Trace {
    pub connected: u64,
    pub write1: u64,
    pub mounted: u64,
    pub globals: u64,
    pub write2: u64,
    pub drawn: u64,
    pub configured: u64,
    pub write3: u64,
    pub frames: u64,
    pub input_frames: u64,
    pub callback_frames: u64,
    pub starved: u64,
    pub redrawn_for_size: u64,
}

struct Buffers {
    shm: Shm,
    pool: u32,
    /// Bytes per slot (a buffer of the largest size seen, rounded up to a page).
    slot: usize,
    /// wl_buffer ids of the current generation, by ring index (0: not created).
    ids: Vec<u32>,
    generation: u32,
    /// Buffers of older generations that the compositor still holds: destroyed on release.
    old: Vec<(u32, u32, u32)>,
    w: i32,
    h: i32,
}

const MAX_BUFS: usize = 4;

impl Buffers {
    fn bytes(w: i32, h: i32) -> usize {
        (w as usize * h as usize * 4).next_multiple_of(4096)
    }
    fn px(&self, i: usize) -> *mut u32 {
        // SAFETY: slot `i` lies inside the mapping (`ensure` grew it).
        unsafe { self.shm.map.add(i * self.slot) as *mut u32 }
    }
    /// Make sure slot `i` exists for the current size and has a `wl_buffer`.
    fn ensure(&mut self, c: &mut Conn, i: usize) -> Result<u32, Error> {
        let need = (i + 1) * self.slot;
        if need > self.shm.len {
            self.shm.grow(need)?;
            if self.pool != 0 {
                c.resize_pool(self.pool, self.shm.len as i32);
            }
        }
        if self.pool == 0 {
            self.pool = c.create_pool(self.shm.fd, self.shm.len as i32);
        }
        if self.ids.len() <= i {
            self.ids.resize(i + 1, 0);
        }
        if self.ids[i] == 0 {
            self.ids[i] = c.create_buffer(self.pool, (i * self.slot) as i32, self.w, self.h, self.generation, i as u32);
        }
        Ok(self.ids[i])
    }
    /// The window's buffers are now `w` x `h`: the old generation's buffers are destroyed (at once if free,
    /// else when released).
    fn resize(&mut self, c: &mut Conn, ring: &Ring, w: i32, h: i32) -> Result<(), Error> {
        for (i, id) in self.ids.drain(..).enumerate() {
            if id == 0 {
                continue;
            }
            if ring.held(i) {
                self.old.push((self.generation, i as u32, id));
            } else {
                c.destroy_buffer(id);
            }
        }
        self.generation += 1;
        (self.w, self.h) = (w, h);
        let slot = Self::bytes(w, h);
        if slot > self.slot {
            // slots move: every slot of the new generation starts blank, which a new ring says anyway
            self.slot = slot;
        }
        Ok(())
    }
    fn released_old(&mut self, c: &mut Conn, generation: u32, index: u32) {
        if let Some(k) = self.old.iter().position(|o| o.0 == generation && o.1 == index) {
            let (_, _, id) = self.old.swap_remove(k);
            c.destroy_buffer(id);
        }
    }
}

fn ns(t0: Instant) -> u64 {
    t0.elapsed().as_nanos() as u64
}

/// Open the window and run `app` until the window closes. Never returns normally.
pub fn run<A: App>(win: Window<'_>, make: impl FnOnce() -> A) -> Result<Trace, Error> {
    let t0 = Instant::now();
    let mut tr = Trace::default();
    let stats = std::env::var_os("BOUGH_HOST_STATS").is_some();
    // write 1: registry and sync
    let mut c = Conn::connect()?;
    tr.connected = ns(t0);
    c.flush()?;
    tr.write1 = ns(t0);
    // while the compositor lists its globals: the application's state, the scene at the size asked for, and
    // the memory of one buffer
    let prep = Prep::from_env();
    let app = &mut make();
    let (mut scale120, mut logical) = (120u32, (win.w, win.h));
    let (mut bw, mut bh) = (win.w, win.h);
    app.resize(bw, bh, 1);
    let mut ring = Ring::new(bw, bh, 1, MAX_BUFS);
    let slot = Buffers::bytes(bw, bh);
    let mut bufs = Buffers { shm: Shm::with(slot, prep)?, pool: 0, slot, ids: Vec::new(), generation: 0, old: Vec::new(), w: bw, h: bh };
    tr.mounted = ns(t0);
    let mut evs: Vec<Wl> = Vec::with_capacity(64);
    while !c.ready() {
        c.read(true)?;
        while c.next(&mut evs)? {}
    }
    tr.globals = ns(t0);
    // write 2: binds, surface, toplevel, the bufferless commit
    c.create(win.title, win.app_id)?;
    c.flush()?;
    tr.write2 = ns(t0);
    // while the compositor makes the toplevel: the first frame, into the first buffer's memory
    let mut first = ring.begin().expect("a new ring has a free buffer");
    let d = app.settle();
    let _ = d;
    // SAFETY: slot 0 of the mapping holds `bh` rows of `bw` pixels and nothing else uses it yet.
    unsafe { app.paint(bufs.px(first.buf), bw as usize, None) };
    app.painted();
    tr.drawn = ns(t0);
    // the first configure
    let mut configured = false;
    while !configured {
        if evs.is_empty() {
            c.read(true)?;
            while c.next(&mut evs)? {}
        }
        for e in evs.drain(..) {
            match e {
                Wl::Configure { w, h, .. } => {
                    configured = true;
                    if w > 0 && h > 0 {
                        logical = (w, h);
                    }
                }
                Wl::Scale(s) => scale120 = s.max(1),
                _ => {}
            }
        }
    }
    tr.configured = ns(t0);
    let size = |logical: (i32, i32), s: u32| (((logical.0 as i64 * s as i64 + 60) / 120).max(1) as i32, ((logical.1 as i64 * s as i64 + 60) / 120).max(1) as i32);
    if size(logical, scale120) != (bw, bh) {
        // the compositor chose another size or scale: the speculative frame is of no use
        (bw, bh) = size(logical, scale120);
        app.resize(bw, bh, scale120.div_ceil(120) as i32);
        ring = Ring::new(bw, bh, 1, MAX_BUFS);
        bufs.resize(&mut c, &ring, bw, bh)?;
        bufs.shm.grow(bufs.slot)?;
        first = ring.begin().expect("a new ring has a free buffer");
        app.settle();
        // SAFETY: as above, at the new size (the mapping was grown to hold a slot of it).
        unsafe { app.paint(bufs.px(first.buf), bw as usize, None) };
        app.painted();
        tr.redrawn_for_size += 1;
    }
    c.scale = scale120 as f64 / 120.0;
    // write 3: the acknowledgement and the first buffer, together
    let id = bufs.ensure(&mut c, first.buf)?;
    let mut animating = app.animating();
    let mut cb_pending = animating;
    c.commit(id, scale120, logical, animating, |f| ring.commit(first.buf, |x, y, w, h| f(x, y, w, h)));
    c.flush()?;
    tr.write3 = ns(t0);
    tr.frames = 1;
    if stats {
        eprintln!("bough-host start: {tr:?}");
    }
    // The next frame goes into a second buffer (the compositor holds the first): make its pages exist now,
    // while nothing is waiting, so the first input pays for a repair and not for a thousand page faults.
    if prep != Prep::Touch {
        bufs.shm.grow(2 * bufs.slot)?;
        bufs.shm.prepare(bufs.slot, bufs.slot, prep);
        c.resize_pool(bufs.pool, bufs.shm.len as i32);
        c.flush()?;
    }

    let deadline = win.seconds.map(|s| t0 + Duration::from_secs_f64(s));
    let period = app.period();
    let mut next_clock = period.map(|p| Instant::now() + p);
    let mut time_dirty = false;
    loop {
        // 1. everything the compositor has sent, applied to the state before anything is drawn
        let mut resized = false;
        loop {
            while c.next(&mut evs)? {}
            if !c.read(false)? {
                break;
            }
        }
        for e in evs.drain(..) {
            match e {
                Wl::Input(crate::event::Event::Close) => return Ok(tr),
                Wl::Input(ev) => app.input(&ev),
                Wl::Frame => {
                    cb_pending = false;
                    time_dirty = true;
                }
                Wl::Release { generation, index } => {
                    if generation == bufs.generation {
                        ring.release(index as usize);
                    } else {
                        bufs.released_old(&mut c, generation, index);
                    }
                }
                Wl::Configure { w, h, .. } => {
                    if w > 0 && h > 0 && (w, h) != logical {
                        logical = (w, h);
                        resized = true;
                    }
                }
                Wl::Scale(s) => {
                    if s.max(1) != scale120 {
                        scale120 = s.max(1);
                        resized = true;
                    }
                }
            }
        }
        if resized {
            (bw, bh) = size(logical, scale120);
            c.scale = scale120 as f64 / 120.0;
            app.resize(bw, bh, scale120.div_ceil(120) as i32);
            let old = core::mem::replace(&mut ring, Ring::new(bw, bh, 1, MAX_BUFS));
            bufs.resize(&mut c, &old, bw, bh)?;
            // slots still held by the compositor in the old generation keep their memory until released: the
            // new generation's slot 0 may overlap one, so it starts after them when any is held
            if !bufs.old.is_empty() {
                // simplest sound choice: a fresh memfd for the new generation
                let fresh = Shm::with(bufs.slot, prep)?;
                let stale = core::mem::replace(&mut bufs.shm, fresh);
                // The old pool object is destroyed now: its buffers that the compositor still holds stay valid
                // and are destroyed as they are released, and the compositor frees the pool's memory with the
                // last of them. Our own mapping and descriptor go at once.
                if bufs.pool != 0 {
                    c.destroy_pool(bufs.pool);
                }
                drop(stale);
                bufs.pool = 0;
            }
        }
        // 2. the application's clock
        let now = Instant::now();
        if let (Some(at), Some(p)) = (next_clock, period)
            && at <= now
        {
            app.clock();
            let mut at = at;
            while at <= now {
                at += p;
            }
            next_clock = Some(at);
        }
        if deadline.is_some_and(|d| d <= now) {
            if stats {
                eprintln!("bough-host: {tr:?} ring {:?} writes {} reads {} commits {}", ring.stats, c.writes, c.reads, c.commits);
            }
            return Ok(tr);
        }
        // 3. time advances with the compositor's frames
        if time_dirty {
            time_dirty = false;
            app.advance(now.duration_since(t0).as_millis() as u32);
        }
        // 4. settle; draw and commit whatever changed, now
        let d = app.settle();
        let any = d.lo < d.hi;
        if any {
            ring.damage(d);
            app.painted();
        }
        animating = app.animating();
        if !c.suspended && (ring.owed() || c.ack.is_some() && resized) {
            match ring.begin() {
                Some(f) => {
                    let id = bufs.ensure(&mut c, f.buf)?;
                    let repair: Option<TilesRef<'_>> = if f.full { None } else { Some(ring.repair()) };
                    // SAFETY: slot `f.buf` holds the window's rows; the compositor does not hold this buffer
                    // (the ring handed it out as free).
                    unsafe { app.paint(bufs.px(f.buf), bw as usize, repair) };
                    let want_cb = animating && !cb_pending;
                    c.commit(id, scale120, logical, want_cb, |g| ring.commit(f.buf, |x, y, w, h| g(x, y, w, h)));
                    cb_pending |= want_cb;
                    tr.frames += 1;
                    if any {
                        tr.input_frames += 1;
                    } else {
                        tr.callback_frames += 1;
                    }
                }
                None => tr.starved += 1,
            }
        } else if c.ack.is_some() {
            c.ack_only();
        }
        if animating && !cb_pending && !c.suspended {
            // in motion with nothing committed this pass: ask for the next frame's callback on its own
            c.callback_only();
            cb_pending = true;
        }
        c.flush()?;
        // 5. wait for the compositor or the clock
        let timeout = [next_clock, deadline].into_iter().flatten().min().map(|at| at.saturating_duration_since(Instant::now()));
        let ms = timeout.map_or(-1, |t| t.as_micros().div_ceil(1000).min(i32::MAX as u128) as i32);
        let mut p = libc::pollfd { fd: c.fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd.
        unsafe { libc::poll(&mut p, 1, ms) };
    }
}
