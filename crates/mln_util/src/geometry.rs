// Ports include/mln/util/geometry.hpp: `FeatureType`, `ToFeatureType` and
// `convertPoint`. The geometry type aliases are the
// `geometry` crate's types (`Point`, `LineString`, ...), re-exported
// here under their mln names.

pub use geometry::{
    Geometry,
    GeometryCollection,
    LineString,
    LinearRing,
    MultiLineString,
    MultiPoint,
    MultiPolygon,
    Point,
    Polygon,
};

/// `mln::FeatureType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum FeatureType {
    #[default]
    Unknown = 0,
    Point = 1,
    LineString = 2,
    Polygon = 3,
}

/// `mln::ToFeatureType` applied to a geometry.
pub fn to_feature_type<T>(geometry: &Geometry<T>) -> FeatureType {
    match geometry {
        Geometry::Empty => FeatureType::Unknown,
        Geometry::Point(_) | Geometry::MultiPoint(_) => FeatureType::Point,
        Geometry::LineString(_) | Geometry::MultiLineString(_) => FeatureType::LineString,
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) => FeatureType::Polygon,
        Geometry::GeometryCollection(_) => FeatureType::Unknown,
    }
}

// TODO(port): convertPoint and forEachPoint, with their first users.
