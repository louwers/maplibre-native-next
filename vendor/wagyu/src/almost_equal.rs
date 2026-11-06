// Ports vendor/wagyu/include/mapbox/geometry/wagyu/almost_equal.hpp
// (Google Test's FloatingPoint<double>::AlmostEquals, kMaxUlps = 4).

const K_MAX_ULPS: u64 = 4;
const K_SIGN_BIT_MASK: u64 = 1 << 63;

/// `SignAndMagnitudeToBiased`.
fn sign_and_magnitude_to_biased(sam: u64) -> u64 {
    if K_SIGN_BIT_MASK & sam != 0 {
        // sam represents a negative number.
        (!sam).wrapping_add(1)
    } else {
        // sam represents a positive number.
        K_SIGN_BIT_MASK | sam
    }
}

/// `DistanceBetweenSignAndMagnitudeNumbers`.
fn distance_between_sign_and_magnitude_numbers(sam1: u64, sam2: u64) -> u64 {
    let biased1 = sign_and_magnitude_to_biased(sam1);
    let biased2 = sign_and_magnitude_to_biased(sam2);
    biased1.abs_diff(biased2)
}

/// `FloatingPoint<double>(x).AlmostEquals(FloatingPoint<double>(y))`: at
/// most 4 ULPs apart; false for NaNs; +0.0 and -0.0 are equal.
pub(crate) fn almost_equals(x: f64, y: f64) -> bool {
    // The IEEE standard says that any comparison operation involving
    // a NAN must return false.
    if x.is_nan() || y.is_nan() {
        return false;
    }
    distance_between_sign_and_magnitude_numbers(x.to_bits(), y.to_bits()) <= K_MAX_ULPS
}
