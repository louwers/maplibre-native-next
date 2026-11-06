// Ports test/util/camera.test.cpp (TEST(FreeCameraOptions, *)).

use std::f64::consts::SQRT_2;

use mln_util::constants::LATITUDE_MAX;
use mln_util::vectors::Vec4;

use super::*;

const ABS_DOUBLE_ERROR: f64 = 1e-7;

fn ll(lat: f64, lon: f64) -> LatLng {
    LatLng::new(lat, lon).unwrap()
}

/// gtest's `FloatingPoint<double>::AlmostEquals`: at most 4 ULPs apart,
/// NaN never equal.
fn almost_equals_f64(a: f64, b: f64) -> bool {
    if a.is_nan() || b.is_nan() {
        return false;
    }
    fn biased(x: f64) -> u64 {
        const SIGN: u64 = 1 << 63;
        let bits = x.to_bits();
        if bits & SIGN != 0 { (!bits).wrapping_add(1) } else { bits | SIGN }
    }
    let (a, b) = (biased(a), biased(b));
    a.abs_diff(b) <= 4
}

/// `ASSERT_DOUBLE_EQ(val1, val2)`.
#[track_caller]
fn assert_double_eq(val1: f64, val2: f64) {
    assert!(almost_equals_f64(val1, val2), "ASSERT_DOUBLE_EQ: {val1:?} vs {val2:?}");
}

/// `ASSERT_NEAR(val1, val2, abs_error)`.
#[track_caller]
fn assert_near(val1: f64, val2: f64, abs_error: f64) {
    let diff = (val1 - val2).abs();
    assert!(diff <= abs_error, "ASSERT_NEAR: |{val1:?} - {val2:?}| = {diff:?} > {abs_error:?}");
}

/// `ASSERT_THAT(arg, Vec3NearEquals1E7(vec))`.
#[track_caller]
fn assert_vec3_near_1e7(arg: &Vec3, vec: &Vec3) {
    let near = (vec[0] - arg[0]).abs() <= ABS_DOUBLE_ERROR
        && (vec[1] - arg[1]).abs() <= ABS_DOUBLE_ERROR
        && (vec[2] - arg[2]).abs() <= ABS_DOUBLE_ERROR;
    assert!(near, "Vec3NearEquals1E7: {arg:?} vs {vec:?}");
}

fn rotated_frame(quaternion: &Vec4) -> (Vec3, Vec3, Vec3) {
    let q = Quaternion::from_vec4(quaternion);
    (q.transform(&[1.0, 0.0, 0.0]), q.transform(&[0.0, -1.0, 0.0]), q.transform(&[0.0, 0.0, -1.0]))
}

// gtest: FreeCameraOptions.SetLocation
#[test]
fn set_location() {
    let mut options = FreeCameraOptions::default();

    options.set_location(&LatLngAltitude { location: ll(0.0, 0.0), altitude: EARTH_RADIUS_M * PI });
    assert!(options.position.is_some());
    assert_vec3_near_1e7(&options.position.unwrap(), &[0.5, 0.5, 0.5]);

    options.set_location(&LatLngAltitude { location: ll(25.0, -180.0), altitude: 1000.0 });
    assert!(options.position.is_some());
    assert_vec3_near_1e7(&options.position.unwrap(), &[0.0, 0.4282409625, 0.000027532812465]);

    options.set_location(&LatLngAltitude {
        location: ll(LATITUDE_MAX, 0.0),
        altitude: EARTH_RADIUS_M * PI * mln_math::deg2rad(LATITUDE_MAX).cos(),
    });
    assert!(options.position.is_some());
    assert_vec3_near_1e7(&options.position.unwrap(), &[0.5, 0.0, 0.5]);

    options.set_location(&LatLngAltitude {
        location: ll(-LATITUDE_MAX, 0.0),
        altitude: EARTH_RADIUS_M * PI * mln_math::deg2rad(-LATITUDE_MAX).cos(),
    });
    assert!(options.position.is_some());
    assert_vec3_near_1e7(&options.position.unwrap(), &[0.5, 1.0, 0.5]);
}

// gtest: FreeCameraOptions.SetLocationNegativeAltitude
#[test]
fn set_location_negative_altitude() {
    let mut options = FreeCameraOptions::default();
    options.set_location(&LatLngAltitude { location: ll(0.0, 0.0), altitude: -100.0 });
    assert!(options.position.is_some());
    assert_double_eq(options.position.unwrap()[2], -100.0 / M2PI / EARTH_RADIUS_M);
}

// gtest: FreeCameraOptions.SetLocationUnwrappedLocation
#[test]
fn set_location_unwrapped_location() {
    let mut options = FreeCameraOptions::default();

    options.set_location(&LatLngAltitude { location: ll(0.0, -540.0), altitude: 0.0 });
    assert!(options.position.is_some());
    assert_vec3_near_1e7(&options.position.unwrap(), &[-1.0, 0.5, 0.0]);
}

// gtest: FreeCameraOptions.GetLocation
#[test]
fn get_location() {
    let mut options = FreeCameraOptions::default();

    options.position = Some([0.5, 0.5, 0.5]);
    assert!(options.get_location().is_some());
    let lat_lng_altitude = options.get_location().unwrap();
    assert_double_eq(lat_lng_altitude.location.latitude(), 0.0);
    assert_double_eq(lat_lng_altitude.location.longitude(), 0.0);
    assert_double_eq(lat_lng_altitude.altitude, 20037508.342789244);

    options.position = Some([0.0, 0.4282409625, 0.000027532812465]);
    assert!(options.get_location().is_some());
    let lat_lng_altitude = options.get_location().unwrap();
    assert_near(lat_lng_altitude.location.latitude(), 25.0, 1e-7);
    assert_near(lat_lng_altitude.location.longitude(), -180.0, 1e-7);
    assert_near(lat_lng_altitude.altitude, 1000.0, 1e-7);

    options.position = Some([0.5, 0.0, 0.5]);
    assert!(options.get_location().is_some());
    let lat_lng_altitude = options.get_location().unwrap();
    assert_near(lat_lng_altitude.location.latitude(), LATITUDE_MAX, 1e-7);
    assert_near(lat_lng_altitude.location.longitude(), 0.0, 1e-7);
    assert_near(lat_lng_altitude.altitude, 1728570.489074, 1e-6);
}

// gtest: FreeCameraOptions.GetLocationInvalidPosition
#[test]
fn get_location_invalid_position() {
    let mut options = FreeCameraOptions::default();
    // Mercator position not set. Should return nothing
    assert!(options.get_location().is_none());

    // Invalid latitude
    options.position = Some([0.0, -0.1, 0.0]);
    assert!(options.get_location().is_none());
    options.position = Some([0.0, 2.0, 0.0]);
    assert!(options.get_location().is_none());
}

// gtest: FreeCameraOptions.GetLocationNegativeAltitude
#[test]
fn get_location_negative_altitude() {
    let mut options = FreeCameraOptions::default();
    options.position = Some([0.5, 0.5, -0.25]);
    let lat_lng_altitude = options.get_location();
    assert!(lat_lng_altitude.is_some());

    assert_double_eq(lat_lng_altitude.unwrap().altitude, -0.5 * EARTH_RADIUS_M * PI);
}

// gtest: FreeCameraOptions.GetLocationUnwrappedPosition
#[test]
fn get_location_unwrapped_position() {
    let mut options = FreeCameraOptions::default();
    options.position = Some([1.25, 0.5, 0.0]);
    let lat_lng_altitude = options.get_location();
    assert!(lat_lng_altitude.is_some());
    let lat_lng_altitude = lat_lng_altitude.unwrap();
    assert_double_eq(lat_lng_altitude.location.longitude(), 270.0);
    assert_double_eq(lat_lng_altitude.location.latitude(), 0.0);
    assert_double_eq(lat_lng_altitude.altitude, 0.0);
}

// gtest: FreeCameraOptions.LookAtPoint
#[test]
fn look_at_point() {
    let mut options = FreeCameraOptions::default();
    let cos_pi4 = 1.0 / SQRT_2;

    // Pitch: 45, bearing: 0
    options.position = Some([0.5, 0.5, 0.5]);
    options.look_at_point(&ll(LATITUDE_MAX, 0.0), None);
    assert!(options.orientation.is_some());
    let (right, up, forward) = rotated_frame(&options.orientation.unwrap());

    assert_vec3_near_1e7(&right, &[1.0, 0.0, 0.0]);
    assert_vec3_near_1e7(&up, &[0.0, -cos_pi4, cos_pi4]);
    assert_vec3_near_1e7(&forward, &[0.0, -cos_pi4, -cos_pi4]);

    // Look directly to east
    options.position = Some([0.5, 0.5, 0.0]);
    options.look_at_point(&ll(0.0, 30.0), None);
    assert!(options.orientation.is_some());
    let (right, up, forward) = rotated_frame(&options.orientation.unwrap());

    assert_vec3_near_1e7(&right, &[0.0, 1.0, 0.0]);
    assert_vec3_near_1e7(&up, &[0.0, 0.0, 1.0]);
    assert_vec3_near_1e7(&forward, &[1.0, 0.0, 0.0]);

    // Pitch: 0, bearing: 0
    options.set_location(&LatLngAltitude { location: ll(60.1699, 24.9384), altitude: 100.0 });
    options.look_at_point(&ll(60.1699, 24.9384), Some([0.0, -1.0, 0.0]));
    assert!(options.orientation.is_some());
    let (right, up, forward) = rotated_frame(&options.orientation.unwrap());

    assert_vec3_near_1e7(&right, &[1.0, 0.0, 0.0]);
    assert_vec3_near_1e7(&up, &[0.0, -1.0, 0.0]);
    assert_vec3_near_1e7(&forward, &[0.0, 0.0, -1.0]);

    // Pitch: 0, bearing: 45
    options.set_location(&LatLngAltitude { location: ll(60.1699, 24.9384), altitude: 100.0 });
    options.look_at_point(&ll(60.1699, 24.9384), Some([-1.0, -1.0, 0.0]));
    assert!(options.orientation.is_some());
    let (right, up, forward) = rotated_frame(&options.orientation.unwrap());

    assert_vec3_near_1e7(&right, &[cos_pi4, -cos_pi4, 0.0]);
    assert_vec3_near_1e7(&up, &[-cos_pi4, -cos_pi4, 0.0]);
    assert_vec3_near_1e7(&forward, &[0.0, 0.0, -1.0]);

    // Looking south, up vector almost same as forward vector
    options.set_location(&LatLngAltitude { location: ll(37.7749, 122.4194), altitude: 0.0 });
    options.look_at_point(&ll(37.5, 122.4194), Some([0.0, 1.0, 0.00001]));
    assert!(options.orientation.is_some());
    let (right, up, forward) = rotated_frame(&options.orientation.unwrap());

    assert_vec3_near_1e7(&right, &[-1.0, 0.0, 0.0]);
    assert_vec3_near_1e7(&up, &[0.0, 0.0, 1.0]);
    assert_vec3_near_1e7(&forward, &[0.0, 1.0, 0.0]);

    // Orientation with roll-component
    options.set_location(&LatLngAltitude { location: ll(-33.8688, 151.2093), altitude: 0.0 });
    options.look_at_point(&ll(-33.8688, 160.0), Some([0.0, -1.0, 0.1]));
    assert!(options.orientation.is_some());
    let (right, up, forward) = rotated_frame(&options.orientation.unwrap());

    assert_vec3_near_1e7(&right, &[0.0, 0.9714568644195829, 0.2372162738348117]);
    assert_vec3_near_1e7(&up, &[0.0, -0.23721627383481167, 0.97145686441958301]);
    assert_vec3_near_1e7(&forward, &[1.0, 0.0, 0.0]);

    // Up vector pointing downwards
    options.position = Some([0.5, 0.5, 0.5]);
    options.look_at_point(&ll(LATITUDE_MAX, 0.0), Some([0.0, 0.0, -0.5]));
    assert!(options.orientation.is_some());
    let (right, up, forward) = rotated_frame(&options.orientation.unwrap());

    assert_vec3_near_1e7(&right, &[1.0, 0.0, 0.0]);
    assert_vec3_near_1e7(&up, &[0.0, -cos_pi4, cos_pi4]);
    assert_vec3_near_1e7(&forward, &[0.0, -cos_pi4, -cos_pi4]);
}

// gtest: FreeCameraOptions.LookAtPointInvalidInput
#[test]
fn look_at_point_invalid_input() {
    let mut options = FreeCameraOptions::default();

    // Position not set
    options.orientation = Some([0.0, 0.0, 0.0, 0.0]);
    options.look_at_point(&ll(0.0, 0.0), None);
    assert!(options.orientation.is_none());

    // Target same as position
    options.orientation = Some([0.0, 0.0, 0.0, 0.0]);
    options.position = Some([0.5, 0.5, 0.0]);
    options.look_at_point(&ll(0.0, 0.0), None);
    assert!(options.orientation.is_none());

    // Camera looking directly down without an explicit up vector
    options.orientation = Some([0.0, 0.0, 0.0, 0.0]);
    options.position = Some([0.5, 0.5, 0.5]);
    options.look_at_point(&ll(0.0, 0.0), None);
    assert!(options.orientation.is_none());

    // Zero up vector
    options.orientation = Some([0.0, 0.0, 0.0, 0.0]);
    options.look_at_point(&ll(0.0, 0.0), Some([0.0, 0.0, 0.0]));
    assert!(options.orientation.is_none());

    // Up vector same as direction
    options.orientation = Some([0.0, 0.0, 0.0, 0.0]);
    options.look_at_point(&ll(0.0, 0.0), Some([0.0, 0.0, -1.0]));
    assert!(options.orientation.is_none());
}

// gtest: FreeCameraOptions.SetRollPitchBearing
#[test]
fn set_roll_pitch_bearing() {
    let mut options = FreeCameraOptions::default();

    options.set_roll_pitch_bearing(0.0, 0.0, 0.0);
    assert!(options.orientation.is_some());
    let (right, up, forward) = rotated_frame(&options.orientation.unwrap());
    assert_vec3_near_1e7(&right, &[1.0, 0.0, 0.0]);
    assert_vec3_near_1e7(&up, &[0.0, -1.0, 0.0]);
    assert_vec3_near_1e7(&forward, &[0.0, 0.0, -1.0]);

    options.set_roll_pitch_bearing(0.0, 0.0, 180.0);
    assert!(options.orientation.is_some());
    let (right, up, forward) = rotated_frame(&options.orientation.unwrap());
    assert_vec3_near_1e7(&right, &[-1.0, 0.0, 0.0]);
    assert_vec3_near_1e7(&up, &[0.0, 1.0, 0.0]);
    assert_vec3_near_1e7(&forward, &[0.0, 0.0, -1.0]);

    let cos60 = mln_math::deg2rad(60.0).cos();
    let sin60 = mln_math::deg2rad(60.0).sin();

    options.set_roll_pitch_bearing(0.0, 60.0, 0.0);
    assert!(options.orientation.is_some());
    let (right, up, forward) = rotated_frame(&options.orientation.unwrap());
    assert_vec3_near_1e7(&right, &[1.0, 0.0, 0.0]);
    assert_vec3_near_1e7(&up, &[0.0, -cos60, sin60]);
    assert_vec3_near_1e7(&forward, &[0.0, -sin60, -cos60]);

    options.set_roll_pitch_bearing(0.0, 60.0, -450.0);
    assert!(options.orientation.is_some());
    let (right, up, forward) = rotated_frame(&options.orientation.unwrap());
    assert_vec3_near_1e7(&right, &[0.0, 1.0, 0.0]);
    assert_vec3_near_1e7(&up, &[cos60, 0.0, sin60]);
    assert_vec3_near_1e7(&forward, &[sin60, 0.0, -cos60]);
}
