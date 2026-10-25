//! The tray icon (two stacked panels), drawn at runtime so it stays crisp at
//! any size without extra image assets.

/// Top-down BGRA pixels with straight alpha, `size` x `size`.
pub fn tray_icon_pixels(size: usize) -> Vec<u8> {
    let mut buf = vec![0u8; size * size * 4];
    let s = size as f32 / 16.0;
    let mut rect = |x: f32, y: f32, w: f32, h: f32, bgra: [u8; 4]| {
        let px = |v: f32| ((v * s).round() as usize).min(size);
        for py in px(y)..px(y + h) {
            for px_ in px(x)..px(x + w) {
                let i = (py * size + px_) * 4;
                buf[i..i + 4].copy_from_slice(&bgra);
            }
        }
    };
    let accent = [0xf0, 0x9c, 0x2e, 0xff]; // #2e9cf0
    let fill = [0xf0, 0x9c, 0x2e, 0x70];
    let light = [0xff, 0xff, 0xff, 0xff];
    // Top panel: title bar, body and two icons.
    rect(1.0, 1.0, 14.0, 2.0, accent);
    rect(1.0, 3.0, 14.0, 5.0, fill);
    rect(3.0, 4.0, 3.0, 3.0, light);
    rect(8.0, 4.0, 3.0, 3.0, light);
    // Bottom panel, collapsed (title bar only).
    rect(1.0, 10.0, 14.0, 2.0, accent);
    rect(1.0, 13.0, 9.0, 2.0, accent);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_the_accent_bar_and_leaves_the_corner_clear() {
        let px = tray_icon_pixels(16);
        assert_eq!(px.len(), 16 * 16 * 4);
        assert_eq!(&px[0..4], &[0, 0, 0, 0]);
        let i = (16 + 1) * 4;
        assert_eq!(&px[i..i + 4], &[0xf0, 0x9c, 0x2e, 0xff]);
        assert_eq!(tray_icon_pixels(32).len(), 32 * 32 * 4);
    }
}
