// Ports src/mln/util/tile_range.hpp.

use geometry::Point;
use mln_tile_id::CanonicalTileID;
use mln_util::constants;
use mln_util::geo::LatLngBounds;
use mln_util::projection::Projection;
use mln_util::range::Range;

/// `util::TileRange`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileRange {
    pub range: Range<Point<u32>>,
    pub zoom_range: Range<u8>,
}

/// `util::clamp(value, min, max)`: `std::max(min, std::min(max, value))`.
fn clamp(value: f64, min: f64, max: f64) -> f64 {
    let lower = if value < max { value } else { max };
    if min < lower { lower } else { min }
}

impl TileRange {
    /// Compute the range of tiles covered by the bounds at maxZoom.
    pub fn from_lat_lng_bounds_range(bounds: &LatLngBounds, mut min_zoom: u8, mut max_zoom: u8) -> TileRange {
        if min_zoom > max_zoom {
            std::mem::swap(&mut min_zoom, &mut max_zoom);
        }

        let sw_proj = Projection::project_zoom(&bounds.southwest().wrapped(), i32::from(max_zoom));
        let ne = bounds.northeast();
        let ne_proj = Projection::project_zoom(
            &if ne.longitude() > constants::LONGITUDE_MAX { ne.wrapped() } else { ne },
            i32::from(max_zoom),
        );

        let max_tile = 2.0f64.powf(f64::from(max_zoom));
        let min_x = sw_proj.x.floor() as u32;
        let max_x = ne_proj.x.floor() as u32;
        let min_y = clamp(ne_proj.y.floor(), 0.0, max_tile) as u32;
        let max_y = clamp(sw_proj.y.floor(), 0.0, max_tile) as u32;

        TileRange {
            range: Range::new(Point::new(min_x, min_y), Point::new(max_x, max_y)),
            zoom_range: Range::new(min_zoom, max_zoom),
        }
    }

    /// Compute the range of tiles covered by the bounds.
    pub fn from_lat_lng_bounds(bounds: &LatLngBounds, z: u8) -> TileRange {
        Self::from_lat_lng_bounds_range(bounds, z, z)
    }

    pub fn contains(&self, tile_id: &CanonicalTileID) -> bool {
        if tile_id.z <= self.zoom_range.max && tile_id.z >= self.zoom_range.min {
            if tile_id.z == 0 {
                return true;
            }
            let dz = self.zoom_range.max - tile_id.z;
            let x0 = self.range.min.x.wrapping_shr(u32::from(dz));
            let x1 = self.range.max.x.wrapping_shr(u32::from(dz));
            let y0 = self.range.min.y.wrapping_shr(u32::from(dz));
            let y1 = self.range.max.y.wrapping_shr(u32::from(dz));
            return (if self.range.min.x > self.range.max.x {
                // For wrapped bounds
                tile_id.x >= x0 || tile_id.x <= x1
            } else {
                tile_id.x <= x1 && tile_id.x >= x0
            }) && tile_id.y <= y1
                && tile_id.y >= y0;
        }
        false
    }
}
