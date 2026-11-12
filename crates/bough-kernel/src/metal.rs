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
    /// 0 commands, 1 programs, 2 atlas, 3 output, 4 image atlas, 5 naga's array-sizes buffer (declared by its translation of
    /// runtime-sized arrays; the kernel never asks for a length).
    bufs: [Option<Buf>; 6],
}

/// An output buffer over caller-owned memory (`Gpu::wrap`).
pub struct Target(Buf);

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
        Some(Gpu { device, queue, pso, bufs: [None, None, None, None, None, None] })
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

    /// Upload the scene, both atlases and the program (in the runtime these already live in shared memory).
    pub fn upload(&mut self, scene: &Scene, atlas: &crate::cpu::Atlas, prog: &Prog) {
        self.put(0, &scene.words);
        self.put(1, &prog.words);
        self.upload_atlas(atlas.px);
        self.upload_images(atlas.img);
    }
    /// Rewrite only the program (a new damage set, or just a new `now`).
    pub fn upload_prog(&mut self, prog: &Prog) {
        self.put(1, &prog.words);
    }

    /// Upload this frame's commands and program only (the atlas is uploaded when it changes).
    pub fn upload_frame(&mut self, scene: &Scene, prog: &Prog) {
        self.put(0, &scene.words);
        self.put(1, &prog.words);
    }
    pub fn upload_atlas(&mut self, atlas: &[u8]) {
        let b = self.buf(2, atlas.len() + 4);
        // SAFETY: as `put`.
        unsafe { std::ptr::copy_nonoverlapping(atlas.as_ptr(), b.contents().as_ptr() as *mut u8, atlas.len()) };
    }

    /// The image atlas (premultiplied RGBA8 bytes).
    pub fn upload_images(&mut self, img: &[u8]) {
        self.put(4, img);
    }

    /// A buffer over memory the caller owns: a surface the kernel writes in place (journal E9: Metal accepts
    /// an IOSurface's base address, and the surface's texture view and the compositor see the words).
    /// `None` if Metal refuses (the address or length is not page aligned).
    ///
    /// # Safety
    /// `ptr..ptr + len` must stay mapped and writable for as long as the `Target` lives, and nothing else may
    /// write it while a frame rendered to it is in flight (`render_to` waits for completion).
    pub unsafe fn wrap(&self, ptr: *mut u8, len: usize) -> Option<Target> {
        let p = std::ptr::NonNull::new(ptr as *mut std::ffi::c_void)?;
        // SAFETY: the caller's contract; no deallocator (the caller owns the memory).
        unsafe { self.device.newBufferWithBytesNoCopy_length_options_deallocator(p, len, MTLResourceOptions::StorageModeShared, None) }.map(Target)
    }
    /// Run the uploaded program with `target` as the output (its stride is the program's).
    pub fn render_to(&mut self, tiles: usize, target: &Target) -> GpuTime {
        self.buf(4, 16);
        self.buf(5, 16);
        self.run(tiles, Some(&target.0))
    }

    /// Run the uploaded program: `tiles` threadgroups into the output buffer of `out_words` words.
    pub fn render(&mut self, tiles: usize, out_words: usize) -> GpuTime {
        self.buf(3, out_words * 4);
        self.buf(4, 16);
        self.buf(5, 16);
        self.run(tiles, None)
    }
    fn run(&mut self, tiles: usize, target: Option<&Buf>) -> GpuTime {
        let t = std::time::Instant::now();
        let cmd = self.queue.commandBuffer().expect("command buffer");
        let enc = cmd.computeCommandEncoder().expect("encoder");
        enc.setComputePipelineState(&self.pso);
        for i in 0..6 {
            let b = if i == 3 { target.map(|t| &**t).or(self.bufs[3].as_deref()) } else { self.bufs[i].as_deref() };
            // SAFETY: each buffer is alive for the frame and bound at the index naga's translation reads.
            unsafe { enc.setBuffer_offset_atIndex(b, 0, i) };
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
