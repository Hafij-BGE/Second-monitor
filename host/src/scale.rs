//! Shrinks frames that are bigger than the viewer's decoder can handle.

use rayon::prelude::*;

use crate::backend::Frame;

/// Largest even size with the frame's aspect ratio that fits in `max_w`x`max_h`.
pub fn fit_size(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if w <= max_w && h <= max_h {
        return (w, h);
    }
    let s = (max_w as f64 / w as f64).min(max_h as f64 / h as f64);
    let nw = ((w as f64 * s) as u32).max(2) & !1;
    let nh = ((h as f64 * s) as u32).max(2) & !1;
    (nw, nh)
}

/// Bilinear resize of a tightly packed BGRA frame.
pub fn resize(src: &Frame, dw: u32, dh: u32) -> Frame {
    let (sw, sh) = (src.width as usize, src.height as usize);
    let (dw, dh) = (dw as usize, dh as usize);
    let x_ratio = sw as f32 / dw as f32;
    let y_ratio = sh as f32 / dh as f32;
    let mut out = vec![0u8; dw * dh * 4];
    out.par_chunks_mut(dw * 4).enumerate().for_each(|(y, row)| {
        let fy = ((y as f32 + 0.5) * y_ratio - 0.5).clamp(0.0, (sh - 1) as f32);
        let y0 = fy as usize;
        let y1 = (y0 + 1).min(sh - 1);
        let wy = fy - y0 as f32;
        let r0 = &src.bgra[y0 * sw * 4..(y0 + 1) * sw * 4];
        let r1 = &src.bgra[y1 * sw * 4..(y1 + 1) * sw * 4];
        for x in 0..dw {
            let fx = ((x as f32 + 0.5) * x_ratio - 0.5).clamp(0.0, (sw - 1) as f32);
            let x0 = fx as usize;
            let x1 = (x0 + 1).min(sw - 1);
            let wx = fx - x0 as f32;
            for c in 0..4 {
                let top = r0[x0 * 4 + c] as f32 * (1.0 - wx) + r0[x1 * 4 + c] as f32 * wx;
                let bottom = r1[x0 * 4 + c] as f32 * (1.0 - wx) + r1[x1 * 4 + c] as f32 * wx;
                row[x * 4 + c] = (top * (1.0 - wy) + bottom * wy + 0.5) as u8;
            }
        }
    });
    Frame {
        width: dw as u32,
        height: dh as u32,
        bgra: out,
    }
}
