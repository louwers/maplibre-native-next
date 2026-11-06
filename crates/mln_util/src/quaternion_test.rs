// Not in the C++ suite (there is no test/util/quaternion.test.cpp): sanity
// checks for the quaternion port.

use std::f64::consts::FRAC_PI_2;

use super::*;

#[test]
fn default_identity_and_vec4_view() {
    assert_eq!(Quaternion::default(), Quaternion::new(0.0, 0.0, 0.0, 0.0));
    assert_eq!(Quaternion::IDENTITY.m(), [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(Quaternion::from_vec4(&[1.0, 2.0, 3.0, 4.0]), Quaternion::new(1.0, 2.0, 3.0, 4.0));
    assert_ne!(Quaternion::IDENTITY, Quaternion::default());
    assert_eq!(Quaternion::IDENTITY.to_rotation_matrix(), crate::mat4::matrix::identity4());
}

#[test]
fn conjugate_length_normalized() {
    let q = Quaternion::new(1.0, 2.0, 2.0, 4.0);
    assert_eq!(q.conjugate(), Quaternion::new(-1.0, -2.0, -2.0, 4.0));
    assert_eq!(q.length(), 5.0);
    assert_eq!(q.normalized(), Quaternion::new(0.2, 0.4, 0.4, 0.8));
    assert_eq!(q.multiply(&Quaternion::IDENTITY), q);
}

#[test]
fn axis_angle_rotates_vector() {
    let q = Quaternion::from_axis_angle(&[0.0, 0.0, 1.0], FRAC_PI_2);
    let v = q.transform(&[1.0, 0.0, 0.0]);
    assert!(v[0].abs() < 1e-15);
    assert!((v[1] - 1.0).abs() < 1e-15);
    assert_eq!(v[2], 0.0);

    let e = Quaternion::from_euler_angles(0.0, 0.0, FRAC_PI_2);
    assert_eq!(e, q);
}
