//! The GPU executor on Vulkan (Linux; round 7, platform lane): naga's SPIR-V of `kernel.rs` (build.rs), one
//! workgroup per scheduled tile, through the vendored wgpu-hal's Vulkan backend. The same API as `metal.rs`, so
//! the exactness gate and the app's backend drive either. Headless: no swapchain.
//!
//! * **Buffers**: the five storage buffers of `kernel.rs` (commands, programs, atlas, output, images) are
//!   host-visible, coherent and persistently mapped: the CPU writes them in place and reads the output in place
//!   (lavapipe and integrated GPUs give such memory; a discrete GPU without resizable BAR would read system memory
//!   over the bus, not measured). naga's SPIR-V reads array lengths itself, so there is no sizes buffer.
//! * **The window's words** (`wrap`): with `VK_EXT_external_memory_host` the output buffer is the caller's memory
//!   imported as device memory: the kernel writes the pixels of a `wl_shm` buffer the compositor reads, with no
//!   copy, as the Metal executor writes an IOSurface's base address. Without the extension `wrap` returns `None`
//!   and the caller copies from `out`.
//! * **Ordering**: one command buffer per frame: the dispatch, then a barrier making the shader's writes visible
//!   to the host; the frame is waited for on a fence before `render` returns.
use crate::cpu::Prog;
use crate::stage::Scene;
use ash::vk;
use std::ptr::NonNull;
use wgpu_hal as hal;
use wgpu_hal::{CommandEncoder as _, Device as _, Instance as _, Queue as _};
use wgpu_types as wgt;

type Api = hal::api::Vulkan;
type HBuf = <Api as hal::Api>::Buffer;

/// SPIR-V bytes as words (the hal takes `&[u32]`; `include_bytes!` is only byte aligned).
pub fn spirv_words(bytes: &[u8]) -> Vec<u32> {
    assert!(bytes.len() % 4 == 0, "SPIR-V is a whole number of words");
    bytes.chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect()
}

/// A mapped storage buffer of this device.
struct Buf {
    b: HBuf,
    ptr: NonNull<u8>,
    cap: usize,
}

struct Objs {
    bgl: <Api as hal::Api>::BindGroupLayout,
    layout: <Api as hal::Api>::PipelineLayout,
    module: <Api as hal::Api>::ShaderModule,
    pso: <Api as hal::Api>::ComputePipeline,
    fence: <Api as hal::Api>::Fence,
}

pub struct Gpu {
    pub info: wgt::AdapterInfo,
    device: <Api as hal::Api>::Device,
    queue: <Api as hal::Api>::Queue,
    /// `VK_EXT_external_memory_host` is enabled: `wrap` imports caller memory.
    host_import: Option<(ash::ext::external_memory_host::Device, u64)>,
    o: std::mem::ManuallyDrop<Objs>,
    /// 0 commands, 1 programs, 2 atlas, 3 output, 4 image atlas.
    bufs: [Option<Buf>; 5],
    encoder: Option<<Api as hal::Api>::CommandEncoder>,
    fence_value: u64,
}

/// An output buffer over caller-owned memory (`Gpu::wrap`): device memory imported from the host pointer.
pub struct Target {
    buf: Option<HBuf>,
    mem: vk::DeviceMemory,
    raw: vk::Buffer,
    device: ash::Device,
}

impl Drop for Target {
    fn drop(&mut self) {
        // SAFETY: the buffer and memory this target created, unused by any pending work (`render_to` waits);
        // the hal wrapper owns no memory (`Buffer::from_raw`) and is dropped without destroying the raw buffer.
        unsafe {
            drop(self.buf.take());
            self.device.destroy_buffer(self.raw, None);
            self.device.free_memory(self.mem, None);
        }
    }
}

/// Times of one GPU frame, in seconds (wall only: lavapipe's timestamps would be CPU time anyway).
#[derive(Clone, Copy, Debug, Default)]
pub struct GpuTime {
    pub wall: f64,
    pub gpu: f64,
}

const STORAGE: wgt::BufferUses = wgt::BufferUses::STORAGE_READ_WRITE;

impl Gpu {
    /// The first Vulkan adapter (lavapipe where it is the only ICD) and the kernel's pipeline from `spirv`
    /// (normally `crate::KERNEL_SPIRV`). `None` without a Vulkan adapter.
    pub fn new(spirv: &[u8]) -> Option<Gpu> {
        // SAFETY (this function): every hal call gets descriptors referencing live objects of this device; the
        // pipeline layout matches the bindings the SPIR-V declares (group 0, bindings 0..5, as kernel.rs).
        unsafe {
            let desc = hal::InstanceDescriptor {
                name: "bough-kernel",
                flags: wgt::InstanceFlags::from_build_config().with_env(),
                memory_budget_thresholds: Default::default(),
                backend_options: Default::default(),
                telemetry: None,
                display: None,
            };
            let instance = <Api as hal::Api>::Instance::init(&desc).ok()?;
            let mut adapters = instance.enumerate_adapters(None);
            if adapters.is_empty() {
                return None;
            }
            let exposed = adapters.swap_remove(0);
            let limits = exposed.capabilities.limits.clone();
            // VK_EXT_external_memory_host when the device has it (lavapipe does)
            let raw_instance = exposed.adapter.shared_instance().raw_instance().clone();
            let phys = exposed.adapter.raw_physical_device();
            let ext = ash::ext::external_memory_host::NAME;
            let has_import = raw_instance.enumerate_device_extension_properties(phys).map(|v| v.iter().any(|e| e.extension_name_as_c_str() == Ok(ext))).unwrap_or(false);
            let cb: Option<Box<hal::vulkan::CreateDeviceCallback<'_>>> = if has_import {
                Some(Box::new(move |args: hal::vulkan::CreateDeviceCallbackArgs<'_, '_, '_>| args.extensions.push(ext)))
            } else {
                None
            };
            let hal::OpenDevice { device, queue } = exposed.adapter.open_with_callback(wgt::Features::empty(), &limits, &wgt::MemoryHints::Performance, cb).ok()?;
            let host_import = if has_import {
                let mut p = vk::PhysicalDeviceExternalMemoryHostPropertiesEXT::default();
                let mut p2 = vk::PhysicalDeviceProperties2::default().push_next(&mut p);
                raw_instance.get_physical_device_properties2(phys, &mut p2);
                let align = p.min_imported_host_pointer_alignment;
                Some((ash::ext::external_memory_host::Device::new(&raw_instance, device.raw_device()), align))
            } else {
                None
            };
            let entries: Vec<wgt::BindGroupLayoutEntry> = (0..5)
                .map(|b| wgt::BindGroupLayoutEntry {
                    binding: b,
                    visibility: wgt::ShaderStages::COMPUTE,
                    ty: wgt::BindingType::Buffer { ty: wgt::BufferBindingType::Storage { read_only: b != 3 }, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                })
                .collect();
            let bgl = device.create_bind_group_layout(&hal::BindGroupLayoutDescriptor { label: None, flags: hal::BindGroupLayoutFlags::empty(), entries: &entries }).ok()?;
            let layout = device.create_pipeline_layout(&hal::PipelineLayoutDescriptor { label: None, flags: hal::PipelineLayoutFlags::empty(), bind_group_layouts: &[Some(&bgl)], immediate_size: 0 }).ok()?;
            let words = spirv_words(spirv);
            let module = device.create_shader_module(&hal::ShaderModuleDescriptor { label: None, runtime_checks: wgt::ShaderRuntimeChecks::unchecked() }, hal::ShaderInput::SpirV(&words)).unwrap_or_else(|e| panic!("kernel SPIR-V: {e:?}"));
            let consts = Default::default();
            let pso = device
                .create_compute_pipeline(&hal::ComputePipelineDescriptor {
                    label: None,
                    layout: &layout,
                    stage: hal::ProgrammableStage { module: &module, entry_point: "fs_ktile", constants: &consts, zero_initialize_workgroup_memory: false },
                    cache: None,
                })
                .unwrap_or_else(|e| panic!("kernel pipeline: {e:?}"));
            let fence = device.create_fence().ok()?;
            let encoder = Some(device.create_command_encoder(&hal::CommandEncoderDescriptor { label: None, queue: &queue }).ok()?);
            Some(Gpu {
                info: exposed.info,
                device,
                queue,
                host_import,
                o: std::mem::ManuallyDrop::new(Objs { bgl, layout, module, pso, fence }),
                bufs: [None, None, None, None, None],
                encoder,
                fence_value: 0,
            })
        }
    }

    /// Buffer `i`, at least `bytes` long (grown to the next power of two; contents not kept).
    fn buf(&mut self, i: usize, bytes: usize) -> &Buf {
        let need = bytes.max(16);
        if self.bufs[i].as_ref().is_none_or(|b| b.cap < need) {
            let cap = need.next_power_of_two();
            if let Some(old) = self.bufs[i].take() {
                // SAFETY: no work is in flight (every frame is waited for).
                unsafe { self.device.destroy_buffer(old.b) };
            }
            // SAFETY: a plain mapped storage buffer of this device.
            let (b, ptr) = unsafe {
                let b = self
                    .device
                    .create_buffer(&hal::BufferDescriptor { label: None, size: cap as u64, usage: STORAGE | wgt::BufferUses::MAP_READ | wgt::BufferUses::MAP_WRITE, memory_flags: hal::MemoryFlags::PREFER_COHERENT })
                    .expect("kernel buffer");
                let m = self.device.map_buffer(&b, 0..cap as u64).expect("map");
                assert!(m.is_coherent, "kernel buffers must be coherent (the CPU writes and reads them in place)");
                (b, m.ptr)
            };
            self.bufs[i] = Some(Buf { b, ptr, cap });
        }
        self.bufs[i].as_ref().unwrap()
    }
    fn put<T: Copy>(&mut self, i: usize, data: &[T]) {
        let n = std::mem::size_of_val(data);
        let p = self.buf(i, n).ptr;
        // SAFETY: a coherent mapping of at least `n` bytes; no work is in flight.
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, p.as_ptr(), n) };
    }

    /// Upload the scene, both atlases and the program.
    pub fn upload(&mut self, scene: &Scene, atlas: &crate::cpu::Atlas, prog: &Prog) {
        self.put(0, &scene.words);
        self.put(1, &prog.words);
        self.upload_atlas(atlas.px);
        self.upload_images(atlas.img);
    }
    /// Rewrite only the program.
    pub fn upload_prog(&mut self, prog: &Prog) {
        self.put(1, &prog.words);
    }
    /// This frame's commands and program only.
    pub fn upload_frame(&mut self, scene: &Scene, prog: &Prog) {
        self.put(0, &scene.words);
        self.put(1, &prog.words);
    }
    pub fn upload_atlas(&mut self, atlas: &[u8]) {
        let p = self.buf(2, atlas.len() + 4).ptr;
        // SAFETY: as `put`.
        unsafe { std::ptr::copy_nonoverlapping(atlas.as_ptr(), p.as_ptr(), atlas.len()) };
    }
    /// The image atlas (premultiplied RGBA8 bytes).
    pub fn upload_images(&mut self, img: &[u8]) {
        self.put(4, img);
    }

    /// The caller's memory as the output buffer (the window's `wl_shm` pages), imported as device memory: the
    /// kernel writes the words the compositor reads. `None` without `VK_EXT_external_memory_host`, or when `ptr`
    /// or `len` is not a multiple of the device's import alignment.
    ///
    /// # Safety
    /// `ptr..ptr + len` must stay mapped and writable for as long as the `Target` lives, and nothing else may
    /// write it while a frame rendered to it is in flight (`render_to` waits for completion).
    pub unsafe fn wrap(&self, ptr: *mut u8, len: usize) -> Option<Target> {
        let (ext, align) = self.host_import.as_ref()?;
        if ptr as u64 % align != 0 || len as u64 % align != 0 || len == 0 {
            return None;
        }
        let raw = self.device.raw_device().clone();
        let ht = vk::ExternalMemoryHandleTypeFlags::HOST_ALLOCATION_EXT;
        // SAFETY: the caller's contract; every object created here is owned by the `Target` (or destroyed on
        // the failure paths).
        unsafe {
            let mut ext_info = vk::ExternalMemoryBufferCreateInfo::default().handle_types(ht);
            let bci = vk::BufferCreateInfo::default().size(len as u64).usage(vk::BufferUsageFlags::STORAGE_BUFFER).sharing_mode(vk::SharingMode::EXCLUSIVE).push_next(&mut ext_info);
            let buffer = raw.create_buffer(&bci, None).ok()?;
            let mut hp = vk::MemoryHostPointerPropertiesEXT::default();
            // (ash 0.38 has no wrapper for this one: the raw entry point)
            if (ext.fp().get_memory_host_pointer_properties_ext)(ext.device(), ht, ptr as *const std::ffi::c_void, &mut hp) != vk::Result::SUCCESS {
                raw.destroy_buffer(buffer, None);
                return None;
            }
            let req = raw.get_buffer_memory_requirements(buffer);
            let bits = req.memory_type_bits & hp.memory_type_bits;
            if bits == 0 {
                raw.destroy_buffer(buffer, None);
                return None;
            }
            let mut import = vk::ImportMemoryHostPointerInfoEXT::default().handle_type(ht).host_pointer(ptr as *mut std::ffi::c_void);
            let ai = vk::MemoryAllocateInfo::default().allocation_size(len as u64).memory_type_index(bits.trailing_zeros()).push_next(&mut import);
            let Ok(mem) = raw.allocate_memory(&ai, None) else {
                raw.destroy_buffer(buffer, None);
                return None;
            };
            if raw.bind_buffer_memory(buffer, mem, 0).is_err() {
                raw.destroy_buffer(buffer, None);
                raw.free_memory(mem, None);
                return None;
            }
            let buf = hal::vulkan::Buffer::from_raw(buffer);
            Some(Target { buf: Some(buf), mem, raw: buffer, device: raw })
        }
    }

    /// Run the uploaded program with `target` as the output (its stride is the program's).
    pub fn render_to(&mut self, tiles: usize, target: &Target) -> GpuTime {
        self.buf(3, 16);
        self.buf(4, 16);
        self.run(tiles, target.buf.as_ref())
    }

    /// Run the uploaded program: `tiles` workgroups into the output buffer of `out_words` words.
    pub fn render(&mut self, tiles: usize, out_words: usize) -> GpuTime {
        self.buf(3, out_words * 4);
        self.buf(4, 16);
        self.run(tiles, None)
    }

    fn run(&mut self, tiles: usize, target: Option<&HBuf>) -> GpuTime {
        let t = std::time::Instant::now();
        for i in [0, 1, 2] {
            self.buf(i, 16);
        }
        // SAFETY: the bind group references this device's live buffers (or the caller's target, alive for the
        // call); the encoder is idle (every frame waits); the group is destroyed after the wait.
        unsafe {
            let out = target.unwrap_or(&self.bufs[3].as_ref().unwrap().b);
            let bufs: Vec<&HBuf> = (0..5).map(|i| if i == 3 { out } else { &self.bufs[i].as_ref().unwrap().b }).collect();
            let bb: Vec<hal::BufferBinding<'_, HBuf>> = bufs.iter().map(|b| hal::BufferBinding::new_unchecked(*b, 0, None)).collect();
            let entries: Vec<hal::BindGroupEntry> = (0..5).map(|i| hal::BindGroupEntry { binding: i, resource_index: i, count: 1 }).collect();
            let bg = self
                .device
                .create_bind_group(&hal::BindGroupDescriptor { label: None, layout: &self.o.bgl, buffers: &bb, samplers: &[], textures: &[], entries: &entries, acceleration_structures: &[], external_textures: &[] })
                .expect("bind group");
            let mut enc = self.encoder.take().expect("encoder");
            enc.begin_encoding(None).unwrap();
            // the host's writes (coherent, before the submission) are visible to the shader by the submission;
            // the shader's writes are made visible to the host after the dispatch
            enc.begin_compute_pass(&hal::ComputePassDescriptor { label: None, timestamp_writes: None });
            enc.set_compute_pipeline(&self.o.pso);
            enc.set_bind_group(&self.o.layout, 0, &bg, &[]);
            if tiles > 0 {
                enc.dispatch([tiles as u32, 1, 1]);
            }
            enc.end_compute_pass();
            enc.transition_buffers(std::iter::once(hal::BufferBarrier { buffer: out, usage: hal::StateTransition { from: STORAGE, to: wgt::BufferUses::MAP_READ } }));
            let cb = enc.end_encoding().unwrap();
            self.fence_value += 1;
            self.queue.submit(&[&cb], &[], (&mut self.o.fence, self.fence_value)).unwrap();
            assert!(self.device.wait(&self.o.fence, self.fence_value, None).unwrap(), "GPU wait timed out");
            enc.reset_all(std::iter::once(cb));
            self.encoder = Some(enc);
            self.device.destroy_bind_group(bg);
        }
        let wall = t.elapsed().as_secs_f64();
        GpuTime { wall, gpu: wall }
    }

    /// The output buffer's first `words` words.
    pub fn out(&self, words: usize) -> &[u32] {
        let b = self.bufs[3].as_ref().expect("render first");
        assert!(b.cap >= words * 4);
        // SAFETY: a coherent mapping of at least `words` words; the GPU is idle.
        unsafe { std::slice::from_raw_parts(b.ptr.as_ptr() as *const u32, words) }
    }
    /// Fill the output with `words` (tests: a sentinel that unscheduled tiles must keep).
    pub fn set_out(&mut self, words: &[u32]) {
        self.put(3, words);
    }
    /// Zero the output.
    pub fn clear_out(&mut self, words: usize) {
        let p = self.buf(3, words * 4).ptr;
        // SAFETY: as `put`.
        unsafe { std::ptr::write_bytes(p.as_ptr(), 0, words * 4) };
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        // SAFETY: the GPU is idle (every frame waited); each object is destroyed once, users before what they use,
        // all before the device (dropped after this). `o` is not touched again.
        unsafe {
            let o = std::mem::ManuallyDrop::take(&mut self.o);
            let d = &self.device;
            self.encoder = None;
            for b in self.bufs.iter_mut().filter_map(|b| b.take()) {
                d.destroy_buffer(b.b);
            }
            d.destroy_compute_pipeline(o.pso);
            d.destroy_shader_module(o.module);
            d.destroy_pipeline_layout(o.layout);
            d.destroy_bind_group_layout(o.bgl);
            d.destroy_fence(o.fence);
        }
    }
}
