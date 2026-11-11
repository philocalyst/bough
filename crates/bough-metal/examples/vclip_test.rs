//! Debug/proof harness for the vertex-clipped stream: old frame + plane P repaired with vclip must equal a full
//! redraw of the new scene inside P and the old frame outside P.
use bough_metal::damage::{Plane, TX, TY};
use bough_metal::gpu::*;
use bough_metal::instance::*;
use bough_metal::present::Engine;
use bough_metal::scene::Rng;
use objc2_metal::*;

fn full_into(e: &Engine, tex: &Tex) {
    let cmd = e.gpu.aux.commandBuffer().unwrap();
    let mut r = bough_metal::damage::Region::full();
    r.set_full();
    e.gpu.encode_repair(&cmd, tex, &e.scene, &r, DamageMode::Bbox);
    cmd.commit();
    cmd.waitUntilCompleted();
}

fn main() {
    let mut e = Engine::new(ShaderSource::Metallib);
    let mut rng = Rng(77);
    let tex = e.gpu.new_texture(MTLStorageMode::Shared, true);
    for case in 0..6 {
        full_into(&e, &tex);
        let old = e.gpu.read_texture(&tex);
        // mutate
        match case % 3 {
            0 => e.scene.op_scattered(&mut rng, 30),
            1 => e.scene.op_move_card(&mut rng),
            _ => e.scene.op_scroll(23.0),
        }
        let want_new = e.gpu.reference(&e.scene);
        let mut p = Plane::new();
        if case < 3 {
            p.copy_from(&e.scene.acc);
        } else {
            for _ in 0..200 {
                let (x, y) = (rng.below(TX) as i32 * 16, rng.below(TY) as i32 * 16);
                p.add_rect(&IRect::new(x, y, x + 16, y + 16));
            }
            p.or_with(&e.scene.acc);
        }
        e.scene.acc.clear();
        let cmd = e.gpu.queue.commandBuffer().unwrap();
        e.gpu.encode_vclip(&cmd, &tex, &e.scene, &p);
        cmd.commit();
        cmd.waitUntilCompleted();
        let got = e.gpu.read_texture(&tex);
        let (mut bad_in, mut bad_out, mut first) = (0, 0, None);
        for y in 0..H as usize {
            for x in 0..W as usize {
                let i = (y * W as usize + x) * 4;
                let inside = p.rows[y / 16][x / 16 / 64] >> ((x / 16) % 64) & 1 == 1 && (y / 16) >= p.r0 as usize && (y / 16) <= p.r1 as usize;
                let want = if inside { &want_new[i..i + 4] } else { &old[i..i + 4] };
                if &got[i..i + 4] != want {
                    if inside { bad_in += 1 } else { bad_out += 1 }
                    if first.is_none() {
                        first = Some((x, y, inside, got[i..i + 4].to_vec(), want.to_vec()));
                    }
                }
            }
        }
        println!("case {case}: entries {}, bad inside P {bad_in}, bad outside P {bad_out}, first {:?}", e.gpu.vclip_entries.get(), first);
    }
}
