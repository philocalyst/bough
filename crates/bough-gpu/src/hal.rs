//! The per-frame path on the vendored wgpu-hal Metal backend (ROUND4 §8; the lead's steer: no wgpu-core on the
//! frame path). Objects are created once; a frame is either one hal command buffer (`frame_submit`) or a frame of a
//! pre-armed stream released by a shared event (`arm` + `release`), the design of `bough_metal::fused`.
//!
//! Creation goes around the parts of wgpu that cost startup: no instance or adapter enumeration
//! (`MTLCreateSystemDefaultDevice` + `Device::device_from_raw`, `Queue::queue_from_raw`), no WGSL parse or naga
//! translation (the metallib is built by build.rs), no validation layer. The region is a `storageModeShared`
//! MTLBuffer the CPU writes in place, wrapped with `Device::buffer_from_raw`; surfaces are IOSurface-backed
//! textures wrapped with `Device::texture_from_raw`.
//!
//! This module is the first increment of the port: the scroll copy kernel (`fs_copy`, wgsl-rs) end to end, with
//! the startup and per-frame costs measured against direct Metal (`examples/halprobe.rs`).
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_foundation::{CFDictionary, CFRetained};
use objc2_foundation::{NSDictionary, NSNumber, NSString, ns_string};
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::*;
use std::time::Instant;
use wgpu_hal as hal;
use wgpu_hal::{CommandEncoder as _, Device as _, Queue as _};
use wgpu_types as wgt;

/// Startup phases, in microseconds.
#[derive(Debug, Default, Clone, Copy)]
pub struct Startup {
    pub device_us: f64,
    pub queue_us: f64,
    pub region_us: f64,
    pub surfaces_us: f64,
    pub pipeline_us: f64,
    pub total_us: f64,
}

pub struct Surface {
    pub io: CFRetained<IOSurfaceRef>,
    pub raw: Retained<ProtocolObject<dyn MTLTexture>>,
    pub tex: hal::metal::Texture,
    pub write_view: hal::metal::TextureView,
    pub read_view: hal::metal::TextureView,
}

pub struct HalGpu {
    pub device: hal::metal::Device,
    pub queue: hal::metal::Queue,
    pub raw_device: Retained<ProtocolObject<dyn MTLDevice>>,
    pub region_raw: Retained<ProtocolObject<dyn MTLBuffer>>,
    pub region: hal::metal::Buffer,
    pub region_ptr: *mut u8,
    pub surfs: Vec<Surface>,
    pub w: usize,
    pub h: usize,
    pub bgl0: hal::metal::BindGroupLayout,
    pub bgl1: hal::metal::BindGroupLayout,
    pub layout: hal::metal::PipelineLayout,
    pub module: hal::metal::ShaderModule,
    pub pso_copy: hal::metal::ComputePipeline,
    pub bg0: hal::metal::BindGroup,
    /// One small uniform buffer + bind group per frame slot (the slot's byte offset in the region).
    pub slot_bufs: Vec<hal::metal::Buffer>,
    pub bg1: Vec<hal::metal::BindGroup>,
    pub fence: hal::metal::Fence,
    pub fence_value: u64,
    pub encoder: hal::metal::CommandEncoder,
    /// The pre-armed stream's event (released frame f: `2f - 1`, done: `2f`).
    pub ev: Retained<ProtocolObject<dyn MTLSharedEvent>>,
    pub armed_upto: u64,
    pub released: u64,
    pub startup: Startup,
    pub pso_compact: hal::metal::ComputePipeline,
    pub slot_raw: Vec<Retained<ProtocolObject<dyn MTLBuffer>>>,
    pub bins_raw: Retained<ProtocolObject<dyn MTLBuffer>>,
    pub bins: hal::metal::Buffer,
    pub counts: hal::metal::Buffer,
    pub resid: Retained<ProtocolObject<dyn MTLResidencySet>>,
}

fn make_iosurface(w: usize, h: usize) -> CFRetained<IOSurfaceRef> {
    let keys: [&NSString; 4] = [ns_string!("IOSurfaceWidth"), ns_string!("IOSurfaceHeight"), ns_string!("IOSurfaceBytesPerElement"), ns_string!("IOSurfacePixelFormat")];
    let vals = [NSNumber::new_i32(w as i32), NSNumber::new_i32(h as i32), NSNumber::new_i32(4), NSNumber::new_u32(0x4247_5241)];
    let refs: Vec<&objc2::runtime::AnyObject> = vals.iter().map(|v| v.as_ref() as &objc2::runtime::AnyObject).collect();
    let dict: Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> = NSDictionary::from_slices(&keys, &refs);
    // SAFETY: NSDictionary is toll-free bridged to CFDictionary.
    let cf: &CFDictionary = unsafe { &*(Retained::as_ptr(&dict) as *const CFDictionary) };
    // SAFETY: a well-formed property dictionary.
    unsafe { IOSurfaceRef::new(cf) }.expect("IOSurface")
}

fn us(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e6
}

impl HalGpu {
    /// Device, queue, a `region_len`-byte shared region, `nsurf` IOSurface surfaces of `w` x `h`, the kernels.
    /// `slots`: byte offsets of the frame slots in the region.
    pub fn new(w: usize, h: usize, region_len: usize, nsurf: usize, slots: &[u32]) -> HalGpu {
        let t_all = Instant::now();
        let mut st = Startup::default();
        let t = Instant::now();
        let raw_device = MTLCreateSystemDefaultDevice().expect("no Metal device");
        let features = wgt::Features::BGRA8UNORM_STORAGE;
        // SAFETY: a live MTLDevice; the features are ones every Apple-silicon GPU has.
        let device = unsafe { hal::metal::Device::device_from_raw(raw_device.clone(), features) };
        st.device_us = us(t);
        let t = Instant::now();
        // SAFETY: a new queue of that device.
        let queue = unsafe { hal::metal::Queue::queue_from_raw(raw_device.newCommandQueue().expect("queue"), 1.0) };
        st.queue_us = us(t);
        let t = Instant::now();
        let region_raw = raw_device.newBufferWithLength_options(region_len, MTLResourceOptions::StorageModeShared).expect("region");
        let region_ptr = region_raw.contents().as_ptr() as *mut u8;
        // SAFETY: the buffer is live and `region_len` bytes long.
        let region = unsafe { hal::metal::Device::buffer_from_raw(region_raw.clone(), region_len as u64) };
        st.region_us = us(t);
        let t = Instant::now();
        let surfs: Vec<Surface> = (0..nsurf).map(|_| Self::new_surface(&raw_device, &device, w, h)).collect();
        st.surfaces_us = us(t);
        let t = Instant::now();
        let storage = |binding: u32, write: bool| wgt::BindGroupLayoutEntry {
            binding,
            visibility: wgt::ShaderStages::COMPUTE,
            ty: if write {
                wgt::BindingType::StorageTexture { access: wgt::StorageTextureAccess::WriteOnly, format: wgt::TextureFormat::Bgra8Unorm, view_dimension: wgt::TextureViewDimension::D2 }
            } else {
                wgt::BindingType::Texture { sample_type: wgt::TextureSampleType::Float { filterable: false }, view_dimension: wgt::TextureViewDimension::D2, multisampled: false }
            },
            count: None,
        };
        let buf = |binding: u32, read_only: bool| wgt::BindGroupLayoutEntry {
            binding,
            visibility: wgt::ShaderStages::COMPUTE,
            ty: wgt::BindingType::Buffer { ty: wgt::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        // the region, the row bins, their counts (textures come from the region's bind table)
        let e0 = vec![buf(0, true), buf(1, false), buf(2, false)];
        // textures are not bound: the kernels read them from the bind table in the region (build.rs `bindless`)
        let _ = storage;
        let e1 = [wgt::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgt::ShaderStages::COMPUTE,
            ty: wgt::BindingType::Buffer { ty: wgt::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }];
        // SAFETY (this block): descriptors reference live objects of this device; the kernels' bindings match the
        // layouts (build.rs mirrors wgpu-hal's index assignment for exactly this layout).
        let (bgl0, bgl1, layout, module, pso_copy, pso_compact) = unsafe {
            let bgl0 = device.create_bind_group_layout(&hal::BindGroupLayoutDescriptor { label: None, flags: hal::BindGroupLayoutFlags::empty(), entries: &e0 }).unwrap();
            let bgl1 = device.create_bind_group_layout(&hal::BindGroupLayoutDescriptor { label: None, flags: hal::BindGroupLayoutFlags::empty(), entries: &e1 }).unwrap();
            let layout = device
                .create_pipeline_layout(&hal::PipelineLayoutDescriptor { label: None, flags: hal::PipelineLayoutFlags::empty(), bind_group_layouts: &[Some(&bgl0), Some(&bgl1)], immediate_size: 0 })
                .unwrap();
            let module = device
                .create_shader_module(&hal::ShaderModuleDescriptor { label: None, runtime_checks: wgt::ShaderRuntimeChecks::unchecked() }, hal::ShaderInput::MetalLib { file: crate::METALLIB, num_workgroups: (32, 8, 1) })
                .unwrap();
            let consts = Default::default();
            let pso_copy = device
                .create_compute_pipeline(&hal::ComputePipelineDescriptor {
                    label: None,
                    layout: &layout,
                    stage: hal::ProgrammableStage { module: &module, entry_point: "fs_copy", constants: &consts, zero_initialize_workgroup_memory: false },
                    cache: None,
                })
                .unwrap();
            let pso_compact = device
                .create_compute_pipeline(&hal::ComputePipelineDescriptor {
                    label: None,
                    layout: &layout,
                    stage: hal::ProgrammableStage { module: &module, entry_point: "fs_compact_test", constants: &consts, zero_initialize_workgroup_memory: false },
                    cache: None,
                })
                .unwrap();
            (bgl0, bgl1, layout, module, pso_copy, pso_compact)
        };
        st.pipeline_us = us(t);
        let mut slot_bufs = Vec::new();
        let mut slot_raw = Vec::new();
        let mut bg1 = Vec::new();
        for &off in slots {
            let raw = raw_device.newBufferWithLength_options(16, MTLResourceOptions::StorageModeShared).unwrap();
            // SAFETY: 16 bytes of shared storage.
            unsafe { (raw.contents().as_ptr() as *mut [u32; 4]).write([off, 0, 0, 0]) };
            slot_raw.push(raw.clone());
            // SAFETY: live buffer of 16 bytes.
            let b = unsafe { hal::metal::Device::buffer_from_raw(raw, 16) };
            let bb = [hal::BufferBinding::new_unchecked(&b, 0, None)];
            let entries = [hal::BindGroupEntry { binding: 0, resource_index: 0, count: 1 }];
            // SAFETY: the bind group references live objects matching `bgl1`.
            let g = unsafe {
                device
                    .create_bind_group(&hal::BindGroupDescriptor { label: None, layout: &bgl1, buffers: &bb, samplers: &[], textures: &[], entries: &entries, acceleration_structures: &[], external_textures: &[] })
                    .unwrap()
            };
            slot_bufs.push(b);
            bg1.push(g);
        }
        let bins_raw = raw_device.newBufferWithLength_options(1 << 20, MTLResourceOptions::StorageModeShared).unwrap();
        let counts_raw = raw_device.newBufferWithLength_options(4096, MTLResourceOptions::StorageModeShared).unwrap();
        // SAFETY: live buffers of these lengths.
        let (bins, counts) = unsafe { (hal::metal::Device::buffer_from_raw(bins_raw.clone(), 1 << 20), hal::metal::Device::buffer_from_raw(counts_raw.clone(), 4096)) };
        let bg0 = Self::bind0(&device, &bgl0, &region, &bins, &counts);
        // SAFETY: device-owned objects.
        let (fence, encoder) = unsafe {
            (device.create_fence().unwrap(), device.create_command_encoder(&hal::CommandEncoderDescriptor { label: None, queue: &queue }).unwrap())
        };
        let ev = raw_device.newSharedEvent().expect("shared event");
        // the bind table (resource ids, region offset 128) and the residency set that makes them reachable
        let rid = |t: &ProtocolObject<dyn MTLTexture>| -> u64 {
            // SAFETY: MTLResourceID is a 64-bit handle.
            unsafe { std::mem::transmute::<MTLResourceID, u64>(t.gpuResourceID()) }
        };
        let n = surfs.len();
        let mut table = [0u64; 11];
        for i in 0..4 {
            table[i] = rid(&surfs[i % n].raw);
            table[7 + i] = table[i];
        }
        table[4] = table[0];
        // SAFETY: the header reserves 256 bytes; the table is 88 bytes at 128 (INTERFACE.md `FBind`).
        unsafe { (region_ptr.add(128) as *mut [u64; 11]).write(table) };
        let d = MTLResidencySetDescriptor::new();
        let set = raw_device.newResidencySetWithDescriptor_error(&d).expect("residency set");
        set.addAllocation(ProtocolObject::from_ref(&*region_raw));
        for s in &surfs {
            set.addAllocation(ProtocolObject::from_ref(&*s.raw));
        }
        set.commit();
        set.requestResidency();
        queue.as_raw().addResidencySet(&set);
        st.total_us = us(t_all);
        HalGpu {
            device,
            queue,
            raw_device,
            region_raw,
            region,
            region_ptr,
            surfs,
            w,
            h,
            bgl0,
            bgl1,
            layout,
            module,
            pso_copy,
            bg0,
            slot_bufs,
            bg1,
            fence,
            fence_value: 0,
            encoder,
            ev,
            armed_upto: 0,
            released: 0,
            startup: st,
            pso_compact,
            slot_raw,
            bins_raw,
            bins,
            counts,
            resid: set,
        }
    }

    fn new_surface(raw_device: &ProtocolObject<dyn MTLDevice>, device: &hal::metal::Device, w: usize, h: usize) -> Surface {
        let io = make_iosurface(w, h);
        // SAFETY: plain texture descriptor.
        let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::BGRA8Unorm, w, h, false) };
        d.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite);
        d.setStorageMode(MTLStorageMode::Shared);
        let raw = raw_device.newTextureWithDescriptor_iosurface_plane(&d, &io, 0).expect("iosurface texture");
        // SAFETY: a live 2D BGRA8 texture of one layer and one mip.
        let tex = unsafe {
            hal::metal::Device::texture_from_raw(raw.clone(), wgt::TextureFormat::Bgra8Unorm, MTLTextureType::Type2D, 1, 1, hal::CopyExtent { width: w as u32, height: h as u32, depth: 1 })
        };
        let view = |usage| {
            // SAFETY: a view of the whole texture in its own format.
            unsafe {
                device
                    .create_texture_view(
                        &tex,
                        &hal::TextureViewDescriptor {
                            label: None,
                            format: wgt::TextureFormat::Bgra8Unorm,
                            dimension: wgt::TextureViewDimension::D2,
                            usage,
                            range: wgt::ImageSubresourceRange::default(),
                        },
                    )
                    .unwrap()
            }
        };
        let write_view = view(wgt::TextureUses::STORAGE_WRITE_ONLY);
        let read_view = view(wgt::TextureUses::RESOURCE);
        Surface { io, raw, tex, write_view, read_view }
    }

    fn bind0(device: &hal::metal::Device, bgl0: &hal::metal::BindGroupLayout, region: &hal::metal::Buffer, bins: &hal::metal::Buffer, counts: &hal::metal::Buffer) -> hal::metal::BindGroup {
        let bb = [hal::BufferBinding::new_unchecked(region, 0, None), hal::BufferBinding::new_unchecked(bins, 0, None), hal::BufferBinding::new_unchecked(counts, 0, None)];
        let tb: Vec<hal::TextureBinding<'_, hal::metal::TextureView>> = Vec::new();
        let entries: Vec<hal::BindGroupEntry> = (0..3).map(|i| hal::BindGroupEntry { binding: i, resource_index: i, count: 1 }).collect();
        // SAFETY: the bind group references live objects matching `bgl0`.
        unsafe {
            device
                .create_bind_group(&hal::BindGroupDescriptor { label: None, layout: bgl0, buffers: &bb, samplers: &[], textures: &tb, entries: &entries, acceleration_structures: &[], external_textures: &[] })
                .unwrap()
        }
    }

    /// Encode slot `k`'s copy into the hal encoder (one compute pass; dispatch size from the slot).
    ///
    /// # Safety
    /// The encoder is recording; slot `k` lies in the region.
    unsafe fn encode_frame(&mut self, k: usize, slot_off: u32) {
        unsafe {
            self.encoder.begin_compute_pass(&hal::ComputePassDescriptor { label: None, timestamp_writes: None });
            self.encoder.set_compute_pipeline(&self.pso_copy);
            self.encoder.set_bind_group(&self.layout, 0, &self.bg0, &[]);
            self.encoder.set_bind_group(&self.layout, 1, &self.bg1[k], &[]);
            self.encoder.dispatch_indirect(&self.region, slot_off as u64 + 32);
            self.encoder.end_compute_pass();
        }
    }

    /// One frame as one hal command buffer: encode, submit with the fence, and (if `wait`) wait for it.
    pub fn frame_submit(&mut self, k: usize, slot_off: u32, wait: bool) {
        // SAFETY: the encoder is idle between frames; objects are live; the CPU wrote slot k before this call.
        unsafe {
            self.encoder.begin_encoding(None).unwrap();
            self.encode_frame(k, slot_off);
            let cb = self.encoder.end_encoding().unwrap();
            self.fence_value += 1;
            self.queue.submit(&[&cb], &[], (&mut self.fence, self.fence_value)).unwrap();
            if wait {
                self.device.wait(&self.fence, self.fence_value, None).unwrap();
            }
            self.encoder.reset_all(std::iter::once(cb));
        }
    }

    /// Wait for the last frame: the last released stream frame (its event value), else the last submitted
    /// command buffer (a pre-armed batch's fence only completes when all its frames are released).
    pub fn wait_idle(&mut self) {
        if self.armed_upto == 0 {
            // SAFETY: fence of this device.
            unsafe { self.device.wait(&self.fence, self.fence_value, None).unwrap() };
        }
        if self.released > 0 {
            let want = 2 * self.released;
            if self.ev.signaledValue() < want {
                self.ev.waitUntilSignaledValue_timeoutMS(want, 5000);
            }
        }
    }

    /// Pre-arm `batch` frames in ONE hal command buffer: each waits for `ev >= 2f - 1`, runs slot `f % 2`, signals
    /// `ev = 2f` (the shared-event waits go through the vendored `raw_command_buffer` extension).
    pub fn arm(&mut self, batch: u64, slots: &[u32]) {
        // SAFETY: the encoder is idle; the event and the region are live.
        unsafe {
            self.encoder.begin_encoding(None).unwrap();
            let evr = self.ev.clone();
            for f in self.armed_upto + 1..=self.armed_upto + batch {
                let ev: &ProtocolObject<dyn MTLEvent> = ProtocolObject::from_ref(&*evr);
                self.encoder.raw_command_buffer().unwrap().encodeWaitForEvent_value(ev, 2 * f - 1);
                let k = (f % 2) as usize;
                self.encode_frame(k, slots[k]);
                self.encoder.raw_command_buffer().unwrap().encodeSignalEvent_value(ev, 2 * f);
            }
            let cb = self.encoder.end_encoding().unwrap();
            self.fence_value += 1;
            self.queue.submit(&[&cb], &[], (&mut self.fence, self.fence_value)).unwrap();
            self.encoder.reset_all(std::iter::once(cb));
        }
        self.armed_upto += batch;
    }

    /// Release the next pre-armed frame (its slot is written): one shared-event write.
    pub fn release(&mut self) {
        let want = 2 * self.released;
        if self.released > 0 && self.ev.signaledValue() < want {
            self.ev.waitUntilSignaledValue_timeoutMS(want, 5000);
        }
        self.released += 1;
        self.ev.setSignaledValue(2 * self.released - 1);
    }

    /// Self-test of the subgroup compaction (`fs_compact_test`): compacts the set bits of the 8 region words at
    /// byte offset `off` (slot 2's uniform carries it) into the bins; returns (lane, total) per hit.
    pub fn compact_test(&mut self, off: u32, words: [u32; 8]) -> Vec<(u32, u32)> {
        // SAFETY: `off` + 32 lies in the region and slot 2's 16-byte uniform buffer; the GPU is idle.
        unsafe {
            (self.region_ptr.add(off as usize) as *mut [u32; 8]).write(words);
            let u = self.slot_raw[2].contents().as_ptr() as *mut u32;
            *u.add(1) = off;
            self.encoder.begin_encoding(None).unwrap();
            self.encoder.begin_compute_pass(&hal::ComputePassDescriptor { label: None, timestamp_writes: None });
            self.encoder.set_compute_pipeline(&self.pso_compact);
            self.encoder.set_bind_group(&self.layout, 0, &self.bg0, &[]);
            self.encoder.set_bind_group(&self.layout, 1, &self.bg1[2], &[]);
            self.encoder.dispatch([1, 1, 1]);
            self.encoder.end_compute_pass();
            let cb = self.encoder.end_encoding().unwrap();
            self.fence_value += 1;
            self.queue.submit(&[&cb], &[], (&mut self.fence, self.fence_value)).unwrap();
            self.device.wait(&self.fence, self.fence_value, None).unwrap();
            self.encoder.reset_all(std::iter::once(cb));
            let n: u32 = words.iter().map(|w| w.count_ones()).sum();
            let b = self.bins_raw.contents().as_ptr() as *const [u32; 4];
            (0..n as usize).map(|i| ((*b.add(i))[0], (*b.add(i))[1])).collect()
        }
    }

    pub fn surface_pixels(&self, i: usize) -> Vec<u8> {
        let io = &self.surfs[i].io;
        let (w, h) = (self.w, self.h);
        let mut v = vec![0u8; w * 4 * h];
        // SAFETY: the surface is locked for reading; rows are `bytes_per_row` apart and `w * 4` long.
        unsafe {
            io.lock(objc2_io_surface::IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
            let bpr = io.bytes_per_row();
            let base = io.base_address().as_ptr() as *const u8;
            for y in 0..h {
                std::ptr::copy_nonoverlapping(base.add(y * bpr), v.as_mut_ptr().add(y * w * 4), w * 4);
            }
            io.unlock(objc2_io_surface::IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        }
        v
    }

    /// Fill surface `i` with `f(x, y)` from the CPU (tests).
    pub fn fill_surface(&self, i: usize, f: impl Fn(usize, usize) -> [u8; 4]) {
        let io = &self.surfs[i].io;
        // SAFETY: the surface is locked for writing; the GPU is idle (callers wait).
        unsafe {
            io.lock(objc2_io_surface::IOSurfaceLockOptions::empty(), std::ptr::null_mut());
            let bpr = io.bytes_per_row();
            let base = io.base_address().as_ptr() as *mut u8;
            for y in 0..self.h {
                for x in 0..self.w {
                    (base.add(y * bpr + x * 4) as *mut [u8; 4]).write(f(x, y));
                }
            }
            io.unlock(objc2_io_surface::IOSurfaceLockOptions::empty(), std::ptr::null_mut());
        }
    }
}
