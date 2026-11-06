// Ports vendor/wagyu/include/mapbox/geometry/wagyu/build_result.hpp.

use geometry::{LinearRing, MultiPolygon, Point, Polygon};

use crate::point::{NIL, RingPtr};
use crate::ring::RingManager;

/// The output coordinate type `T2` of `wagyu::execute`: `static_cast<T2>`
/// of an int32 coordinate.
pub trait OutputCoordinate: Copy {
    fn from_i32(v: i32) -> Self;
}

impl OutputCoordinate for i16 {
    fn from_i32(v: i32) -> Self {
        v as i16
    }
}

impl OutputCoordinate for i32 {
    fn from_i32(v: i32) -> Self {
        v
    }
}

impl OutputCoordinate for i64 {
    fn from_i32(v: i32) -> Self {
        i64::from(v)
    }
}

impl OutputCoordinate for f64 {
    fn from_i32(v: i32) -> Self {
        f64::from(v)
    }
}

/// `push_ring_to_polygon`.
fn push_ring_to_polygon<T2: OutputCoordinate>(
    poly: &mut Polygon<T2>,
    r: RingPtr,
    manager: &mut RingManager,
    reverse_output: bool,
) {
    let mut lr: LinearRing<T2> = LinearRing::with_capacity(manager.size(r) + 1);
    let first_pt = manager.rings[r].points;
    let mut pt_it = first_pt;
    loop {
        let (x, y) = manager.xy(pt_it);
        lr.push(Point::new(T2::from_i32(x), T2::from_i32(y)));
        pt_it = if reverse_output { manager.next(pt_it) } else { manager.prev(pt_it) };
        if pt_it == first_pt {
            break;
        }
    }
    let (x, y) = manager.xy(first_pt);
    lr.push(Point::new(T2::from_i32(x), T2::from_i32(y))); // close the ring
    poly.push(lr);
}

/// `build_result_polygons`.
fn build_result_polygons<T2: OutputCoordinate>(
    solution: &mut MultiPolygon<T2>,
    rings: &[RingPtr],
    manager: &mut RingManager,
    reverse_output: bool,
) {
    for &r in rings {
        if r == NIL {
            continue;
        }
        debug_assert!(manager.rings[r].points != NIL);
        solution.push(Polygon::new());
        let mut polygon = Polygon::new();
        push_ring_to_polygon(&mut polygon, r, manager, reverse_output);
        let children = manager.rings[r].children.clone();
        for &c in &children {
            if c == NIL {
                continue;
            }
            debug_assert!(manager.rings[c].points != NIL);
            push_ring_to_polygon(&mut polygon, c, manager, reverse_output);
        }
        *solution.last_mut().expect("just pushed") = polygon;
        for &c in &children {
            if c == NIL {
                continue;
            }
            if !manager.rings[c].children.is_empty() {
                let grandchildren = manager.rings[c].children.clone();
                build_result_polygons(solution, &grandchildren, manager, reverse_output);
            }
        }
    }
}

/// `build_result`.
pub(crate) fn build_result<T2: OutputCoordinate>(
    solution: &mut MultiPolygon<T2>,
    manager: &mut RingManager,
    reverse_output: bool,
) {
    let children = manager.children.clone();
    build_result_polygons(solution, &children, manager, reverse_output);
}
