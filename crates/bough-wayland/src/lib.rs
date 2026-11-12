//! bough-wayland: a Wayland toplevel for bough, written against the protocol directly (wayland-client's pure-Rust
//! backend; no libwayland, no toolkit, no event-loop framework), so every request the window makes is one bough
//! chose:
//!
//! * **xdg-shell** toplevel (v6: the `suspended` state stops frames and frame callbacks while hidden); every
//!   configure is acknowledged, a size change with the first commit drawn at the new size.
//! * **wl_shm** presentation from a ring of `XRGB8888` buffers in one memfd per size ([`Shell::pixels`]): the
//!   façade's surface ring repairs a buffer by its age, so a frame writes only its damaged tiles, and
//!   `wl_surface.damage_buffer` carries exactly the tiles that differ from what the compositor shows
//!   ([`Shell::present`]: one rectangle per run of damaged tiles, merged down rows). A buffer is drawable again
//!   once the compositor releases it.
//! * **Frame callbacks**: a present requests one; the host draws again only after it fired ([`Shell::frame_pending`]),
//!   so redraws follow the compositor's refresh and an idle window commits nothing.
//! * **Input**: `wl_pointer` (motion, buttons, finger and wheel axes grouped per `frame`), `wl_keyboard` with the
//!   compositor's xkb keymap (libxkbcommon loaded at run time) and modifiers.
//! * **Scale**: `wp_fractional_scale_v1` with `wp_viewporter` when the compositor has them, else the integer
//!   `wl_surface.preferred_buffer_scale` (v6) with `set_buffer_scale`.
//!
//! The host loop (the façade's `backend::wayland`) owns a [`Shell`]: it waits in [`Shell::dispatch`] (one `poll`
//! on the connection with the next timer as the timeout), takes the decoded [`Event`]s, runs turns, and draws
//! into [`Shell::pixels`] / [`Shell::present`]. Linux only: on other targets the crate is empty.
#![cfg(target_os = "linux")]

use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols::wp::fractional_scale::v1::client::{wp_fractional_scale_manager_v1 as frac_mgr, wp_fractional_scale_v1 as frac};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

/// Keyboard modifiers in effect for a key event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
}

/// What the window reports to its host, decoded and in arrival order.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The compositor sized the window to `w` x `h` logical px (resize the content; draw at the new size).
    Configure { w: u32, h: u32 },
    /// The preferred scale changed (buffer px per logical px).
    Scale(f64),
    /// The window is hidden (`true`): draw nothing, request no frame callbacks; or shown again.
    Suspended(bool),
    /// The user (or the compositor) asked the window to close.
    Close,
    /// The frame callback of the last present fired: the next frame may be drawn.
    Frame { time_ms: u32 },
    /// The pointer moved to (x, y) logical px (also on enter).
    Motion { x: f64, y: f64 },
    /// The pointer left the window.
    Leave,
    /// A button (`BTN_LEFT` = 0x110, `BTN_RIGHT` = 0x111, ...) at (x, y).
    Button { x: f64, y: f64, button: u32, pressed: bool },
    /// One pointer frame's scroll at (x, y): `finger` (touchpad, continuous) in logical px, else wheel detents
    /// (value120 / 120, or the axis value / 15 without value120). Positive dy scrolls the content up (wheel down).
    Scroll { x: f64, y: f64, dx: f64, dy: f64, finger: bool },
    /// A key: its keysym under the current modifiers and layout, the printable character it types (if any),
    /// press or release.
    Key { sym: u32, text: Option<char>, pressed: bool, mods: Mods },
}

pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;

/// One generation of buffers: a memfd, its pool, its mapping and `n` same-sized buffers at `stride * h` apart.
struct Gen {
    id: u32,
    w: u32,
    h: u32,
    stride: usize,
    map: *mut u8,
    len: usize,
    _fd: OwnedFd,
    pool: wl_shm_pool::WlShmPool,
    bufs: Vec<wl_buffer::WlBuffer>,
    busy: Vec<bool>,
}

impl Gen {
    fn destroy(self) {
        for b in &self.bufs {
            b.destroy();
        }
        self.pool.destroy();
        // SAFETY: the mapping this generation made and alone owns; no pointer into it survives (the host draws
        // only into the current generation, and `pixels` pointers are valid until the next resize).
        unsafe { libc::munmap(self.map as *mut libc::c_void, self.len) };
    }
}

/// The xkb state of the keyboard (libxkbcommon through xkbcommon-dl).
struct Xkb {
    ctx: *mut xkbcommon_dl::xkb_context,
    /// Owned by this keyboard (kept alive with its state).
    _keymap: *mut xkbcommon_dl::xkb_keymap,
    state: *mut xkbcommon_dl::xkb_state,
}

pub struct State {
    qh: QueueHandle<State>,
    _compositor: wl_compositor::WlCompositor,
    shm: wl_shm::WlShm,
    surface: wl_surface::WlSurface,
    xdg_surface: xdg_surface::XdgSurface,
    _toplevel: xdg_toplevel::XdgToplevel,
    viewport: Option<wp_viewport::WpViewport>,
    _frac: Option<frac::WpFractionalScaleV1>,
    /// Logical size the window has (the last configure applied by the host).
    logical: (u32, u32),
    /// The last toplevel configure, applied at its xdg_surface configure: (w, h, suspended).
    pending: Option<(u32, u32, bool)>,
    /// A configure serial to acknowledge with the next present (its size differs from what is drawn).
    ack: Option<u32>,
    configured: bool,
    /// Buffer px per logical px: the fractional scale (120ths) if the compositor sends one, else the integer one.
    scale120: u32,
    int_scale: i32,
    has_frac: bool,
    /// The buffer scale and viewport destination last committed.
    set_scale: i32,
    set_dest: (u32, u32),
    suspended: bool,
    frame_pending: bool,
    gens: Vec<Gen>,
    next_gen: u32,
    nbufs: usize,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    ptr: (f64, f64),
    axis: (f64, f64),
    axis_120: (i32, i32),
    axis_finger: bool,
    axis_any: bool,
    xkb: Option<Xkb>,
    mods: Mods,
    events: Vec<Event>,
    /// Requests: (commits, damage rectangles) sent, for diagnostics.
    pub commits: u64,
    pub damage_rects: u64,
}

/// A connected toplevel window. See the crate docs.
pub struct Shell {
    conn: Connection,
    queue: EventQueue<State>,
    pub state: State,
}

#[derive(Debug)]
pub struct Error(pub String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
fn err(e: impl std::fmt::Display) -> Error {
    Error(e.to_string())
}

impl Shell {
    /// Connect to `$WAYLAND_DISPLAY`, make a toplevel titled `title` with application id `app_id`, asking for
    /// `w` x `h` logical px, with `nbufs` buffers in its ring, and wait for the compositor's first configure (the
    /// window is not shown until the first present).
    pub fn connect(title: &str, app_id: &str, w: u32, h: u32, nbufs: usize) -> Result<Shell, Error> {
        let conn = Connection::connect_to_env().map_err(err)?;
        let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(err)?;
        let qh = queue.handle();
        let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 4..=6, ()).map_err(err)?;
        let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).map_err(err)?;
        let wm: xdg_wm_base::XdgWmBase = globals.bind(&qh, 1..=6, ()).map_err(err)?;
        let seat: Option<wl_seat::WlSeat> = globals.bind(&qh, 1..=7, ()).ok();
        let viewporter: Option<wp_viewporter::WpViewporter> = globals.bind(&qh, 1..=1, ()).ok();
        let fmgr: Option<frac_mgr::WpFractionalScaleManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
        let surface = compositor.create_surface(&qh, ());
        let xdg_surface = wm.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg_surface.get_toplevel(&qh, ());
        toplevel.set_title(title.to_string());
        toplevel.set_app_id(app_id.to_string());
        // fractional scale needs the viewporter (the buffer is then sized in buffer px, the viewport in logical px)
        let (viewport, fr) = match (&viewporter, &fmgr) {
            (Some(v), Some(m)) => (Some(v.get_viewport(&surface, &qh, ())), Some(m.get_fractional_scale(&surface, &qh, ()))),
            _ => (None, None),
        };
        let _ = seat;
        surface.commit();
        let mut state = State {
            qh,
            _compositor: compositor,
            shm,
            surface,
            xdg_surface,
            _toplevel: toplevel,
            has_frac: fr.is_some(),
            viewport,
            _frac: fr,
            logical: (w, h),
            pending: None,
            ack: None,
            configured: false,
            scale120: 120,
            int_scale: 1,
            set_scale: 1,
            set_dest: (0, 0),
            suspended: false,
            frame_pending: false,
            gens: Vec::new(),
            next_gen: 0,
            nbufs: nbufs.max(1),
            pointer: None,
            keyboard: None,
            ptr: (0.0, 0.0),
            axis: (0.0, 0.0),
            axis_120: (0, 0),
            axis_finger: false,
            axis_any: false,
            xkb: None,
            mods: Mods::default(),
            events: Vec::new(),
            commits: 0,
            damage_rects: 0,
        };
        while !state.configured {
            queue.blocking_dispatch(&mut state).map_err(err)?;
        }
        // the preferred scale usually follows the surface's first configure: one more round trip collects it
        queue.roundtrip(&mut state).map_err(err)?;
        Ok(Shell { conn, queue, state })
    }

    /// Buffer px per logical px.
    pub fn scale(&self) -> f64 {
        self.state.scale120 as f64 / 120.0
    }
    /// The window's logical size.
    pub fn logical(&self) -> (u32, u32) {
        self.state.logical
    }
    /// The buffer size for the window's logical size at its scale.
    pub fn buffer_size(&self) -> (u32, u32) {
        let s = self.scale();
        let (w, h) = self.state.logical;
        ((w as f64 * s).round().max(1.0) as u32, (h as f64 * s).round().max(1.0) as u32)
    }
    /// A frame callback is outstanding: the compositor has not yet shown (or dropped) the last present.
    pub fn frame_pending(&self) -> bool {
        self.state.frame_pending
    }
    pub fn suspended(&self) -> bool {
        self.state.suspended
    }
    /// Decoded events since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.state.events)
    }
    /// Apply a configured logical size (the host calls this when it handles `Event::Configure`).
    pub fn set_logical(&mut self, w: u32, h: u32) {
        self.state.logical = (w, h);
    }

    /// Send what is queued, wait up to `timeout` (`None`: until something arrives) for the compositor, read
    /// and decode what came. Returns once events were decoded or the timeout passed.
    pub fn dispatch(&mut self, timeout: Option<Duration>) -> Result<(), Error> {
        self.queue.dispatch_pending(&mut self.state).map_err(err)?;
        // events already decoded (and not yet taken): read what else arrived, but do not wait
        let timeout = if self.state.events.is_empty() { timeout } else { Some(Duration::ZERO) };
        self.flush()?;
        let Some(guard) = self.queue.prepare_read() else {
            // events were queued meanwhile: decode them
            self.queue.dispatch_pending(&mut self.state).map_err(err)?;
            return Ok(());
        };
        let fd = guard.connection_fd().as_raw_fd();
        let ms = timeout.map_or(-1, |t| t.as_micros().div_ceil(1000).min(i32::MAX as u128) as i32);
        let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd.
        let n = unsafe { libc::poll(&mut p, 1, ms) };
        if n > 0 {
            match guard.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(err(e)),
            }
        } else {
            drop(guard);
        }
        self.queue.dispatch_pending(&mut self.state).map_err(err)?;
        Ok(())
    }

    /// Read and decode whatever the compositor already sent, without waiting (buffer releases while every buffer
    /// is busy).
    pub fn poll_now(&mut self) -> Result<(), Error> {
        self.dispatch(Some(Duration::ZERO))
    }

    pub fn flush(&self) -> Result<(), Error> {
        match self.conn.flush() {
            Ok(()) => Ok(()),
            Err(wayland_client::backend::WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
            Err(e) => Err(err(e)),
        }
    }

    // ------------------------------------------------------------------------------------------------ buffers

    /// Buffers in the ring.
    pub fn count(&self) -> usize {
        self.state.nbufs
    }
    /// The current buffers' size in px (0 x 0 before the first `resize_buffers`).
    pub fn size(&self) -> (u32, u32) {
        self.state.gens.last().map_or((0, 0), |g| (g.w, g.h))
    }
    /// The compositor holds buffer `i` (attached and not yet released): do not draw into it.
    pub fn in_use(&self, i: usize) -> bool {
        self.state.gens.last().is_some_and(|g| g.busy[i])
    }
    /// Buffer `i`'s pixels (XRGB8888 little-endian: B, G, R, X bytes): (base, bytes per row). Valid until the
    /// next `resize_buffers`.
    pub fn pixels(&mut self, i: usize) -> (*mut u8, usize) {
        let g = self.state.gens.last().expect("pixels before resize_buffers");
        // SAFETY: buffer `i` occupies [i * one, i * one + stride * h) of the mapping, one = stride * h rounded
        // up to a page (`resize_buffers`).
        (unsafe { g.map.add(i * (g.stride * g.h as usize).next_multiple_of(4096)) }, g.stride)
    }

    /// Make a new generation of buffers of `w` x `h` px (the old one is destroyed once the compositor released
    /// every buffer of it). A buffer's pages exist only once it is first drawn.
    pub fn resize_buffers(&mut self, w: u32, h: u32) -> Result<(), Error> {
        if self.state.gens.last().is_some_and(|g| (g.w, g.h) == (w, h)) {
            return Ok(());
        }
        let st = &mut self.state;
        let stride = w as usize * 4;
        // each buffer starts on a page: a GPU executor imports a buffer's pages as its output (Vulkan's
        // host-pointer import is page granular), and a page-aligned buffer is also the cheapest to copy
        let one = (stride * h as usize).next_multiple_of(4096);
        let len = one * st.nbufs;
        let name = c"bough-shm";
        // SAFETY: a NUL-terminated name; the fd is owned below.
        let raw = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
        if raw < 0 {
            return Err(Error(format!("memfd_create: {}", std::io::Error::last_os_error())));
        }
        // SAFETY: a fresh fd this function owns.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        // SAFETY: sizing our own memfd.
        if unsafe { libc::ftruncate(fd.as_raw_fd(), len as libc::off_t) } != 0 {
            return Err(Error(format!("ftruncate: {}", std::io::Error::last_os_error())));
        }
        // SAFETY: a shared read-write mapping of the whole memfd; unmapped by `Gen::destroy`.
        let map = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd.as_raw_fd(), 0) };
        if map == libc::MAP_FAILED {
            return Err(Error(format!("mmap: {}", std::io::Error::last_os_error())));
        }
        let id = st.next_gen;
        st.next_gen += 1;
        let pool = st.shm.create_pool(fd.as_fd(), len as i32, &st.qh, ());
        let bufs = (0..st.nbufs).map(|i| pool.create_buffer((i * one) as i32, w as i32, h as i32, stride as i32, wl_shm::Format::Xrgb8888, &st.qh, (id, i))).collect();
        st.gens.push(Gen { id, w, h, stride, map: map as *mut u8, len, _fd: fd, pool, bufs, busy: vec![false; st.nbufs] });
        st.reap();
        Ok(())
    }

    /// Show buffer `i`: attach it, damage the 32 px tiles set in `damage` (bit `tx` of `damage[ty]`; `full`:
    /// everything), request a frame callback (unless suspended), acknowledge a pending configure, commit.
    pub fn present(&mut self, i: usize, damage: &[u128], full: bool) -> Result<(), Error> {
        let st = &mut self.state;
        let (w, h, buf) = {
            let g = st.gens.last_mut().expect("present before resize_buffers");
            g.busy[i] = true;
            (g.w as i32, g.h as i32, g.bufs[i].clone())
        };
        if let Some(serial) = st.ack.take() {
            st.xdg_surface.ack_configure(serial);
        }
        if st.has_frac {
            if st.set_dest != st.logical {
                st.set_dest = st.logical;
                if let Some(v) = &st.viewport {
                    v.set_destination(st.logical.0 as i32, st.logical.1 as i32);
                }
            }
        } else if st.set_scale != st.int_scale {
            st.set_scale = st.int_scale;
            st.surface.set_buffer_scale(st.int_scale);
        }
        st.surface.attach(Some(&buf), 0, 0);
        if full {
            st.surface.damage_buffer(0, 0, w, h);
            st.damage_rects += 1;
        } else {
            // one rectangle per run of damaged tiles in a row, extended down while the next rows have the same run
            let rows = damage.len().min((h as usize).div_ceil(32));
            let mut open: Vec<(u32, u32, usize, usize)> = Vec::new(); // (tx0, tx1, ty0, ty1)
            let flush = |r: (u32, u32, usize, usize), st: &mut State| {
                let (x0, y0) = (r.0 as i32 * 32, r.2 as i32 * 32);
                let (x1, y1) = ((r.1 as i32 * 32).min(w), (r.3 as i32 * 32).min(h));
                st.surface.damage_buffer(x0, y0, x1 - x0, y1 - y0);
                st.damage_rects += 1;
            };
            for (ty, &bits) in damage[..rows].iter().enumerate() {
                let mut runs: Vec<(u32, u32)> = Vec::new();
                let mut b = bits;
                while b != 0 {
                    let a = b.trailing_zeros();
                    let len = (!(b >> a)).trailing_zeros();
                    runs.push((a, a + len));
                    b &= if a + len >= 128 { 0 } else { !0u128 << (a + len) };
                }
                let mut next = Vec::new();
                for o in open.drain(..) {
                    if o.3 == ty && runs.contains(&(o.0, o.1)) {
                        next.push((o.0, o.1, o.2, ty + 1));
                    } else {
                        flush(o, st);
                    }
                }
                for r in runs {
                    if !next.iter().any(|n| (n.0, n.1) == r && n.3 == ty + 1) {
                        next.push((r.0, r.1, ty, ty + 1));
                    }
                }
                open = next;
            }
            for o in open {
                flush(o, st);
            }
        }
        if !st.suspended {
            st.surface.frame(&st.qh, ());
            st.frame_pending = true;
        }
        st.surface.commit();
        st.commits += 1;
        self.flush()
    }
}

impl State {
    /// Destroy the old generations whose buffers the compositor released.
    fn reap(&mut self) {
        let last = self.gens.len().saturating_sub(1);
        let mut k = 0;
        while k < last.min(self.gens.len()) {
            if self.gens[k].busy.iter().all(|b| !b) && k + 1 < self.gens.len() {
                self.gens.remove(k).destroy();
            } else {
                k += 1;
            }
        }
    }

    fn keymap(&mut self, fd: OwnedFd, size: u32) {
        let Some(x) = xkbcommon_dl::xkbcommon_option() else {
            eprintln!("bough-wayland: libxkbcommon not found; keyboard input disabled");
            return;
        };
        // SAFETY: a private read-only mapping of the keymap fd the compositor sent, `size` bytes (NUL-terminated
        // text); unmapped below once compiled.
        let p = unsafe { libc::mmap(std::ptr::null_mut(), size as usize, libc::PROT_READ, libc::MAP_PRIVATE, fd.as_raw_fd(), 0) };
        if p == libc::MAP_FAILED {
            return;
        }
        // SAFETY: libxkbcommon's documented API; the string is the mapped keymap; every object made here is owned by
        // `Xkb` (never freed: one keyboard per process lifetime, replaced keymaps leak a few KB).
        unsafe {
            let ctx = self.xkb.as_ref().map_or_else(|| (x.xkb_context_new)(xkbcommon_dl::xkb_context_flags::XKB_CONTEXT_NO_FLAGS), |k| k.ctx);
            let keymap = (x.xkb_keymap_new_from_string)(ctx, p as *const libc::c_char, xkbcommon_dl::xkb_keymap_format::XKB_KEYMAP_FORMAT_TEXT_V1, xkbcommon_dl::xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS);
            libc::munmap(p, size as usize);
            if keymap.is_null() {
                return;
            }
            let state = (x.xkb_state_new)(keymap);
            self.xkb = Some(Xkb { ctx, _keymap: keymap, state });
        }
    }

    fn key(&mut self, code: u32, pressed: bool) {
        let (Some(x), Some(k)) = (xkbcommon_dl::xkbcommon_option(), self.xkb.as_ref()) else { return };
        // SAFETY: a live xkb state; evdev code + 8 is the xkb keycode.
        let sym = unsafe { (x.xkb_state_key_get_one_sym)(k.state, code + 8) };
        // SAFETY: as above.
        let cp = unsafe { (x.xkb_keysym_to_utf32)(sym) };
        let text = char::from_u32(cp).filter(|c| cp != 0 && !c.is_control());
        self.events.push(Event::Key { sym, text, pressed, mods: self.mods });
    }

    fn modifiers(&mut self, dep: u32, lat: u32, lock: u32, group: u32) {
        let (Some(x), Some(k)) = (xkbcommon_dl::xkbcommon_option(), self.xkb.as_ref()) else { return };
        // SAFETY: a live xkb state; the masks are the compositor's.
        unsafe {
            (x.xkb_state_update_mask)(k.state, dep, lat, lock, 0, 0, group);
            let on = |n: &[u8]| (x.xkb_state_mod_name_is_active)(k.state, n.as_ptr() as *const libc::c_char, xkbcommon_dl::xkb_state_component::XKB_STATE_MODS_EFFECTIVE) > 0;
            self.mods = Mods { shift: on(xkbcommon_dl::XKB_MOD_NAME_SHIFT), ctrl: on(xkbcommon_dl::XKB_MOD_NAME_CTRL), alt: on(xkbcommon_dl::XKB_MOD_NAME_ALT), logo: on(xkbcommon_dl::XKB_MOD_NAME_LOGO) };
        }
    }

    fn set_scale120(&mut self, s: u32) {
        if s != self.scale120 && s > 0 {
            self.scale120 = s;
            self.events.push(Event::Scale(s as f64 / 120.0));
        }
    }
}

// ------------------------------------------------------------------------------------------------- dispatch

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wp_viewporter::WpViewporter);
delegate_noop!(State: ignore wp_viewport::WpViewport);
delegate_noop!(State: ignore frac_mgr::WpFractionalScaleManagerV1);

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for State {
    fn event(_: &mut Self, wm: &xdg_wm_base::XdgWmBase, ev: xdg_wm_base::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let xdg_wm_base::Event::Ping { serial } = ev {
            wm.pong(serial);
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for State {
    fn event(st: &mut Self, _: &xdg_toplevel::XdgToplevel, ev: xdg_toplevel::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            xdg_toplevel::Event::Configure { width, height, states } => {
                let suspended = states.chunks_exact(4).any(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]) == xdg_toplevel::State::Suspended as u32);
                st.pending = Some((width.max(0) as u32, height.max(0) as u32, suspended));
            }
            xdg_toplevel::Event::Close => st.events.push(Event::Close),
            _ => {}
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for State {
    fn event(st: &mut Self, s: &xdg_surface::XdgSurface, ev: xdg_surface::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let xdg_surface::Event::Configure { serial } = ev else { return };
        let (w, h, suspended) = st.pending.take().unwrap_or((0, 0, st.suspended));
        // 0 leaves the size to the client: keep the current one (the requested one at first)
        let size = (if w == 0 { st.logical.0.max(1) } else { w }, if h == 0 { st.logical.1.max(1) } else { h });
        if suspended != st.suspended {
            st.suspended = suspended;
            st.events.push(Event::Suspended(suspended));
        }
        if !st.configured || size != st.logical {
            // acknowledged with the first commit drawn at this size
            st.ack = Some(serial);
            st.events.push(Event::Configure { w: size.0, h: size.1 });
            if !st.configured {
                st.logical = size;
            }
        } else {
            s.ack_configure(serial);
        }
        st.configured = true;
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for State {
    fn event(st: &mut Self, _: &wl_surface::WlSurface, ev: wl_surface::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_surface::Event::PreferredBufferScale { factor } = ev {
            st.int_scale = factor.max(1);
            if !st.has_frac {
                st.set_scale120(factor.max(1) as u32 * 120);
            }
        }
    }
}

impl Dispatch<frac::WpFractionalScaleV1, ()> for State {
    fn event(st: &mut Self, _: &frac::WpFractionalScaleV1, ev: frac::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let frac::Event::PreferredScale { scale } = ev {
            st.set_scale120(scale);
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for State {
    fn event(st: &mut Self, _: &wl_callback::WlCallback, ev: wl_callback::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_callback::Event::Done { callback_data } = ev {
            st.frame_pending = false;
            st.events.push(Event::Frame { time_ms: callback_data });
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, (u32, usize)> for State {
    fn event(st: &mut Self, _: &wl_buffer::WlBuffer, ev: wl_buffer::Event, &(id, i): &(u32, usize), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_buffer::Event::Release = ev {
            if let Some(g) = st.gens.iter_mut().find(|g| g.id == id) {
                g.busy[i] = false;
            }
            st.reap();
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(st: &mut Self, seat: &wl_seat::WlSeat, ev: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(c) } = ev {
            if c.contains(wl_seat::Capability::Pointer) && st.pointer.is_none() {
                st.pointer = Some(seat.get_pointer(qh, ()));
            }
            if c.contains(wl_seat::Capability::Keyboard) && st.keyboard.is_none() {
                st.keyboard = Some(seat.get_keyboard(qh, ()));
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for State {
    fn event(st: &mut Self, p: &wl_pointer::WlPointer, ev: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        use wl_pointer::Event as E;
        let frames = p.version() >= 5;
        match ev {
            E::Enter { surface_x, surface_y, .. } | E::Motion { surface_x, surface_y, .. } => {
                st.ptr = (surface_x, surface_y);
                st.events.push(Event::Motion { x: surface_x, y: surface_y });
            }
            E::Leave { .. } => st.events.push(Event::Leave),
            E::Button { button, state, .. } => {
                let pressed = matches!(state, WEnum::Value(wl_pointer::ButtonState::Pressed));
                st.events.push(Event::Button { x: st.ptr.0, y: st.ptr.1, button, pressed });
            }
            E::AxisSource { axis_source } => st.axis_finger = matches!(axis_source, WEnum::Value(wl_pointer::AxisSource::Finger | wl_pointer::AxisSource::Continuous)),
            E::Axis { axis, value, .. } => {
                match axis {
                    WEnum::Value(wl_pointer::Axis::VerticalScroll) => st.axis.1 += value,
                    WEnum::Value(wl_pointer::Axis::HorizontalScroll) => st.axis.0 += value,
                    _ => {}
                }
                st.axis_any = true;
                if !frames {
                    st.end_axis();
                }
            }
            E::AxisValue120 { axis, value120 } => {
                match axis {
                    WEnum::Value(wl_pointer::Axis::VerticalScroll) => st.axis_120.1 += value120,
                    WEnum::Value(wl_pointer::Axis::HorizontalScroll) => st.axis_120.0 += value120,
                    _ => {}
                }
                st.axis_any = true;
            }
            E::Frame => st.end_axis(),
            _ => {}
        }
    }
}

impl State {
    /// The end of a pointer frame: its scroll as one event.
    fn end_axis(&mut self) {
        if !self.axis_any {
            return;
        }
        let (dx, dy) = if self.axis_finger {
            self.axis
        } else if self.axis_120 != (0, 0) {
            (self.axis_120.0 as f64 / 120.0, self.axis_120.1 as f64 / 120.0)
        } else {
            // a wheel without value120: the axis value is ~15 per detent (libinput's default)
            (self.axis.0 / 15.0, self.axis.1 / 15.0)
        };
        if dx != 0.0 || dy != 0.0 {
            self.events.push(Event::Scroll { x: self.ptr.0, y: self.ptr.1, dx, dy, finger: self.axis_finger });
        }
        self.axis = (0.0, 0.0);
        self.axis_120 = (0, 0);
        self.axis_finger = false;
        self.axis_any = false;
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for State {
    fn event(st: &mut Self, _: &wl_keyboard::WlKeyboard, ev: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        use wl_keyboard::Event as E;
        match ev {
            E::Keymap { format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1), fd, size } => st.keymap(fd, size),
            E::Key { key, state, .. } => st.key(key, matches!(state, WEnum::Value(wl_keyboard::KeyState::Pressed))),
            E::Modifiers { mods_depressed, mods_latched, mods_locked, group, .. } => st.modifiers(mods_depressed, mods_latched, mods_locked, group),
            _ => {}
        }
    }
}

// keysyms the host maps to bough's keys
pub mod keysym {
    pub const BACKSPACE: u32 = 0xff08;
    pub const TAB: u32 = 0xff09;
    pub const RETURN: u32 = 0xff0d;
    pub const ESCAPE: u32 = 0xff1b;
    pub const HOME: u32 = 0xff50;
    pub const LEFT: u32 = 0xff51;
    pub const UP: u32 = 0xff52;
    pub const RIGHT: u32 = 0xff53;
    pub const DOWN: u32 = 0xff54;
    pub const PAGE_UP: u32 = 0xff55;
    pub const PAGE_DOWN: u32 = 0xff56;
    pub const END: u32 = 0xff57;
    pub const KP_ENTER: u32 = 0xff8d;
    pub const ISO_LEFT_TAB: u32 = 0xfe20;
    /// F1; F2..F35 follow.
    pub const F1: u32 = 0xffbe;
    pub const F35: u32 = 0xffe0;
    pub const DELETE: u32 = 0xffff;
}
