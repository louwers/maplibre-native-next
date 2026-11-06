// Ports test/util/position.test.cpp.

use mln_style_types::Position;

use crate::assert_near;

#[track_caller]
fn expect_array_eq(got: [f32; 3], expected: [f32; 3]) {
    for i in 0..3 {
        assert_near(got[i] as f64, expected[i] as f64, 0.00001);
    }
}

#[track_caller]
fn expect_array_ne(got: [f32; 3], expected: [f32; 3]) {
    let eq = (0..3).filter(|&i| got[i] == expected[i]).count();
    assert_ne!(eq, 3);
}

fn create_position(pos: [f32; 3]) -> Position {
    Position::new(pos)
}

// gtest: Position.Calculations
#[test]
fn calculations() {
    let spherical = [2.0, 10.0, 270.0];
    let mut position = Position::new(spherical);

    expect_array_ne(position.get_cartesian(), spherical);
    expect_array_eq(position.get_spherical(), spherical);
    #[allow(clippy::excessive_precision)]
    expect_array_eq(position.get_cartesian(), [0.34729638695716858, -1.9696154594421387, 2.384976127700611e-08]);

    expect_array_ne(
        create_position([2.0, 30.0, 10.0]).get_spherical(),
        create_position([2.0, 30.0, 370.0]).get_spherical(),
    );
    expect_array_eq(
        create_position([2.0, 30.0, 10.0]).get_cartesian(),
        create_position([2.0, 30.0, 370.0]).get_cartesian(),
    );

    let new_spherical = [1.0, 80.0, 270.0];
    position.set(new_spherical);
    expect_array_ne(position.get_spherical(), spherical);
    #[allow(clippy::excessive_precision)]
    expect_array_ne(position.get_cartesian(), [0.34729638695716858, -1.9696154594421387, 2.384976127700611e-08]);
    #[allow(clippy::excessive_precision)]
    expect_array_eq(position.get_cartesian(), [0.98480772972106934, -0.17364829778671265, 1.1924880638503055e-08]);
}
