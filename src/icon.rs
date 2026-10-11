//! The app icon, drawn procedurally: three fader slots on a rounded dark tile,
//! with a glowing wire running through the caps.

/// RGBA pixels of a `size`×`size` icon.
pub fn rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut px = vec![0u8; (size * size * 4) as usize];
    let radius = s * 0.22;
    let slots = [0.28f32, 0.5, 0.72];
    let caps = [0.62f32, 0.34, 0.5];
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Rounded square coverage (1px anti-aliasing).
            let dx = (fx - s / 2.0).abs() - (s / 2.0 - radius);
            let dy = (fy - s / 2.0).abs() - (s / 2.0 - radius);
            let dist = dx.max(0.0).hypot(dy.max(0.0)) - radius;
            let alpha = (0.5 - dist).clamp(0.0, 1.0);
            if alpha <= 0.0 {
                continue;
            }
            // Background gradient.
            let t = fy / s;
            let mut c = [lerp(38.0, 22.0, t), lerp(40.0, 24.0, t), lerp(56.0, 34.0, t)];
            for sx in slots {
                // Slot.
                if (fx - sx * s).abs() < s * 0.035 && fy > s * 0.18 && fy < s * 0.82 {
                    c = [70.0, 74.0, 96.0];
                }
            }
            // The wire: a polyline from the left edge through every cap to the right edge.
            let mut pts = vec![(0.08 * s, caps[0] * s)];
            pts.extend(slots.iter().zip(caps).map(|(x, y)| (x * s, y * s)));
            pts.push((0.92 * s, caps[2] * s));
            let d = pts.windows(2).map(|w| seg_dist((fx, fy), w[0], w[1])).fold(f32::MAX, f32::min);
            let wire = (s * 0.022 + 0.5 - d).clamp(0.0, 1.0);
            let glow = (1.0 - d / (s * 0.09)).clamp(0.0, 1.0).powi(2) * 0.35;
            let k = wire.max(glow);
            for (ch, w) in c.iter_mut().zip([110.0, 230.0, 255.0]) {
                *ch = lerp(*ch, w, k);
            }
            for (i, sx) in slots.iter().enumerate() {
                let cx = sx * s;
                // Cap.
                let cy = caps[i] * s;
                if (fx - cx).abs() < s * 0.09 && (fy - cy).abs() < s * 0.055 {
                    c = if i == 1 { [255.0, 140.0, 66.0] } else { [92.0, 200.0, 255.0] };
                }
            }
            let o = ((y * size + x) * 4) as usize;
            px[o] = c[0] as u8;
            px[o + 1] = c[1] as u8;
            px[o + 2] = c[2] as u8;
            px[o + 3] = (alpha * 255.0) as u8;
        }
    }
    px
}

fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * abx + (p.1 - a.1) * aby) / (abx * abx + aby * aby)).clamp(0.0, 1.0);
    (p.0 - a.0 - abx * t).hypot(p.1 - a.1 - aby * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn window_icon() -> eframe::egui::IconData {
    eframe::egui::IconData { rgba: rgba(128), width: 128, height: 128 }
}

/// Tray icons use ARGB32 in network byte order.
pub fn tray_icon() -> ksni::Icon {
    let size = 64;
    let mut data = rgba(size);
    for p in data.chunks_exact_mut(4) {
        p.rotate_right(1);
    }
    ksni::Icon { width: size as i32, height: size as i32, data }
}

/// Write a PNG-free icon for the desktop entry: a tiny uncompressed PNG
/// encoder is overkill, so we emit an SVG with the same design.
pub fn svg() -> String {
    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128">
  <defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#262838"/><stop offset="1" stop-color="#161822"/></linearGradient></defs>
  <rect width="128" height="128" rx="28" fill="url(#g)"/>
  <g fill="#464a60"><rect x="31.3" y="23" width="9" height="82" rx="4.5"/>
    <rect x="59.5" y="23" width="9" height="82" rx="4.5"/><rect x="87.7" y="23" width="9" height="82" rx="4.5"/></g>
  <polyline points="10,79.4 35.8,79.4 64,43.5 92.2,64 118,64" fill="none" stroke="#6ee6ff" stroke-width="12" stroke-opacity="0.18" stroke-linejoin="round" stroke-linecap="round"/>
  <polyline points="10,79.4 35.8,79.4 64,43.5 92.2,64 118,64" fill="none" stroke="#6ee6ff" stroke-width="5.6" stroke-linejoin="round" stroke-linecap="round"/>
  <rect x="24.3" y="72.3" width="23" height="14" rx="4" fill="#5cc8ff"/>
  <rect x="52.5" y="36.5" width="23" height="14" rx="4" fill="#ff8c42"/>
  <rect x="80.7" y="57" width="23" height="14" rx="4" fill="#5cc8ff"/>
</svg>
"##
    .into()
}

#[cfg(test)]
mod tests {
    /// Packages install `assets/slidewire.svg`; regenerate it with
    /// `SLIDEWIRE_BLESS=1 cargo test` after changing the icon.
    #[test]
    fn packaged_icon_matches() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/slidewire.svg");
        if std::env::var_os("SLIDEWIRE_BLESS").is_some() {
            std::fs::write(path, super::svg()).unwrap();
        }
        assert_eq!(std::fs::read_to_string(path).unwrap(), super::svg(), "run SLIDEWIRE_BLESS=1 cargo test");
    }
}
