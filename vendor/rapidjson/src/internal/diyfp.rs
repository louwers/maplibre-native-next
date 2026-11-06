// Ports include/rapidjson/internal/diyfp.h (RapidJSON, (C) 2015 THL A29
// Limited, a Tencent company, and Milo Yip, MIT License): the Grisu2
// "do-it-yourself floating point" of Loitsch, "Printing floating-point
// numbers quickly and accurately with integers" (PLDI 2010).
//
// All `uint64_t` arithmetic wraps, as in C++.

pub const DIY_SIGNIFICAND_SIZE: i32 = 64;
pub const DP_SIGNIFICAND_SIZE: i32 = 52;
pub const DP_EXPONENT_BIAS: i32 = 0x3FF + DP_SIGNIFICAND_SIZE;
pub const DP_MAX_EXPONENT: i32 = 0x7FF - DP_EXPONENT_BIAS;
pub const DP_MIN_EXPONENT: i32 = -DP_EXPONENT_BIAS;
pub const DP_DENORMAL_EXPONENT: i32 = -DP_EXPONENT_BIAS + 1;
pub const DP_EXPONENT_MASK: u64 = 0x7FF0_0000_0000_0000;
pub const DP_SIGNIFICAND_MASK: u64 = 0x000F_FFFF_FFFF_FFFF;
pub const DP_HIDDEN_BIT: u64 = 0x0010_0000_0000_0000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiyFp {
    pub f: u64,
    pub e: i32,
}

impl DiyFp {
    pub const fn new(fp: u64, exp: i32) -> Self {
        DiyFp { f: fp, e: exp }
    }

    pub fn from_f64(d: f64) -> Self {
        let u64_ = d.to_bits();
        let biased_e = ((u64_ & DP_EXPONENT_MASK) >> DP_SIGNIFICAND_SIZE) as i32;
        let significand = u64_ & DP_SIGNIFICAND_MASK;
        if biased_e != 0 {
            DiyFp { f: significand.wrapping_add(DP_HIDDEN_BIT), e: biased_e - DP_EXPONENT_BIAS }
        } else {
            DiyFp { f: significand, e: DP_MIN_EXPONENT + 1 }
        }
    }

    /// `operator-`
    pub fn sub(&self, rhs: &DiyFp) -> DiyFp {
        DiyFp::new(self.f.wrapping_sub(rhs.f), self.e)
    }

    /// `operator*`: the high 64 bits of the 128-bit product, rounded up when
    /// bit 63 of the low half is set (the `unsigned __int128` branch).
    pub fn mul(&self, rhs: &DiyFp) -> DiyFp {
        let p = (self.f as u128) * (rhs.f as u128);
        let mut h = (p >> 64) as u64;
        let l = p as u64;
        if l & (1u64 << 63) != 0 {
            // rounding
            h = h.wrapping_add(1);
        }
        DiyFp::new(h, self.e + rhs.e + 64)
    }

    pub fn normalize(&self) -> DiyFp {
        debug_assert!(self.f != 0); // clzll's precondition
        let s = self.f.leading_zeros() as i32;
        DiyFp::new(self.f << s, self.e - s)
    }

    pub fn normalize_boundary(&self) -> DiyFp {
        let mut res = *self;
        while res.f & (DP_HIDDEN_BIT << 1) == 0 {
            res.f <<= 1;
            res.e -= 1;
        }
        res.f <<= DIY_SIGNIFICAND_SIZE - DP_SIGNIFICAND_SIZE - 2;
        res.e -= DIY_SIGNIFICAND_SIZE - DP_SIGNIFICAND_SIZE - 2;
        res
    }

    /// Returns `(minus, plus)`.
    pub fn normalized_boundaries(&self) -> (DiyFp, DiyFp) {
        let pl = DiyFp::new((self.f << 1).wrapping_add(1), self.e - 1).normalize_boundary();
        let mut mi = if self.f == DP_HIDDEN_BIT {
            DiyFp::new((self.f << 2).wrapping_sub(1), self.e - 2)
        } else {
            DiyFp::new((self.f << 1).wrapping_sub(1), self.e - 1)
        };
        mi.f <<= mi.e - pl.e;
        mi.e = pl.e;
        (mi, pl)
    }

    pub fn to_double(&self) -> f64 {
        debug_assert!(self.f <= DP_HIDDEN_BIT + DP_SIGNIFICAND_MASK);
        if self.e < DP_DENORMAL_EXPONENT {
            // Underflow.
            return 0.0;
        }
        if self.e >= DP_MAX_EXPONENT {
            // Overflow.
            return f64::INFINITY;
        }
        let be: u64 = if self.e == DP_DENORMAL_EXPONENT && (self.f & DP_HIDDEN_BIT) == 0 {
            0
        } else {
            (self.e + DP_EXPONENT_BIAS) as u64
        };
        f64::from_bits((self.f & DP_SIGNIFICAND_MASK) | (be << DP_SIGNIFICAND_SIZE))
    }
}

// 10^-348, 10^-340, ..., 10^340
const CACHED_POWERS_F: [u64; 87] = [
    0xfa8fd5a0_081c0288,
    0xbaaee17f_a23ebf76,
    0x8b16fb20_3055ac76,
    0xcf42894a_5dce35ea,
    0x9a6bb0aa_55653b2d,
    0xe61acf03_3d1a45df,
    0xab70fe17_c79ac6ca,
    0xff77b1fc_bebcdc4f,
    0xbe5691ef_416bd60c,
    0x8dd01fad_907ffc3c,
    0xd3515c28_31559a83,
    0x9d71ac8f_ada6c9b5,
    0xea9c2277_23ee8bcb,
    0xaecc4991_4078536d,
    0x823c1279_5db6ce57,
    0xc2109436_4dfb5637,
    0x9096ea6f_3848984f,
    0xd77485cb_25823ac7,
    0xa086cfcd_97bf97f4,
    0xef340a98_172aace5,
    0xb23867fb_2a35b28e,
    0x84c8d4df_d2c63f3b,
    0xc5dd4427_1ad3cdba,
    0x936b9fce_bb25c996,
    0xdbac6c24_7d62a584,
    0xa3ab6658_0d5fdaf6,
    0xf3e2f893_dec3f126,
    0xb5b5ada8_aaff80b8,
    0x87625f05_6c7c4a8b,
    0xc9bcff60_34c13053,
    0x964e858c_91ba2655,
    0xdff97724_70297ebd,
    0xa6dfbd9f_b8e5b88f,
    0xf8a95fcf_88747d94,
    0xb9447093_8fa89bcf,
    0x8a08f0f8_bf0f156b,
    0xcdb02555_653131b6,
    0x993fe2c6_d07b7fac,
    0xe45c10c4_2a2b3b06,
    0xaa242499_697392d3,
    0xfd87b5f2_8300ca0e,
    0xbce50864_92111aeb,
    0x8cbccc09_6f5088cc,
    0xd1b71758_e219652c,
    0x9c400000_00000000,
    0xe8d4a510_00000000,
    0xad78ebc5_ac620000,
    0x813f3978_f8940984,
    0xc097ce7b_c90715b3,
    0x8f7e32ce_7bea5c70,
    0xd5d238a4_abe98068,
    0x9f4f2726_179a2245,
    0xed63a231_d4c4fb27,
    0xb0de6538_8cc8ada8,
    0x83c7088e_1aab65db,
    0xc45d1df9_42711d9a,
    0x924d692c_a61be758,
    0xda01ee64_1a708dea,
    0xa26da399_9aef774a,
    0xf209787b_b47d6b85,
    0xb454e4a1_79dd1877,
    0x865b8692_5b9bc5c2,
    0xc83553c5_c8965d3d,
    0x952ab45c_fa97a0b3,
    0xde469fbd_99a05fe3,
    0xa59bc234_db398c25,
    0xf6c69a72_a3989f5c,
    0xb7dcbf53_54e9bece,
    0x88fcf317_f22241e2,
    0xcc20ce9b_d35c78a5,
    0x98165af3_7b2153df,
    0xe2a0b5dc_971f303a,
    0xa8d9d153_5ce3b396,
    0xfb9b7cd9_a4a7443c,
    0xbb764c4c_a7a44410,
    0x8bab8eef_b6409c1a,
    0xd01fef10_a657842c,
    0x9b10a4e5_e9913129,
    0xe7109bfb_a19c0c9d,
    0xac2820d9_623bf429,
    0x80444b5e_7aa7cf85,
    0xbf21e440_03acdd2d,
    0x8e679c2f_5e44ff8f,
    0xd433179d_9c8cb841,
    0x9e19db92_b4e31ba9,
    0xeb96bf6e_badf77d9,
    0xaf87023b_9bf0ee6b,
];

const CACHED_POWERS_E: [i16; 87] = [
    -1220, -1193, -1166, -1140, -1113, -1087, -1060, -1034, -1007, -980, -954, -927, -901, -874, -847, -821, -794,
    -768, -741, -715, -688, -661, -635, -608, -582, -555, -529, -502, -475, -449, -422, -396, -369, -343, -316, -289,
    -263, -236, -210, -183, -157, -130, -103, -77, -50, -24, 3, 30, 56, 83, 109, 136, 162, 189, 216, 242, 269, 295,
    322, 348, 375, 402, 428, 455, 481, 508, 534, 561, 588, 614, 641, 667, 694, 720, 747, 774, 800, 827, 853, 880, 907,
    933, 960, 986, 1013, 1039, 1066,
];

pub fn get_cached_power_by_index(index: usize) -> DiyFp {
    debug_assert!(index < 87);
    DiyFp::new(CACHED_POWERS_F[index], CACHED_POWERS_E[index] as i32)
}

/// Returns `(power, K)`.
pub fn get_cached_power(e: i32) -> (DiyFp, i32) {
    //int k = static_cast<int>(ceil((-61 - e) * 0.30102999566398114)) + 374;
    let dk = (-61 - e) as f64 * 0.30102999566398114 + 347.0; // dk must be positive, so can do ceiling in positive
    let mut k = dk as i32;
    if dk - k as f64 > 0.0 {
        k += 1;
    }

    let index = ((k >> 3) + 1) as u32;
    let big_k = -(-348 + (index << 3) as i32); // decimal exponent no need lookup table

    (get_cached_power_by_index(index as usize), big_k)
}

/// Returns `(power, outExp)`.
pub fn get_cached_power10(exp: i32) -> (DiyFp, i32) {
    debug_assert!(exp >= -348);
    let index = (exp + 348) as u32 / 8;
    let out_exp = -348 + index as i32 * 8;
    (get_cached_power_by_index(index as usize), out_exp)
}
