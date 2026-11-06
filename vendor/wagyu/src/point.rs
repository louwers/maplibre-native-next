// Ports vendor/wagyu/include/mapbox/geometry/wagyu/point.hpp.
//
// The C++ points are nodes of circular doubly linked lists (`point_ptr` =
// raw pointer, nullptr after removal). Here they live in the ring
// manager's arena and are referenced by index; NIL is nullptr, and
// following NIL panics where C++ would dereference nullptr.

/// `mapbox::geometry::point<int32_t>`.
pub(crate) type GPoint = geometry::Point<i32>;

/// nullptr for `point_ptr`, `ring_ptr` and `bound_ptr`.
pub(crate) const NIL: usize = usize::MAX;

pub(crate) type PointPtr = usize;
pub(crate) type RingPtr = usize;

/// `mapbox::geometry::wagyu::point<int32_t>`.
#[derive(Clone, Debug)]
pub(crate) struct Point {
    pub ring: RingPtr,
    pub x: i32,
    pub y: i32,
    pub next: PointPtr,
    pub prev: PointPtr,
}
