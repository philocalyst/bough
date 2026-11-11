//! The frame log (feature `framelog`): per turn, monotonic timestamps of the input's own time, the turn's
//! phases, the frame's slot write and publication, and the publisher's GPU-done and commit times. Without
//! the feature every function here is an empty `#[inline(always)]` body: the call sites compile to nothing
//! (free when off, by construction, not by a runtime flag).
//!
//! Clock: `mach_absolute_time` converted to ns, the clock of `NSEvent.timestamp`, `CACurrentMediaTime` and
//! `CADisplayLink.timestamp` (all "host time": uptime, not counting sleep), so input, turn, commit and
//! vsync times share one axis.
//!
//! Storage: one preallocated table of 16-word rows behind a mutex (the main thread and the publisher thread
//! append), dumped as text at exit (`dump`). Format: `docs/lanes/pacing/FRAMELOG.md`.

/// Row tags.
pub const TAG_TURN: u64 = 1;
pub const TAG_PUB: u64 = 2;
pub const TAG_VSYNC: u64 = 3;
pub const TAG_NOTE: u64 = 4;
pub const TAG_EVENT: u64 = 5;

/// Turn kinds (`TAG_TURN` word 1).
pub const K_MOVE: u64 = 1;
pub const K_DOWN: u64 = 2;
pub const K_UP: u64 = 3;
pub const K_WHEEL: u64 = 4;
pub const K_KEY: u64 = 5;
pub const K_TICK: u64 = 6;
pub const K_TIMER: u64 = 7;
pub const K_RESIZE: u64 = 8;
pub const K_LEAVE: u64 = 9;
pub const K_PACED: u64 = 10;
pub const K_OTHER: u64 = 11;

/// Slots of the current turn (`TAG_TURN` words). 0 is the tag, 1 the kind.
pub const M_EVENT: usize = 2; // the input's own timestamp (NSEvent.timestamp), 0 for clock turns
pub const M_DISPATCH: usize = 3; // the host callback entered
pub const M_SYNC: usize = 4; // the first GPU sync of the turn returned (the region is writable)
pub const M_SETTLE: usize = 5; // routes ran; wires settled
pub const M_DESCRIBE: usize = 6; // `changed` waves and re-description done
pub const M_LAYOUT: usize = 7; // layout flushed: the turn's state is final
pub const M_FRAME: usize = 8; // the frame began (damage known)
pub const M_PICK: usize = 9; // the surface was chosen (ring waits included)
pub const M_SLOT: usize = 10; // the slot was written and the GPU released (or a held surface posted)
pub const M_END: usize = 11; // the host callback returned
pub const M_FID: usize = 12; // the presenter's publication sequence number of this turn's frame (0: none)
pub const M_FLAGS: usize = 13; // bit 0 drew, 1 core damage, 2 paint damage, 3 the input left something to show, bits 8.. pick (1 on screen, 2 held, 3 draw), 16.. tiles
pub const M_CPU: usize = 14; // the thread's CPU ns over the turn
pub const M_FLT: usize = 15; // minor faults (process) over the turn

/// Notes (`TAG_NOTE` word 1).
pub const N_SUPERSEDED: u64 = 1; // a queued publication replaced before the publisher took it (a = its seq)
pub const N_RING_WAIT: u64 = 2; // a frame waited for a free surface (a = µs)
pub const N_TRIM: u64 = 3; // the calm-window trim ran
pub const N_WARM: u64 = 4; // a keep-warm blit
pub const N_LINK: u64 = 5; // display link paused (a = 0) or resumed (a = 1)
pub const N_BINS: u64 = 6; // bins grew and the frame was redrawn
pub const N_POSTED: u64 = 7; // a synthetic event was posted (a = kind, b = sequence)
pub const N_OCCLUSION: u64 = 8; // the window's occlusion state (a = visible)
pub const N_HELD_BACK: u64 = 9; // the publisher held an overflowed frame back
pub const N_PHASE: u64 = 10; // a probe's workload phase began (a = index)
pub const N_WAKEUPS: u64 = 11; // process wakeups (a) and instructions (b) so far (a probe's idle window)
pub const N_GPU_WAIT: u64 = 12; // the CPU waited for the GPU's last frame before writing the region (a = µs, b = frame)
pub const N_DRAIN: u64 = 13; // the residency set grew: pre-armed frames drained one by one (a = µs, b = frames)
pub const N_PREPARE: u64 = 14; // a surface made ready (a = µs, b = 1 first use, 2 purged, 4 was volatile)
pub const N_UPLOAD: u64 = 15; // an atlas upload (a = µs, b = 1 glyph atlas, 2 glyph atlas resized, 3 images)
pub const N_LATCH: u64 = 16; // a latch armed (a = the target vsync, b = the latch time; host ns)

#[cfg(feature = "framelog")]
mod imp {
    use std::cell::Cell;
    use std::sync::Mutex;

    unsafe extern "C" {
        fn mach_absolute_time() -> u64;
        fn mach_timebase_info(info: *mut [u32; 2]) -> i32;
        fn clock_gettime_nsec_np(clock: u32) -> u64;
    }
    const CLOCK_THREAD_CPUTIME_ID: u32 = 16;

    const CAP: usize = 1 << 17;
    static ROWS: Mutex<Vec<[u64; 16]>> = Mutex::new(Vec::new());
    static DROPPED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static TB: std::sync::OnceLock<(u64, u64)> = std::sync::OnceLock::new();

    thread_local! {
        static CUR: [Cell<u64>; 16] = const { [const { Cell::new(0) }; 16] };
        static OPEN: Cell<bool> = const { Cell::new(false) };
    }

    pub fn timebase() -> (u64, u64) {
        tb()
    }

    fn tb() -> (u64, u64) {
        *TB.get_or_init(|| {
            let mut t = [0u32; 2];
            // SAFETY: fills the two-u32 struct (numer, denom).
            unsafe { mach_timebase_info(&mut t) };
            (t[0] as u64, t[1] as u64)
        })
    }

    #[inline]
    pub fn now_ns() -> u64 {
        let (n, d) = tb();
        // SAFETY: no arguments.
        let t = unsafe { mach_absolute_time() };
        t * n / d
    }

    fn cpu_ns() -> u64 {
        // SAFETY: a valid clock id.
        unsafe { clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID) }
    }

    fn minflt() -> u64 {
        let mut r: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: fills `r`.
        unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut r) };
        r.ru_minflt as u64
    }

    pub fn push(r: [u64; 16]) {
        let mut g = ROWS.lock().unwrap();
        if g.capacity() == 0 {
            g.reserve_exact(CAP);
        }
        if g.len() < CAP {
            g.push(r);
        } else {
            DROPPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn turn_begin(kind: u64, event_ns: u64) {
        let t = now_ns();
        CUR.with(|c| {
            for x in c.iter() {
                x.set(0);
            }
            c[0].set(super::TAG_TURN);
            c[1].set(kind);
            c[super::M_EVENT].set(event_ns);
            c[super::M_DISPATCH].set(t);
            c[super::M_CPU].set(cpu_ns());
            c[super::M_FLT].set(minflt());
        });
        OPEN.with(|o| o.set(true));
    }

    #[inline]
    pub fn mark(i: usize) {
        if OPEN.with(|o| o.get()) {
            let t = now_ns();
            CUR.with(|c| {
                if c[i].get() == 0 {
                    c[i].set(t);
                }
            });
        }
    }

    #[inline]
    pub fn mark_last(i: usize) {
        if OPEN.with(|o| o.get()) {
            let t = now_ns();
            CUR.with(|c| c[i].set(t));
        }
    }

    #[inline]
    pub fn set(i: usize, v: u64) {
        if OPEN.with(|o| o.get()) {
            CUR.with(|c| c[i].set(v));
        }
    }

    #[inline]
    pub fn or(i: usize, v: u64) {
        if OPEN.with(|o| o.get()) {
            CUR.with(|c| c[i].set(c[i].get() | v));
        }
    }

    pub fn turn_end() {
        if !OPEN.with(|o| o.replace(false)) {
            return;
        }
        let t = now_ns();
        let (cpu, flt) = (cpu_ns(), minflt());
        let mut r = [0u64; 16];
        CUR.with(|c| {
            for (k, x) in c.iter().enumerate() {
                r[k] = x.get();
            }
        });
        r[super::M_END] = t;
        r[super::M_CPU] = cpu.saturating_sub(r[super::M_CPU]);
        r[super::M_FLT] = flt.saturating_sub(r[super::M_FLT]);
        push(r);
    }

    pub fn dump(path: &std::path::Path) {
        use std::io::Write;
        let g = ROWS.lock().unwrap();
        let Ok(f) = std::fs::File::create(path) else {
            return;
        };
        let mut w = std::io::BufWriter::new(f);
        let _ = writeln!(
            w,
            "# bough frame log v1; ns on the host clock; dropped {}",
            DROPPED.load(std::sync::atomic::Ordering::Relaxed)
        );
        for r in g.iter() {
            let s: Vec<String> = r.iter().map(|x| x.to_string()).collect();
            let _ = writeln!(w, "{}", s.join(" "));
        }
    }
}

/// Now, in ns on the host clock (0 without the feature).
#[inline(always)]
pub fn now_ns() -> u64 {
    #[cfg(feature = "framelog")]
    return imp::now_ns();
    #[cfg(not(feature = "framelog"))]
    0
}

/// Host-clock seconds (`NSEvent.timestamp`, `CACurrentMediaTime`) to ns.
#[inline(always)]
pub fn secs_to_ns(s: f64) -> u64 {
    (s * 1e9) as u64
}

/// Start a turn on this thread: `kind`, the input's own time (0 for a clock turn).
#[inline(always)]
pub fn turn_begin(_kind: u64, _event_ns: u64) {
    #[cfg(feature = "framelog")]
    imp::turn_begin(_kind, _event_ns);
}

/// Stamp slot `i` of the current turn with now, if it is not stamped yet (the first sync of a turn).
#[inline(always)]
pub fn mark(_i: usize) {
    #[cfg(feature = "framelog")]
    imp::mark(_i);
}

/// Stamp slot `i` with now, overwriting (the last occurrence).
#[inline(always)]
pub fn mark_last(_i: usize) {
    #[cfg(feature = "framelog")]
    imp::mark_last(_i);
}

#[inline(always)]
pub fn set(_i: usize, _v: u64) {
    #[cfg(feature = "framelog")]
    imp::set(_i, _v);
}

#[inline(always)]
pub fn or(_i: usize, _v: u64) {
    #[cfg(feature = "framelog")]
    imp::or(_i, _v);
}

/// Close the current turn and append its row.
#[inline(always)]
pub fn turn_end() {
    #[cfg(feature = "framelog")]
    imp::turn_end();
}

/// A publication: sequence, posted, taken by the publisher, GPU done (as observed), commit begin and end,
/// committed (1) or held back (0), surface, source (0 publisher thread, 1 calling thread).
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub fn publish(
    _seq: u64,
    _posted: u64,
    _taken: u64,
    _drawn: u64,
    _c0: u64,
    _c1: u64,
    _ok: bool,
    _surf: usize,
    _main: bool,
) {
    #[cfg(feature = "framelog")]
    imp::push([
        TAG_PUB,
        _seq,
        _posted,
        _taken,
        _drawn,
        _c0,
        _c1,
        _ok as u64,
        _surf as u64,
        _main as u64,
        0,
        0,
        0,
        0,
        0,
        0,
    ]);
}

/// A display-link callback: the vsync it reports, the target, and when the callback ran.
#[inline(always)]
pub fn vsync(_ts: f64, _target: f64) {
    #[cfg(feature = "framelog")]
    imp::push([
        TAG_VSYNC,
        secs_to_ns(_ts),
        secs_to_ns(_target),
        imp::now_ns(),
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]);
}

/// A vsync from a measurement recorder (host ns): the vsync, the output time it reports, a source tag.
#[inline(always)]
pub fn vsync_ns(_ts: u64, _target: u64, _src: u64) {
    #[cfg(feature = "framelog")]
    imp::push([
        TAG_VSYNC,
        _ts,
        _target,
        imp::now_ns(),
        _src,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]);
}

/// The host clock's timebase (numer, denom: ticks * numer / denom = ns).
pub fn timebase() -> (u64, u64) {
    #[cfg(feature = "framelog")]
    return imp::timebase();
    #[cfg(not(feature = "framelog"))]
    (1, 1)
}

/// A note: what, a, b (stamped now).
#[inline(always)]
pub fn note(_what: u64, _a: u64, _b: u64) {
    #[cfg(feature = "framelog")]
    imp::push([
        TAG_NOTE,
        _what,
        imp::now_ns(),
        _a,
        _b,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]);
}

/// An input event as the host received it: kind, its own time, received, a sequence number it carries
/// (synthetic events: the driver's), extra.
#[inline(always)]
pub fn event(_kind: u64, _event_ns: u64, _seq: u64, _extra: u64) {
    #[cfg(feature = "framelog")]
    imp::push([
        TAG_EVENT,
        _kind,
        _event_ns,
        imp::now_ns(),
        _seq,
        _extra,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]);
}

/// Is the log compiled in?
pub const ON: bool = cfg!(feature = "framelog");

/// Write the log as text (one row per line) to `path`.
#[inline(always)]
pub fn dump(_path: &std::path::Path) {
    #[cfg(feature = "framelog")]
    imp::dump(_path);
}
