// Ports include/mln/style/expression/interpolator.hpp.

use mln_util::UnitBezier;
use mln_util::interpolate::interpolation_factor;

use crate::range::Range;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExponentialInterpolator {
    pub base: f64,
}

impl ExponentialInterpolator {
    pub fn new(base: f64) -> Self {
        ExponentialInterpolator { base }
    }

    pub fn interpolation_factor(&self, input_levels: Range<f64>, input: f64) -> f64 {
        interpolation_factor(
            self.base as f32,
            Range::new(input_levels.min as f32, input_levels.max as f32),
            input as f32,
        ) as f64
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicBezierInterpolator {
    pub ub: UnitBezier,
}

impl CubicBezierInterpolator {
    pub fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        CubicBezierInterpolator { ub: UnitBezier::new(x1, y1, x2, y2) }
    }

    pub fn interpolation_factor(&self, input_levels: Range<f64>, input: f64) -> f64 {
        self.ub.solve(
            interpolation_factor(1.0, Range::new(input_levels.min as f32, input_levels.max as f32), input as f32)
                as f64,
            1e-6,
        )
    }
}

/// `Interpolator`: `variant<ExponentialInterpolator, CubicBezierInterpolator>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Interpolator {
    Exponential(ExponentialInterpolator),
    CubicBezier(CubicBezierInterpolator),
}

impl Interpolator {
    pub fn interpolation_factor(&self, input_levels: Range<f64>, input: f64) -> f64 {
        match self {
            Interpolator::Exponential(i) => i.interpolation_factor(input_levels, input),
            Interpolator::CubicBezier(i) => i.interpolation_factor(input_levels, input),
        }
    }
}

impl From<ExponentialInterpolator> for Interpolator {
    fn from(i: ExponentialInterpolator) -> Self {
        Interpolator::Exponential(i)
    }
}

impl From<CubicBezierInterpolator> for Interpolator {
    fn from(i: CubicBezierInterpolator) -> Self {
        Interpolator::CubicBezier(i)
    }
}
