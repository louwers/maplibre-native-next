// Ports the FT_INT64 branch of ../maplibre-native/vendor/freetype/src/base/ftcalc.c
// (FT_MulDiv, FT_DivFix, FT_MulFix). `FT_Long` is 64-bit on Linux x86-64.

/// `FT_MOVE_SIGN`.
#[inline]
fn move_sign(x: i64, s: &mut i32) -> u64 {
    if x < 0 {
        *s = -*s;
        0u64.wrapping_sub(x as u64)
    } else {
        x as u64
    }
}

/// `NEG_LONG`.
#[inline]
fn neg_long(x: i64) -> i64 {
    0u64.wrapping_sub(x as u64) as i64
}

/// `FT_MulDiv(a, b, c)`: `(a * b + c / 2) / c`, rounding the magnitude.
pub fn mul_div(a: i64, b: i64, c: i64) -> i64 {
    let mut s = 1;
    let a = move_sign(a, &mut s);
    let b = move_sign(b, &mut s);
    let c = move_sign(c, &mut s);
    let d = if c > 0 { a.wrapping_mul(b).wrapping_add(c >> 1) / c } else { 0x7FFF_FFFF };
    let d = d as i64;
    if s < 0 { neg_long(d) } else { d }
}

/// `FT_DivFix(a, b)`: `(a << 16 + b / 2) / b`.
pub fn div_fix(a: i64, b: i64) -> i64 {
    let mut s = 1;
    let a = move_sign(a, &mut s);
    let b = move_sign(b, &mut s);
    let q = if b > 0 { (a << 16).wrapping_add(b >> 1) / b } else { 0x7FFF_FFFF };
    let q = q as i64;
    if s < 0 { neg_long(q) } else { q }
}

/// `FT_MulFix(a, b)` (the C version; the x86-64 inline assembly computes
/// the same, truncated to 32 bits).
pub fn mul_fix(a: i64, b: i64) -> i64 {
    let ab = (a as i32 as i64).wrapping_mul(b as i32 as i64);
    ((ab + 0x8000 - (ab < 0) as i64) >> 16) as i32 as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_point() {
        assert_eq!(div_fix(1536, 2048), 49152);
        assert_eq!(div_fix(1536, 1000), 100663);
        assert_eq!(mul_div(500, 100663, 64), 786430);
        assert_eq!(mul_div(-3, 5, 2), -8);
        assert_eq!(mul_fix(1 << 16, 3), 3);
        assert_eq!(mul_fix(-(1 << 15), 1), -1); // -0.5 rounds away from zero
    }
}
