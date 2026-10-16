pub fn rgba() -> Vec<u8> {
    let mut pixels = vec![0; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let mark = ((6..9).contains(&x) && (9..22).contains(&y))
                || ((23..26).contains(&x) && (9..22).contains(&y))
                || ((10..14).contains(&x) && (19..24).contains(&y))
                || ((18..22).contains(&x) && (19..24).contains(&y))
                || ((14..18).contains(&x) && (14..20).contains(&y));
            let color = if mark {
                [151, 219, 174, 255]
            } else {
                [19, 23, 28, 255]
            };
            pixels[(y * 32 + x) * 4..(y * 32 + x + 1) * 4].copy_from_slice(&color);
        }
    }
    pixels
}
