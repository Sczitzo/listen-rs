//! Listen Rust's Voice Bars -> I-Beam mark for the dashboard, window, Windows file icon and tray.

pub const IDLE: [u8; 3] = [0xDC, 0xC0, 0x9A];
const TILE: [u8; 3] = [0x24, 0x1E, 0x1A];
const RING_ALPHA: f32 = 0.28;

// Three voice-bar pills stepping into an I-beam text cursor in a 64x64 logical space.
const VOICE_BARS: [(f32, f32, f32, f32); 3] = [
    (15.6, 27.6, 36.4, 0.62),
    (23.6, 20.6, 43.4, 0.86),
    (31.6, 25.6, 38.4, 0.76),
];
const BAR_RADIUS: f32 = 2.6;

const I_BEAM: [((f32, f32), (f32, f32)); 3] = [
    ((39.5, 15.5), (50.5, 15.5)),
    ((45.0, 15.5), (45.0, 48.5)),
    ((39.5, 48.5), (50.5, 48.5)),
];
const I_BEAM_RADIUS: f32 = 2.4;

fn blend(bg: [u8; 3], fg: [u8; 3], alpha: f32) -> [u8; 3] {
    [0, 1, 2].map(|i| {
        let value = f32::from(bg[i]) * (1.0 - alpha) + f32::from(fg[i]) * alpha;
        value.round().clamp(0.0, 255.0) as u8
    })
}

fn in_capsule(x: f32, y: f32, a: (f32, f32), b: (f32, f32), radius: f32) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq > 0.0 {
        (((x - a.0) * dx + (y - a.1) * dy) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (ex, ey) = (x - (a.0 + t * dx), y - (a.1 + t * dy));
    ex * ex + ey * ey <= radius * radius
}

fn tile_sdf(x: f32, y: f32) -> f32 {
    // Rounded tile x=2..62, y=2..62, corner radius 15 (inner half-extent 15 from center 32).
    let qx = (x - 32.0).abs() - 15.0;
    let qy = (y - 32.0).abs() - 15.0;
    (qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0)) - 15.0
}

fn sample(x: f32, y: f32, color: [u8; 3]) -> Option<[u8; 3]> {
    let sdf = tile_sdf(x, y);
    if sdf > 0.0 {
        return None;
    }
    for &(a, b) in &I_BEAM {
        if in_capsule(x, y, a, b, I_BEAM_RADIUS) {
            return Some(color);
        }
    }
    for &(cx, y0, y1, alpha) in &VOICE_BARS {
        if in_capsule(x, y, (cx, y0), (cx, y1), BAR_RADIUS) {
            return Some(blend(TILE, color, alpha));
        }
    }
    if sdf >= -2.0 {
        return Some(blend(TILE, color, RING_ALPHA));
    }
    Some(TILE)
}

/// Straight-alpha pixels with 4×4 samples per pixel for smooth edges at tray size.
pub fn rgba(size: u32, color: [u8; 3]) -> Vec<u8> {
    const SAMPLES: u32 = 4;
    let scale = 64.0 / (size * SAMPLES) as f32;
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let mut sum = [0u32; 3];
            let mut covered = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let u = ((x * SAMPLES + sx) as f32 + 0.5) * scale;
                    let v = ((y * SAMPLES + sy) as f32 + 0.5) * scale;
                    if let Some(rgb) = sample(u, v, color) {
                        for (total, channel) in sum.iter_mut().zip(rgb) {
                            *total += u32::from(channel);
                        }
                        covered += 1;
                    }
                }
            }
            pixels.extend(sum.map(|total| (total / covered.max(1)) as u8));
            pixels.push((255 * covered / (SAMPLES * SAMPLES)) as u8);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_icon_keeps_its_mark_and_recolors_glyph_when_tray_state_changes() {
        let recording = [0xe5, 0x48, 0x4d];
        let idle_16 = rgba(16, IDLE);
        let rec_16 = rgba(16, recording);
        assert_eq!(idle_16.len(), 16 * 16 * 4);
        assert_eq!(&idle_16[..4], &[0, 0, 0, 0]);
        assert_eq!(&rec_16[..4], &[0, 0, 0, 0]);

        // Dark charcoal tile interior stays dark across tray state changes.
        let tile_16 = (4 * 16 + 2) * 4;
        assert_eq!(&idle_16[tile_16..tile_16 + 4], &[TILE[0], TILE[1], TILE[2], 255]);
        assert_eq!(&rec_16[tile_16..tile_16 + 4], &idle_16[tile_16..tile_16 + 4]);

        // At 32x32 tray size, the I-beam stem and tallest voice bar take the active state color.
        let idle_32 = rgba(32, IDLE);
        let rec_32 = rgba(32, recording);
        let ibeam_32 = (16 * 32 + 22) * 4;
        assert_eq!(&idle_32[ibeam_32..ibeam_32 + 4], &[IDLE[0], IDLE[1], IDLE[2], 255]);
        assert_eq!(
            &rec_32[ibeam_32..ibeam_32 + 4],
            &[recording[0], recording[1], recording[2], 255]
        );

        let bar2_32 = (16 * 32 + 11) * 4;
        let expected_idle_bar = blend(TILE, IDLE, 0.86);
        let expected_rec_bar = blend(TILE, recording, 0.86);
        assert_eq!(
            &idle_32[bar2_32..bar2_32 + 4],
            &[expected_idle_bar[0], expected_idle_bar[1], expected_idle_bar[2], 255]
        );
        assert_eq!(
            &rec_32[bar2_32..bar2_32 + 4],
            &[expected_rec_bar[0], expected_rec_bar[1], expected_rec_bar[2], 255]
        );
    }
}
