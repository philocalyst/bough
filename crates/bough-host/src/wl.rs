//! A Wayland toplevel on the wire (Linux).
//!
//! No libwayland, no protocol crate, no event-loop framework, no allocation per message: requests are encoded
//! into one output buffer and leave in one `sendmsg`, events are decoded in place from one input buffer. The
//! window chooses every request and every flush, which is what lets the start-up chain be what it is:
//!
//! 1. `connect`; **write 1**: `wl_display.get_registry`, `wl_display.sync`.
//! 2. (the globals arrive) **write 2**: every bind, `create_surface`, `get_xdg_surface`, `get_toplevel`, title,
//!    application id, the first (bufferless) `commit`.
//! 3. (the configure arrives) **write 3**: `ack_configure`, `create_pool`, `create_buffer`, `attach`,
//!    `damage_buffer`, `commit`.
//!
//! Two round trips, three writes, and the application draws its first frame between write 2 and the configure,
//! into shared memory that needs nothing from the compositor to exist. No frame callback is asked for unless
//! something is in continuous motion.
//!
//! Protocols spoken: `wl_display`, `wl_registry`, `wl_compositor` (v4..6), `wl_shm`, `wl_shm_pool`, `wl_buffer`,
//! `wl_surface`, `wl_callback`, `xdg_wm_base` (v1..6), `xdg_surface`, `xdg_toplevel`, `wl_seat` (v1..9),
//! `wl_pointer`, `wl_keyboard`, `wp_viewporter`, `wp_viewport`, `wp_fractional_scale_manager_v1`,
//! `wp_fractional_scale_v1`. Opcodes are written out below with the request or event they are.
use crate::event::{Event, Mods, Sym};
use crate::keymap::Keymap;
use std::os::fd::RawFd;

#[derive(Debug)]
pub struct Error(pub String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
fn os(what: &str) -> Error {
    Error(format!("{what}: {}", std::io::Error::last_os_error()))
}

/// What an object id is, so an event can be decoded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Free,
    Display,
    Registry,
    Sync,
    FrameCb,
    Compositor,
    Shm,
    WmBase,
    Seat,
    Viewporter,
    FracMgr,
    Surface,
    XdgSurface,
    Toplevel,
    Viewport,
    Frac,
    Pool,
    /// A buffer: (generation, index in the ring).
    Buffer(u32, u32),
    Pointer,
    Keyboard,
    /// Destroyed by us; the server may still send events for it until it confirms with `delete_id`.
    Dead,
}

/// What the wire layer reports besides input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wl {
    /// The compositor configured the window: logical size (0: ours to choose) and whether it is suspended.
    Configure { w: i32, h: i32, suspended: bool },
    /// Preferred scale in 120ths.
    Scale(u32),
    /// A frame callback fired.
    Frame,
    /// Buffer `index` of generation `generation` was released.
    Release { generation: u32, index: u32 },
    Input(Event),
}

const IN: usize = 1 << 16;

pub struct Conn {
    fd: RawFd,
    out: Vec<u8>,
    out_fds: Vec<RawFd>,
    inb: Box<[u8; IN]>,
    in_len: usize,
    in_at: usize,
    in_fds: Vec<RawFd>,
    kinds: Vec<Kind>,
    free: Vec<u32>,
    // globals: (name, version)
    g_compositor: (u32, u32),
    g_shm: (u32, u32),
    g_wm: (u32, u32),
    g_seat: (u32, u32),
    g_viewporter: (u32, u32),
    g_frac: (u32, u32),
    // objects
    compositor: u32,
    shm: u32,
    wm: u32,
    seat: u32,
    surface: u32,
    xdg_surface: u32,
    toplevel: u32,
    viewport: u32,
    pointer: u32,
    keyboard: u32,
    seat_version: u32,
    surface_version: u32,
    sync_done: bool,
    // toplevel configure state
    pend: (i32, i32, bool),
    pub suspended: bool,
    /// The serial of a configure not yet acknowledged.
    pub ack: Option<u32>,
    // input state
    ptr: (f64, f64),
    axis: (f64, f64),
    axis120: (i32, i32),
    axis_any: bool,
    axis_finger: bool,
    pub mods: Mods,
    lock: bool,
    keymap_fd: Option<(RawFd, u32)>,
    keymap: Option<Keymap>,
    /// Buffer pixels per logical pixel, for input coordinates.
    pub scale: f64,
    /// Counters.
    pub writes: u64,
    pub reads: u64,
    pub commits: u64,
}

fn pad(n: usize) -> usize {
    (n + 3) & !3
}

impl Conn {
    /// Connect to `$WAYLAND_DISPLAY` in `$XDG_RUNTIME_DIR` (or `$WAYLAND_SOCKET`, an inherited descriptor) and
    /// queue the registry request and a sync. Nothing is sent until [`Conn::flush`].
    pub fn connect() -> Result<Conn, Error> {
        let fd = if let Some(s) = std::env::var_os("WAYLAND_SOCKET") {
            s.to_str().and_then(|s| s.parse::<RawFd>().ok()).ok_or_else(|| Error("WAYLAND_SOCKET is not a descriptor".into()))?
        } else {
            let name = std::env::var_os("WAYLAND_DISPLAY").unwrap_or_else(|| "wayland-0".into());
            let mut path = std::path::PathBuf::new();
            if !std::path::Path::new(&name).is_absolute() {
                path.push(std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| Error("XDG_RUNTIME_DIR is not set".into()))?);
            }
            path.push(name);
            let bytes = std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str());
            // SAFETY: plain socket calls on a descriptor this function owns; `addr` is a zeroed `sockaddr_un`
            // filled with a path shorter than its buffer.
            unsafe {
                let fd = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
                if fd < 0 {
                    return Err(os("socket"));
                }
                let mut addr: libc::sockaddr_un = core::mem::zeroed();
                addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
                if bytes.len() >= addr.sun_path.len() {
                    return Err(Error("the socket path is too long".into()));
                }
                for (d, s) in addr.sun_path.iter_mut().zip(bytes) {
                    *d = *s as libc::c_char;
                }
                let len = (core::mem::size_of::<libc::sa_family_t>() + bytes.len() + 1) as libc::socklen_t;
                if libc::connect(fd, &addr as *const _ as *const libc::sockaddr, len) != 0 {
                    return Err(os("connect"));
                }
                fd
            }
        };
        let mut c = Conn {
            fd,
            out: Vec::with_capacity(4096),
            out_fds: Vec::new(),
            // SAFETY: a byte array is valid zeroed.
            inb: unsafe { Box::new_zeroed().assume_init() },
            in_len: 0,
            in_at: 0,
            in_fds: Vec::new(),
            kinds: vec![Kind::Free, Kind::Display],
            free: Vec::new(),
            g_compositor: (0, 0),
            g_shm: (0, 0),
            g_wm: (0, 0),
            g_seat: (0, 0),
            g_viewporter: (0, 0),
            g_frac: (0, 0),
            compositor: 0,
            shm: 0,
            wm: 0,
            seat: 0,
            surface: 0,
            xdg_surface: 0,
            toplevel: 0,
            viewport: 0,
            pointer: 0,
            keyboard: 0,
            seat_version: 0,
            surface_version: 0,
            sync_done: false,
            pend: (0, 0, false),
            suspended: false,
            ack: None,
            ptr: (0.0, 0.0),
            axis: (0.0, 0.0),
            axis120: (0, 0),
            axis_any: false,
            axis_finger: false,
            mods: Mods::default(),
            lock: false,
            keymap_fd: None,
            keymap: None,
            scale: 1.0,
            writes: 0,
            reads: 0,
            commits: 0,
        };
        let reg = c.new_id(Kind::Registry);
        c.req(1, 1, |b| put32(b, reg)); // wl_display.get_registry(new_id)
        let cb = c.new_id(Kind::Sync);
        c.req(1, 0, |b| put32(b, cb)); // wl_display.sync(new_id)
        Ok(c)
    }
    pub fn fd(&self) -> RawFd {
        self.fd
    }

    fn new_id(&mut self, k: Kind) -> u32 {
        if let Some(i) = self.free.pop() {
            self.kinds[i as usize] = k;
            i
        } else {
            self.kinds.push(k);
            self.kinds.len() as u32 - 1
        }
    }
    /// Encode one request: header, then whatever `f` writes.
    #[inline]
    fn req(&mut self, obj: u32, op: u16, f: impl FnOnce(&mut Vec<u8>)) {
        let at = self.out.len();
        put32(&mut self.out, obj);
        put32(&mut self.out, 0);
        f(&mut self.out);
        let size = (self.out.len() - at) as u32;
        self.out[at + 4..at + 8].copy_from_slice(&(size << 16 | op as u32).to_ne_bytes());
    }

    /// Send everything queued, in one `sendmsg` (with the descriptors queued, if any).
    pub fn flush(&mut self) -> Result<(), Error> {
        let mut at = 0;
        while at < self.out.len() {
            // SAFETY: a `msghdr` over our own buffers; the control buffer holds the queued descriptors.
            let n = unsafe {
                let mut iov = libc::iovec { iov_base: self.out.as_ptr().add(at) as *mut libc::c_void, iov_len: self.out.len() - at };
                let mut msg: libc::msghdr = core::mem::zeroed();
                msg.msg_iov = &mut iov;
                msg.msg_iovlen = 1;
                let mut cbuf = [0u64; 16];
                if !self.out_fds.is_empty() {
                    let bytes = self.out_fds.len() * 4;
                    msg.msg_control = cbuf.as_mut_ptr() as *mut libc::c_void;
                    msg.msg_controllen = libc::CMSG_SPACE(bytes as u32) as _;
                    let cm = libc::CMSG_FIRSTHDR(&msg);
                    (*cm).cmsg_level = libc::SOL_SOCKET;
                    (*cm).cmsg_type = libc::SCM_RIGHTS;
                    (*cm).cmsg_len = libc::CMSG_LEN(bytes as u32) as _;
                    core::ptr::copy_nonoverlapping(self.out_fds.as_ptr() as *const u8, libc::CMSG_DATA(cm), bytes);
                }
                libc::sendmsg(self.fd, &msg, libc::MSG_NOSIGNAL)
            };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                match e.kind() {
                    std::io::ErrorKind::Interrupted => continue,
                    std::io::ErrorKind::WouldBlock => {
                        // the compositor is not reading: wait until it does (it must, to make progress)
                        let mut p = libc::pollfd { fd: self.fd, events: libc::POLLOUT, revents: 0 };
                        // SAFETY: one valid pollfd.
                        unsafe { libc::poll(&mut p, 1, -1) };
                        continue;
                    }
                    _ => return Err(Error(format!("sendmsg: {e}"))),
                }
            }
            self.out_fds.clear();
            at += n as usize;
            self.writes += 1;
        }
        self.out.clear();
        Ok(())
    }
    pub fn queued(&self) -> bool {
        !self.out.is_empty()
    }

    /// Read what the compositor sent. `wait`: block until something arrives. Returns whether anything was read.
    pub fn read(&mut self, wait: bool) -> Result<bool, Error> {
        // keep an incomplete message at the front
        if self.in_at > 0 {
            self.inb.copy_within(self.in_at..self.in_len, 0);
            self.in_len -= self.in_at;
            self.in_at = 0;
        }
        loop {
            // SAFETY: a `msghdr` over our own buffers.
            let (n, fds) = unsafe {
                let mut iov = libc::iovec { iov_base: self.inb.as_mut_ptr().add(self.in_len) as *mut libc::c_void, iov_len: IN - self.in_len };
                let mut cbuf = [0u64; 32];
                let mut msg: libc::msghdr = core::mem::zeroed();
                msg.msg_iov = &mut iov;
                msg.msg_iovlen = 1;
                msg.msg_control = cbuf.as_mut_ptr() as *mut libc::c_void;
                msg.msg_controllen = core::mem::size_of_val(&cbuf) as _;
                let n = libc::recvmsg(self.fd, &mut msg, libc::MSG_CMSG_CLOEXEC | if wait { 0 } else { libc::MSG_DONTWAIT });
                let mut fds = [-1 as RawFd; 28];
                let mut nf = 0;
                if n > 0 {
                    let mut cm = libc::CMSG_FIRSTHDR(&msg);
                    while !cm.is_null() {
                        if (*cm).cmsg_level == libc::SOL_SOCKET && (*cm).cmsg_type == libc::SCM_RIGHTS {
                            let bytes = (*cm).cmsg_len as usize - libc::CMSG_LEN(0) as usize;
                            let p = libc::CMSG_DATA(cm) as *const RawFd;
                            for k in 0..bytes / 4 {
                                if nf < fds.len() {
                                    fds[nf] = core::ptr::read_unaligned(p.add(k));
                                    nf += 1;
                                }
                            }
                        }
                        cm = libc::CMSG_NXTHDR(&msg, cm);
                    }
                }
                (n, (fds, nf))
            };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                return match e.kind() {
                    std::io::ErrorKind::Interrupted => continue,
                    std::io::ErrorKind::WouldBlock => Ok(false),
                    _ => Err(Error(format!("recvmsg: {e}"))),
                };
            }
            if n == 0 {
                return Err(Error("the compositor closed the connection".into()));
            }
            self.in_len += n as usize;
            self.in_fds.extend_from_slice(&fds.0[..fds.1]);
            self.reads += 1;
            return Ok(true);
        }
    }

    /// Decode the next complete event in the input buffer, if any, into `out`. Events that only change the
    /// connection's own state produce nothing.
    pub fn next(&mut self, out: &mut Vec<Wl>) -> Result<bool, Error> {
        if self.in_len - self.in_at < 8 {
            return Ok(false);
        }
        let b = &self.inb[self.in_at..self.in_len];
        let obj = get32(b, 0);
        let word = get32(b, 4);
        let (size, op) = ((word >> 16) as usize, word & 0xffff);
        if size < 8 || size > IN {
            return Err(Error(format!("a malformed event (object {obj}, size {size})")));
        }
        if b.len() < size {
            return Ok(false);
        }
        let at = self.in_at;
        self.in_at += size;
        let kind = self.kinds.get(obj as usize).copied().unwrap_or(Kind::Free);
        self.event(kind, obj, op, at + 8, size - 8, out)?;
        Ok(true)
    }

    fn a(&self, at: usize, i: usize) -> u32 {
        get32(&self.inb[..], at + 4 * i)
    }
    fn fixed(&self, at: usize, i: usize) -> f64 {
        self.a(at, i) as i32 as f64 / 256.0
    }

    fn event(&mut self, kind: Kind, obj: u32, op: u32, at: usize, len: usize, out: &mut Vec<Wl>) -> Result<(), Error> {
        match (kind, op) {
            // wl_display.error(object, code, message)
            (Kind::Display, 0) => {
                let (o, code, n) = (self.a(at, 0), self.a(at, 1), self.a(at, 2) as usize);
                let msg = String::from_utf8_lossy(&self.inb[at + 12..at + 12 + n.saturating_sub(1).min(len.saturating_sub(12))]).into_owned();
                return Err(Error(format!("protocol error on object {o} ({:?}), code {code}: {msg}", self.kinds.get(o as usize))));
            }
            // wl_display.delete_id(id)
            (Kind::Display, 1) => {
                let id = self.a(at, 0);
                if let Some(k) = self.kinds.get_mut(id as usize) {
                    *k = Kind::Free;
                    self.free.push(id);
                }
            }
            // wl_registry.global(name, interface, version)
            (Kind::Registry, 0) => {
                let (name, n) = (self.a(at, 0), self.a(at, 1) as usize);
                let iface = &self.inb[at + 8..at + 8 + n.saturating_sub(1)];
                let ver = get32(&self.inb[..], at + 8 + pad(n));
                let slot = match iface {
                    b"wl_compositor" => &mut self.g_compositor,
                    b"wl_shm" => &mut self.g_shm,
                    b"xdg_wm_base" => &mut self.g_wm,
                    b"wl_seat" => &mut self.g_seat,
                    b"wp_viewporter" => &mut self.g_viewporter,
                    b"wp_fractional_scale_manager_v1" => &mut self.g_frac,
                    _ => return Ok(()),
                };
                if slot.1 == 0 {
                    *slot = (name, ver);
                }
            }
            // wl_callback.done
            (Kind::Sync, 0) => self.sync_done = true,
            (Kind::FrameCb, 0) => out.push(Wl::Frame),
            // xdg_wm_base.ping(serial)
            (Kind::WmBase, 0) => {
                let serial = self.a(at, 0);
                let wm = self.wm;
                self.req(wm, 3, |b| put32(b, serial)); // xdg_wm_base.pong
            }
            // xdg_toplevel.configure(width, height, states)
            (Kind::Toplevel, 0) => {
                let (w, h, n) = (self.a(at, 0) as i32, self.a(at, 1) as i32, self.a(at, 2) as usize);
                let mut suspended = false;
                for k in 0..n / 4 {
                    suspended |= self.a(at + 12, k) == 9; // xdg_toplevel.state.suspended
                }
                self.pend = (w, h, suspended);
            }
            // xdg_toplevel.close
            (Kind::Toplevel, 1) => out.push(Wl::Input(Event::Close)),
            // xdg_surface.configure(serial)
            (Kind::XdgSurface, 0) => {
                self.ack = Some(self.a(at, 0));
                self.suspended = self.pend.2;
                out.push(Wl::Configure { w: self.pend.0, h: self.pend.1, suspended: self.pend.2 });
            }
            // wl_surface.preferred_buffer_scale(factor)
            (Kind::Surface, 2) => {
                if self.viewport == 0 {
                    out.push(Wl::Scale(self.a(at, 0).max(1) * 120));
                }
            }
            // wp_fractional_scale_v1.preferred_scale(scale in 120ths)
            (Kind::Frac, 0) => out.push(Wl::Scale(self.a(at, 0))),
            // wl_buffer.release
            (Kind::Buffer(generation, index), 0) => out.push(Wl::Release { generation, index }),
            // wl_seat.capabilities(bits: pointer 1, keyboard 2)
            (Kind::Seat, 0) => {
                let caps = self.a(at, 0);
                let seat = self.seat;
                if caps & 1 != 0 && self.pointer == 0 {
                    let id = self.new_id(Kind::Pointer);
                    self.pointer = id;
                    self.req(seat, 0, |b| put32(b, id)); // wl_seat.get_pointer
                }
                if caps & 2 != 0 && self.keyboard == 0 {
                    let id = self.new_id(Kind::Keyboard);
                    self.keyboard = id;
                    self.req(seat, 1, |b| put32(b, id)); // wl_seat.get_keyboard
                }
            }
            // wl_pointer.enter(serial, surface, x, y) / motion(time, x, y)
            (Kind::Pointer, 0) | (Kind::Pointer, 2) => {
                let k = if op == 0 { 2 } else { 1 };
                self.ptr = (self.fixed(at, k) * self.scale, self.fixed(at, k + 1) * self.scale);
                out.push(Wl::Input(Event::Motion { x: self.ptr.0, y: self.ptr.1 }));
            }
            // wl_pointer.leave
            (Kind::Pointer, 1) => out.push(Wl::Input(Event::Leave)),
            // wl_pointer.button(serial, time, button, state)
            (Kind::Pointer, 3) => {
                let button = match self.a(at, 2) {
                    crate::event::BTN_RIGHT => 1,
                    crate::event::BTN_MIDDLE => 2,
                    _ => 0,
                };
                out.push(Wl::Input(Event::Button { x: self.ptr.0, y: self.ptr.1, button, pressed: self.a(at, 3) == 1 }));
            }
            // wl_pointer.axis(time, axis, value)
            (Kind::Pointer, 4) => {
                let v = self.fixed(at, 2);
                if self.a(at, 1) == 0 {
                    self.axis.1 += v
                } else {
                    self.axis.0 += v
                }
                self.axis_any = true;
                if self.seat_version < 5 {
                    self.end_axis(out);
                }
            }
            // wl_pointer.frame
            (Kind::Pointer, 5) => self.end_axis(out),
            // wl_pointer.axis_source(source: wheel 0, finger 1, continuous 2)
            (Kind::Pointer, 6) => self.axis_finger = matches!(self.a(at, 0), 1 | 2),
            // wl_pointer.axis_value120(axis, value120)
            (Kind::Pointer, 9) => {
                let v = self.a(at, 1) as i32;
                if self.a(at, 0) == 0 {
                    self.axis120.1 += v
                } else {
                    self.axis120.0 += v
                }
                self.axis_any = true;
            }
            // wl_keyboard.keymap(format, fd, size)
            (Kind::Keyboard, 0) => {
                if let Some(fd) = (!self.in_fds.is_empty()).then(|| self.in_fds.remove(0)) {
                    if let Some((old, _)) = self.keymap_fd.replace((fd, self.a(at, 1))) {
                        // SAFETY: a descriptor this connection received and owns.
                        unsafe { libc::close(old) };
                    }
                    // compiled when the first key arrives: not on the path to the first frame
                    self.keymap = None;
                }
            }
            // wl_keyboard.enter / leave
            (Kind::Keyboard, 1) => out.push(Wl::Input(Event::Focus(true))),
            (Kind::Keyboard, 2) => out.push(Wl::Input(Event::Focus(false))),
            // wl_keyboard.key(serial, time, key, state)
            (Kind::Keyboard, 3) => {
                let (code, pressed) = (self.a(at, 2), self.a(at, 3) == 1);
                self.load_keymap();
                if let Some(km) = &self.keymap
                    && let Some(sym) = km.sym(code + 8, self.mods.shift, self.lock)
                {
                    out.push(Wl::Input(Event::Key { sym, pressed, mods: self.mods }));
                }
            }
            // wl_keyboard.modifiers(serial, depressed, latched, locked, group)
            (Kind::Keyboard, 4) => {
                let m = self.a(at, 1) | self.a(at, 2);
                let locked = self.a(at, 3);
                // the real modifiers have fixed bits in every xkb keymap: Shift 0, Lock 1, Control 2, Mod1 3, Mod4 6
                self.mods = Mods { shift: m & 1 != 0, ctrl: m & 4 != 0, alt: m & 8 != 0, logo: m & 64 != 0 };
                self.lock = locked & 2 != 0;
            }
            _ => {}
        }
        let _ = obj;
        Ok(())
    }

    fn end_axis(&mut self, out: &mut Vec<Wl>) {
        if !self.axis_any {
            return;
        }
        // points one wheel detent scrolls (no glide: content moves at once)
        const DETENT: f64 = 15.0;
        let (dx, dy) = if self.axis_finger {
            self.axis
        } else if self.axis120 != (0, 0) {
            (self.axis120.0 as f64 / 120.0 * DETENT, self.axis120.1 as f64 / 120.0 * DETENT)
        } else {
            // a wheel without value120: the axis value is already about 15 per detent
            self.axis
        };
        if dx != 0.0 || dy != 0.0 {
            out.push(Wl::Input(Event::Scroll { x: self.ptr.0, y: self.ptr.1, dx: dx * self.scale, dy: dy * self.scale }));
        }
        (self.axis, self.axis120, self.axis_any, self.axis_finger) = ((0.0, 0.0), (0, 0), false, false);
    }

    fn load_keymap(&mut self) {
        if self.keymap.is_some() {
            return;
        }
        let Some((fd, size)) = self.keymap_fd else { return };
        // SAFETY: a private read-only mapping of the keymap the compositor sent, `size` bytes, unmapped below.
        unsafe {
            let p = libc::mmap(core::ptr::null_mut(), size as usize, libc::PROT_READ, libc::MAP_PRIVATE, fd, 0);
            if p != libc::MAP_FAILED {
                let text = core::slice::from_raw_parts(p as *const u8, size as usize);
                self.keymap = Some(Keymap::parse(text));
                libc::munmap(p, size as usize);
            }
        }
    }

    /// The globals have arrived (the first sync is done).
    pub fn ready(&self) -> bool {
        self.sync_done
    }

    /// Write 2: bind what the window needs and make the toplevel. Call once, after [`Conn::ready`].
    pub fn create(&mut self, title: &str, app_id: &str) -> Result<(), Error> {
        if self.g_compositor.1 < 4 || self.g_shm.1 == 0 || self.g_wm.1 == 0 {
            return Err(Error("the compositor lacks wl_compositor v4, wl_shm or xdg_wm_base".into()));
        }
        let reg = 2;
        let bind = |c: &mut Conn, g: (u32, u32), iface: &str, max: u32, k: Kind| -> (u32, u32) {
            let id = c.new_id(k);
            let ver = g.1.min(max);
            c.req(reg, 0, |b| {
                // wl_registry.bind(name, interface, version, new_id)
                put32(b, g.0);
                put_str(b, iface);
                put32(b, ver);
                put32(b, id);
            });
            (id, ver)
        };
        (self.compositor, self.surface_version) = bind(self, self.g_compositor, "wl_compositor", 6, Kind::Compositor);
        self.shm = bind(self, self.g_shm, "wl_shm", 1, Kind::Shm).0;
        self.wm = bind(self, self.g_wm, "xdg_wm_base", 6, Kind::WmBase).0;
        if self.g_seat.1 != 0 {
            (self.seat, self.seat_version) = bind(self, self.g_seat, "wl_seat", 9, Kind::Seat);
        }
        let surface = self.new_id(Kind::Surface);
        self.surface = surface;
        let comp = self.compositor;
        self.req(comp, 0, |b| put32(b, surface)); // wl_compositor.create_surface
        let xs = self.new_id(Kind::XdgSurface);
        self.xdg_surface = xs;
        let wm = self.wm;
        self.req(wm, 2, |b| {
            // xdg_wm_base.get_xdg_surface(new_id, surface)
            put32(b, xs);
            put32(b, surface);
        });
        let top = self.new_id(Kind::Toplevel);
        self.toplevel = top;
        self.req(xs, 1, |b| put32(b, top)); // xdg_surface.get_toplevel
        self.req(top, 2, |b| put_str(b, title)); // xdg_toplevel.set_title
        self.req(top, 3, |b| put_str(b, app_id)); // xdg_toplevel.set_app_id
        if self.g_viewporter.1 != 0 && self.g_frac.1 != 0 {
            let vp = bind(self, self.g_viewporter, "wp_viewporter", 1, Kind::Viewporter).0;
            let fm = bind(self, self.g_frac, "wp_fractional_scale_manager_v1", 1, Kind::FracMgr).0;
            let v = self.new_id(Kind::Viewport);
            self.viewport = v;
            self.req(vp, 1, |b| {
                // wp_viewporter.get_viewport(new_id, surface)
                put32(b, v);
                put32(b, surface);
            });
            let f = self.new_id(Kind::Frac);
            self.req(fm, 1, |b| {
                // wp_fractional_scale_manager_v1.get_fractional_scale(new_id, surface)
                put32(b, f);
                put32(b, surface);
            });
        }
        self.req(surface, 6, |_| {}); // wl_surface.commit (no buffer: asks for the first configure)
        Ok(())
    }

    /// `wl_shm.create_pool(new_id, fd, size)`: returns the pool.
    pub fn create_pool(&mut self, fd: RawFd, size: i32) -> u32 {
        let id = self.new_id(Kind::Pool);
        let shm = self.shm;
        self.req(shm, 0, |b| {
            put32(b, id);
            put32(b, size as u32);
        });
        self.out_fds.push(fd);
        id
    }
    /// `wl_shm_pool.destroy`: the buffers made from it stay valid until they are destroyed; the compositor frees
    /// the pool's memory when the last of them is.
    pub fn destroy_pool(&mut self, pool: u32) {
        self.req(pool, 1, |_| {});
        self.kinds[pool as usize] = Kind::Dead;
    }
    /// `wl_shm_pool.resize(size)`.
    pub fn resize_pool(&mut self, pool: u32, size: i32) {
        self.req(pool, 2, |b| put32(b, size as u32));
    }
    /// `wl_shm_pool.create_buffer(new_id, offset, width, height, stride, format = XRGB8888)`.
    pub fn create_buffer(&mut self, pool: u32, offset: i32, w: i32, h: i32, generation: u32, index: u32) -> u32 {
        let id = self.new_id(Kind::Buffer(generation, index));
        self.req(pool, 0, |b| {
            for v in [id, offset as u32, w as u32, h as u32, (w * 4) as u32, 1] {
                put32(b, v);
            }
        });
        id
    }
    /// `wl_buffer.destroy`.
    pub fn destroy_buffer(&mut self, id: u32) {
        self.req(id, 0, |_| {});
        self.kinds[id as usize] = Kind::Dead;
    }

    /// One commit: the pending acknowledgement, the scale, the buffer, its damage (`damage` calls its argument
    /// per rectangle), a frame callback if `callback`.
    pub fn commit(&mut self, buffer: u32, scale120: u32, logical: (i32, i32), callback: bool, damage: impl FnOnce(&mut dyn FnMut(i32, i32, i32, i32))) {
        let (surface, xs) = (self.surface, self.xdg_surface);
        if let Some(serial) = self.ack.take() {
            self.req(xs, 4, |b| put32(b, serial)); // xdg_surface.ack_configure
        }
        if self.viewport != 0 {
            let v = self.viewport;
            self.req(v, 2, |b| {
                // wp_viewport.set_destination(width, height)
                put32(b, logical.0 as u32);
                put32(b, logical.1 as u32);
            });
        } else if self.surface_version >= 3 {
            self.req(surface, 8, |b| put32(b, scale120.div_ceil(120).max(1))); // wl_surface.set_buffer_scale
        }
        self.req(surface, 1, |b| {
            // wl_surface.attach(buffer, x, y)
            put32(b, buffer);
            put32(b, 0);
            put32(b, 0);
        });
        let this = self as *mut Conn;
        damage(&mut |x, y, w, h| {
            // SAFETY: `damage` is a plain producer of rectangles; it holds no borrow of the connection.
            let c = unsafe { &mut *this };
            c.req(surface, 9, |b| {
                // wl_surface.damage_buffer(x, y, width, height)
                for v in [x, y, w, h] {
                    put32(b, v as u32);
                }
            });
        });
        if callback {
            let cb = self.new_id(Kind::FrameCb);
            self.req(surface, 3, |b| put32(b, cb)); // wl_surface.frame
        }
        self.req(surface, 6, |_| {}); // wl_surface.commit
        self.commits += 1;
    }
    /// Acknowledge a configure with a commit that changes nothing else (a configure that needs no new frame).
    pub fn ack_only(&mut self) {
        if let Some(serial) = self.ack.take() {
            let (surface, xs) = (self.surface, self.xdg_surface);
            self.req(xs, 4, |b| put32(b, serial));
            self.req(surface, 6, |_| {});
        }
    }
    /// A frame callback with a commit that changes nothing else.
    pub fn callback_only(&mut self) {
        let surface = self.surface;
        let cb = self.new_id(Kind::FrameCb);
        self.req(surface, 3, |b| put32(b, cb));
        self.req(surface, 6, |_| {});
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        // SAFETY: descriptors this connection owns.
        unsafe {
            if let Some((fd, _)) = self.keymap_fd {
                libc::close(fd);
            }
            for &fd in &self.in_fds {
                libc::close(fd);
            }
            libc::close(self.fd);
        }
    }
}

#[inline(always)]
fn put32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_ne_bytes());
}
fn put_str(b: &mut Vec<u8>, s: &str) {
    put32(b, s.len() as u32 + 1);
    b.extend_from_slice(s.as_bytes());
    for _ in s.len()..pad(s.len() + 1) {
        b.push(0);
    }
}
#[inline(always)]
fn get32(b: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Shared memory for the window's buffers: one memfd, mapped once, grown in place. `n` slots of `slot` bytes;
/// a buffer is a slot. Pages exist only once written.
pub struct Shm {
    pub fd: RawFd,
    pub map: *mut u8,
    pub len: usize,
}
/// How a buffer's pages come to exist (`BOUGH_SHM`, a measurement switch; the default is `populate`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Prep {
    /// Nothing ahead of time: every page is a fault at first write (allocate, zero, map).
    Touch,
    /// `fallocate`: the pages are allocated and zeroed in one call; each is still a (minor) fault to map.
    Allocate,
    /// `fallocate`, then `MADV_POPULATE_WRITE`: allocated and mapped in two calls, no fault at all.
    Populate,
}
impl Prep {
    pub fn from_env() -> Prep {
        match std::env::var("BOUGH_SHM").as_deref() {
            Ok("touch") => Prep::Touch,
            Ok("fallocate") => Prep::Allocate,
            _ => Prep::Populate,
        }
    }
}

impl Shm {
    pub fn new(len: usize) -> Result<Shm, Error> {
        Shm::with(len, Prep::Populate)
    }
    pub fn with(len: usize, prep: Prep) -> Result<Shm, Error> {
        // SAFETY: memfd, ftruncate and mmap on a descriptor this function owns.
        unsafe {
            let fd = libc::memfd_create(c"bough".as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING);
            if fd < 0 {
                return Err(os("memfd_create"));
            }
            if libc::ftruncate(fd, len as libc::off_t) != 0 {
                return Err(os("ftruncate"));
            }
            let map = libc::mmap(core::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);
            if map == libc::MAP_FAILED {
                return Err(os("mmap"));
            }
            let mut s = Shm { fd, map: map as *mut u8, len };
            s.prepare(0, len, prep);
            Ok(s)
        }
    }
    /// Make the pages of `[at, at + len)` exist ahead of their first write. Failure is not an error: the pages
    /// then come to exist by faults, as they would have.
    pub fn prepare(&mut self, at: usize, len: usize, prep: Prep) {
        if prep == Prep::Touch || len == 0 {
            return;
        }
        // SAFETY: a range inside our own memfd and our own mapping of it.
        unsafe {
            libc::fallocate(self.fd, 0, at as libc::off_t, len as libc::off_t);
            if prep == Prep::Populate {
                const MADV_POPULATE_WRITE: libc::c_int = 23;
                libc::madvise(self.map.add(at) as *mut libc::c_void, len, MADV_POPULATE_WRITE);
            }
        }
    }
    /// Grow to `len` bytes. The mapping may move: pointers into it are invalid afterwards.
    pub fn grow(&mut self, len: usize) -> Result<(), Error> {
        if len <= self.len {
            return Ok(());
        }
        // SAFETY: growing our own memfd and remapping our own mapping.
        unsafe {
            if libc::ftruncate(self.fd, len as libc::off_t) != 0 {
                return Err(os("ftruncate"));
            }
            let map = libc::mremap(self.map as *mut libc::c_void, self.len, len, libc::MREMAP_MAYMOVE);
            if map == libc::MAP_FAILED {
                return Err(os("mremap"));
            }
            self.map = map as *mut u8;
            self.len = len;
        }
        Ok(())
    }
    /// Give the pages of `[at, at + len)` back to the system (they read as zero afterwards).
    pub fn punch(&mut self, at: usize, len: usize) {
        // SAFETY: a range inside our own memfd.
        unsafe { libc::fallocate(self.fd, libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE, at as libc::off_t, len as libc::off_t) };
    }
}
impl Drop for Shm {
    fn drop(&mut self) {
        // SAFETY: our own mapping and descriptor.
        unsafe {
            libc::munmap(self.map as *mut libc::c_void, self.len);
            libc::close(self.fd);
        }
    }
}

/// The printable character of a key event, if it types one.
pub fn typed(sym: Sym, mods: Mods) -> Option<char> {
    match sym {
        Sym::Char(c) if !mods.ctrl && !mods.alt && !mods.logo && !c.is_control() => Some(c),
        _ => None,
    }
}
