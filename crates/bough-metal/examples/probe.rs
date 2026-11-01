//! Do CAMetalLayer drawable contents survive reuse? Identify drawables, write a counter pattern, read back.
//! usage: probe [frames=240] [framebufferOnly=0|1]   (runs in a real window at display rate)
use bough_metal::app;
use bough_metal::gpu::*;
use bough_metal::present::*;
use objc2::runtime::ProtocolObject;
use objc2_metal::*;
use objc2_quartz_core::CAMetalDrawable;
use std::collections::HashMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let frames: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(240);
    let fb_only = args.get(2).map(|s| s == "1").unwrap_or(false);
    let stride: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1); // act on every Nth display-link tick
    let maxd: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(3);
    let mut tick_no = 0u32;
    let mut st: Option<(Engine, objc2::rc::Retained<objc2_quartz_core::CAMetalLayer>, u32)> = None;
    // per iosurface id: (counter written last, tex ptr set)
    let mut last_written: HashMap<usize, u32> = HashMap::new();
    let mut ptrs: HashMap<usize, std::collections::HashSet<usize>> = HashMap::new();
    let mut ids_seq: Vec<usize> = vec![];
    let (mut preserved, mut lost, mut checked, mut partial) = (0u32, 0u32, 0u32, 0u32);
    let mut seen_ptr_total = std::collections::HashSet::new();
    app::run("bough-metal probe", move |host, _link| {
        if st.is_none() {
            let e = Engine::new(ShaderSource::Metallib);
            let l = make_metal_layer(&e.gpu.device, fb_only);
            host.attach(&objc2::rc::Retained::into_super(l.clone()));
            st = Some((e, l, 0));
        }
        tick_no += 1;
        if tick_no % stride != 0 {
            return;
        }
        let (e, layer, n) = st.as_mut().unwrap();
        layer.setMaximumDrawableCount(maxd);
        *n += 1;
        let drawable = layer.nextDrawable().unwrap();
        let tex = drawable.texture();
        let io_id = tex.iosurface().map(|s| s.id() as usize).unwrap_or(0);
        let ptr = objc2::rc::Retained::as_ptr(&tex) as *const u8 as usize;
        seen_ptr_total.insert(ptr);
        ptrs.entry(io_id).or_default().insert(ptr);
        ids_seq.push(io_id);
        // 1) read back what the drawable holds on acquisition (needs framebufferOnly=false)
        if !fb_only {
            if let Some(&expect) = last_written.get(&io_id) {
                let px = e.gpu.read_texture(&tex);
                let want = [(expect & 0xff) as u8, ((expect >> 8) & 0xff) as u8, ((expect >> 16) & 0xff) as u8, 255];
                let ok = px.chunks_exact(4).filter(|c| **c == want).count();
                checked += 1;
                if ok == px.len() / 4 {
                    preserved += 1;
                } else if ok == 0 {
                    lost += 1;
                } else {
                    partial += 1;
                }
            }
        }
        // 2) write a new counter pattern: clear to colour(counter) (loadAction=Clear)
        let counter = *n;
        let pd = MTLRenderPassDescriptor::renderPassDescriptor();
        let c0 = unsafe { pd.colorAttachments().objectAtIndexedSubscript(0) };
        c0.setTexture(Some(&tex));
        c0.setLoadAction(MTLLoadAction::Clear);
        c0.setStoreAction(MTLStoreAction::Store);
        let (b, g, r) = ((counter & 0xff) as f64 / 255.0, ((counter >> 8) & 0xff) as f64 / 255.0, ((counter >> 16) & 0xff) as f64 / 255.0);
        c0.setClearColor(MTLClearColor { red: r, green: g, blue: b, alpha: 1.0 });
        let cmd = e.gpu.queue.commandBuffer().unwrap();
        let enc = cmd.renderCommandEncoderWithDescriptor(&pd).unwrap();
        enc.endEncoding();
        cmd.presentDrawable(ProtocolObject::from_ref(&*drawable));
        cmd.commit();
        last_written.insert(io_id, counter);
        if *n as usize >= frames {
            let mut seq: Vec<String> = ids_seq.iter().take(12).map(|x| x.to_string()).collect();
            seq.dedup();
            println!("framebufferOnly={fb_only} stride={stride} maxDrawables={maxd}: frames {}", *n);
            println!("distinct IOSurface ids: {} ; distinct MTLTexture object pointers: {} ; per-id pointer counts: {:?}", last_written.len(), seen_ptr_total.len(), ptrs.values().map(|s| s.len()).collect::<Vec<_>>());
            println!("first acquisitions (iosurface ids): {}", seq.join(" "));
            println!("reacquisition checks: {checked}; contents preserved exactly: {preserved}; entirely lost: {lost}; partial: {partial}");
            host.close();
            std::process::exit(0);
        }
    });
}
