p='crates/bough-metal/src/fused.rs'
s=open(p).read()
# 1. a counter of GPU work since the last release
old='''    /// Has the process's GPU queue run work since it was made (the driver's per-queue block exists)?
    pub fn gpu_used(&self) -> bool {
        self.stats.gpu_frames > 0 || self.bin_growths > 0
    }'''
new='''    /// Have the presenter's queues run work since they were made (the driver's per-queue block exists)?
    pub fn gpu_used(&self) -> bool {
        self.work_since_release
    }

    /// Release the queues that ran work, and with them the driver's per-queue block (~69 MiB: examples/drvprobe.rs
    /// returns it within 55-300 ms), putting new, idle queues in their place: a queue costs nothing until it runs
    /// work, and its first frame then pays the driver's wake (~3 ms, ~11 M instructions), which a GPU idle for more
    /// than ~1.25 s pays anyway. The pre-armed stream, the residency set and keep-warm move to the new queues; the
    /// publisher thread is stopped (it starts again at the next publication). Returns whether anything was released.
    pub fn release_queues(&mut self) -> bool {
        if !self.work_since_release {
            return false;
        }
        self.sync();
        self.publisher_idle();
        // every armed frame of the old stream runs now (released with empty slots), so no command buffer of the
        // old queue is left pending; the stream is armed again on the new queue below
        {
            let kern = self.kern.clone();
            let st = kern.arm.lock().unwrap();
            for f in self.released + 1..=st.armed_upto {
                // SAFETY: slot f % 2 lies in the region; the previous frame finished (waited below / synced).
                unsafe { write_slot(self.pool_ptr, &self.lay, (f % 2) as usize, 0, &[], false, &SlotCopy::NONE) };
                self.ev.setSignaledValue(2 * f - 1);
                self.ev.waitUntilSignaledValue_timeoutMS(2 * f, 5000);
            }
            self.released = st.armed_upto;
        }
        self.publisher = None;
        if let Some(w) = self.warm.take() {
            w.cancelled.store(true, Relaxed);
        }
        let (q, a) = (self.device.newCommandQueue().expect("queue"), self.device.newCommandQueue().expect("aux queue"));
        if let Some(r) = &self.resid {
            self.queue.removeResidencySet(r);
            self.aux.removeResidencySet(r);
            q.addResidencySet(r);
            a.addResidencySet(r);
        }
        self.queue = q;
        self.aux = a;
        let old = &self.kern;
        let kern = Arc::new(Kern {
            queue: self.queue.clone(),
            ev: self.ev.clone(),
            pool: self.pool.clone(),
            pso_bin: old.pso_bin.clone(),
            pso_tile: old.pso_tile.clone(),
            pso_copy: old.pso_copy.clone(),
            slots: [0, 1, 2].map(|k| std::sync::atomic::AtomicU32::new(old.slots[k].load(Relaxed))),
            batch: std::sync::atomic::AtomicUsize::new(old.batch.load(Relaxed)),
            armed: AtomicU64::new(self.released),
            arm: std::sync::Mutex::new(ArmState { armed_upto: self.released, batches: std::collections::VecDeque::with_capacity(3) }),
        });
        self.kern = kern;
        self.warm = self.keep_warm.map(|(period, window)| Arc::new(Warm::new(self.aux.clone(), &self.device, period, window)));
        self.work_since_release = false;
        self.stats.releases += 1;
        // waiting command buffers allocate nothing until released (floorprobe `armed`)
        self.kern.ensure(self.released + 1);
        true
    }'''
assert old in s; s=s.replace(old,new,1)
# 2. fields
old='''    /// Column views (`new_views`): `None` when every column is a range of the one region buffer.
    views: Option<Views>,
}'''
new='''    /// Column views (`new_views`): `None` when every column is a range of the one region buffer.
    views: Option<Views>,
    /// A command buffer that ran work was committed since the queues were made (`release_queues`).
    work_since_release: bool,
    /// Keep-warm's configuration (`BOUGH_KEEPWARM`: period s, window s), to rebuild it on new queues.
    keep_warm: Option<(f64, f64)>,
}'''
assert old in s; s=s.replace(old,new,1)
old='''            last_main: NONE_SURF,
            views,
        };'''
new='''            last_main: NONE_SURF,
            views,
            work_since_release: false,
            keep_warm: None,
        };'''
assert old in s; s=s.replace(old,new,1)
old='''        if let Some((period, window)) = warm {
            let tiny = f.device.newBufferWithLength_options(16, MTLResourceOptions::StorageModePrivate).expect("keep-warm buffer");
            f.warm = Some(Arc::new(Warm {
                queue: f.aux.clone(),
                tiny,
                period,
                window,
                last_gpu: AtomicU64::new(0f64.to_bits()),
                last_blit: AtomicU64::new(0f64.to_bits()),
                armed: AtomicBool::new(false),
                cancelled: AtomicBool::new(false),
                blits: AtomicU64::new(0),
            }));
        }
        f'''
new='''        f.keep_warm = warm;
        if let Some((period, window)) = warm {
            f.warm = Some(Arc::new(Warm::new(f.aux.clone(), &f.device, period, window)));
        }
        f'''
assert old in s; s=s.replace(old,new,1)
old='''impl Warm {
    fn arm(self: &Arc<Self>, after: f64) {'''
new='''impl Warm {
    fn new(queue: Retained<ProtocolObject<dyn MTLCommandQueue>>, device: &Dev, period: f64, window: f64) -> Warm {
        let tiny = device.newBufferWithLength_options(16, MTLResourceOptions::StorageModePrivate).expect("keep-warm buffer");
        Warm {
            queue,
            tiny,
            period,
            window,
            last_gpu: AtomicU64::new(0f64.to_bits()),
            last_blit: AtomicU64::new(0f64.to_bits()),
            armed: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            blits: AtomicU64::new(0),
        }
    }
    fn arm(self: &Arc<Self>, after: f64) {'''
assert old in s; s=s.replace(old,new,1)
# 3. stats.releases
old='''    /// Pre-armed batches drained because the residency set grew.
    pub drains: u64,
}'''
new='''    /// Pre-armed batches drained because the residency set grew.
    pub drains: u64,
    /// Queue releases (`release_queues`).
    pub releases: u64,
}'''
assert old in s; s=s.replace(old,new,1)
# 4. mark work: GPU frames (release), check_bins redraw, reference renders, bench, residency drains, keep-warm blits
s=s.replace('''    fn release(&mut self, k: usize, nt: u32) {
        let f = self.released + 1;''','''    fn release(&mut self, k: usize, nt: u32) {
        self.work_since_release = true;
        let f = self.released + 1;''',1)
s=s.replace('''    fn residency_changed(&mut self) {
        let want = 2 * self.released;''','''    fn residency_changed(&mut self) {
        // releasing armed frames runs them (empty, but GPU work): the queue's block exists afterwards
        self.work_since_release = true;
        let want = 2 * self.released;''',1)
s=s.replace('''    pub fn reference_pixels(&mut self, n_dir: u32) -> Vec<u8> {
        self.sync();''','''    pub fn reference_pixels(&mut self, n_dir: u32) -> Vec<u8> {
        self.sync();
        self.work_since_release = true;''',1)
s=s.replace('''    pub fn bench_frames(&mut self, target: usize, rows: &[u128], full: bool, n_dir: u32, reps: usize, debug: u32, copy: &SlotCopy) -> f64 {
        self.sync();''','''    pub fn bench_frames(&mut self, target: usize, rows: &[u128], full: bool, n_dir: u32, reps: usize, debug: u32, copy: &SlotCopy) -> f64 {
        self.sync();
        self.work_since_release = true;''',1)
open(p,'w').write(s)
print('ok')
