//! The GPU executor (Metal): naga's MSL of `kernel.rs`, compiled when the executor is created, one
//! threadgroup per scheduled tile. Headless: no window, no drawable. The four buffers are shared storage
//! (unified memory): the commands, the tile programs and the atlas are the bytes the CPU executors read, and
//! the output is read (or handed on) without a copy.
use crate::cpu::Prog;
use crate::stage::Scene;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::*;

type Buf = Retained<ProtocolObject<dyn MTLBuffer>>;

pub struct Gpu {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    pso: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
    /// 0 commands, 1 programs, 2 atlas, 3 output, 4 naga's array-sizes buffer (declared by its translation of
    /// runtime-sized arrays; the kernel never asks for a length).
    bufs: [Option<Buf>; 5],
}

/// Times of one GPU frame, in seconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct GpuTime {
    /// Commit to completion as the calling thread sees it.
    pub wall: f64,
    /// The kernel's time on the GPU (`GPUEndTime - GPUStartTime`).
    pub gpu: f64,
}

impl Gpu {
    /// Compile `msl` (normally `crate::KERNEL_MSL`; the gates pass mutated text).
    pub fn new(msl: &str) -> Option<Gpu> {
        let device = MTLCreateSystemDefaultDevice()?;
        let queue = device.newCommandQueue()?;
        let lib = device.newLibraryWithSource_options_error(&NSString::from_str(msl), None).unwrap_or_else(|e| panic!("kernel msl: {e}"));
        let f = lib.newFunctionWithName(&NSString::from_str("fs_ktile")).expect("fs_ktile");
        let pso = device.newComputePipelineStateWithFunction_error(&f).expect("pipeline");
        Some(Gpu { device, queue, pso, bufs: [None, None, None, None, None] })
    }

    fn buf(&mut self, i: usize, bytes: usize) -> &Buf {
        let need = bytes.max(16);
        if self.bufs[i].as_ref().is_none_or(|b| b.length() < need) {
            let cap = need.next_power_of_two();
            self.bufs[i] = Some(self.device.newBufferWithLength_options(cap, MTLResourceOptions::StorageModeShared).expect("buffer"));
        }
        self.bufs[i].as_ref().unwrap()
    }
    fn put<T: Copy>(&mut self, i: usize, data: &[T]) {
        let n = std::mem::size_of_val(data);
        let b = self.buf(i, n);
        // SAFETY: the buffer is shared storage of at least `n` bytes; no command buffer is in flight (every
        // frame is waited for before `render` returns).
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, b.contents().as_ptr() as *mut u8, n) };
    }

    /// Upload the scene, the atlas and the program (in the runtime these already live in shared memory).
    pub fn upload(&mut self, scene: &Scene, atlas: &[u8], prog: &Prog) {
        self.put(0, &scene.words);
        self.put(1, &prog.words);
        // the kernel reads the atlas as words: pad to a multiple of 4 bytes
        let b = self.buf(2, atlas.len() + 4);
        // SAFETY: as `put`.
        unsafe { std::ptr::copy_nonoverlapping(atlas.as_ptr(), b.contents().as_ptr() as *mut u8, atlas.len()) };
    }
    /// Rewrite only the program (a new damage set, or just a new `now`).
    pub fn upload_prog(&mut self, prog: &Prog) {
        self.put(1, &prog.words);
    }

    /// Run the uploaded program: `tiles` threadgroups into the output buffer of `out_words` words.
    pub fn render(&mut self, tiles: usize, out_words: usize) -> GpuTime {
        self.buf(3, out_words * 4);
        self.buf(4, 16);
        let t = std::time::Instant::now();
        let cmd = self.queue.commandBuffer().expect("command buffer");
        let enc = cmd.computeCommandEncoder().expect("encoder");
        enc.setComputePipelineState(&self.pso);
        for i in 0..5 {
            // SAFETY: each buffer is alive for the frame and bound at the index naga's translation reads.
            unsafe { enc.setBuffer_offset_atIndex(self.bufs[i].as_deref(), 0, i) };
        }
        if tiles > 0 {
            enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: tiles, height: 1, depth: 1 }, MTLSize { width: 16, height: 16, depth: 1 });
        }
        enc.endEncoding();
        cmd.commit();
        cmd.waitUntilCompleted();
        GpuTime { wall: t.elapsed().as_secs_f64(), gpu: cmd.GPUEndTime() - cmd.GPUStartTime() }
    }

    /// The output buffer's first `words` words.
    pub fn out(&self, words: usize) -> &[u32] {
        let b = self.bufs[3].as_ref().expect("render first");
        assert!(b.length() >= words * 4);
        // SAFETY: shared storage, at least `words` words, page aligned, and the GPU is idle.
        unsafe { std::slice::from_raw_parts(b.contents().as_ptr() as *const u32, words) }
    }
    /// Fill the output with `words` (tests: a sentinel that unscheduled tiles must keep).
    pub fn set_out(&mut self, words: &[u32]) {
        self.put(3, words);
    }
    /// Zero the output (tests: tiles a frame does not schedule must stay untouched).
    pub fn clear_out(&mut self, words: usize) {
        let b = self.buf(3, words * 4);
        // SAFETY: as `put`.
        unsafe { std::ptr::write_bytes(b.contents().as_ptr() as *mut u8, 0, words * 4) };
    }
}
