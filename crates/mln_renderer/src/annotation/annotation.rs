// Ports include/mln/annotation/annotation.hpp.

use maplibre_native_base::{LineString, MultiLineString, MultiPolygon, Point, Polygon};
use mln_style::PropertyValue;
use mln_util::Color;

pub type AnnotationID = u64;
pub type AnnotationIDs = Vec<AnnotationID>;

/// `SymbolAnnotation`.
#[derive(Clone, Debug, PartialEq)]
pub struct SymbolAnnotation {
    pub geometry: Point<f64>,
    pub icon: String,
}

impl SymbolAnnotation {
    /// `SymbolAnnotation(Point<double> geometry_, std::string icon_ = {})`.
    pub fn new(geometry: Point<f64>, icon: impl Into<String>) -> Self {
        SymbolAnnotation { geometry, icon: icon.into() }
    }
}

/// `ShapeAnnotationGeometry = variant<LineString<double>, Polygon<double>,
/// MultiLineString<double>, MultiPolygon<double>>`.
#[derive(Clone, Debug, PartialEq)]
pub enum ShapeAnnotationGeometry {
    LineString(LineString<f64>),
    Polygon(Polygon<f64>),
    MultiLineString(MultiLineString<f64>),
    MultiPolygon(MultiPolygon<f64>),
}

/// `LineAnnotation`.
#[derive(Clone, Debug)]
pub struct LineAnnotation {
    pub geometry: ShapeAnnotationGeometry,
    pub opacity: PropertyValue<f32>,
    pub width: PropertyValue<f32>,
    pub color: PropertyValue<Color>,
}

impl LineAnnotation {
    /// `LineAnnotation(geometry, opacity = 1.0f, width = 1.0f, color =
    /// Color::black())`.
    pub fn new(geometry: ShapeAnnotationGeometry) -> Self {
        LineAnnotation {
            geometry,
            opacity: PropertyValue::Constant(1.0),
            width: PropertyValue::Constant(1.0),
            color: PropertyValue::Constant(Color::black()),
        }
    }
}

/// `FillAnnotation`.
#[derive(Clone, Debug)]
pub struct FillAnnotation {
    pub geometry: ShapeAnnotationGeometry,
    pub opacity: PropertyValue<f32>,
    pub color: PropertyValue<Color>,
    pub outline_color: PropertyValue<Color>,
}

impl FillAnnotation {
    /// `FillAnnotation(geometry, opacity = 1.0f, color = Color::black(),
    /// outlineColor = {})`.
    pub fn new(geometry: ShapeAnnotationGeometry) -> Self {
        FillAnnotation {
            geometry,
            opacity: PropertyValue::Constant(1.0),
            color: PropertyValue::Constant(Color::black()),
            outline_color: PropertyValue::Undefined,
        }
    }
}

/// `Annotation = variant<SymbolAnnotation, LineAnnotation, FillAnnotation>`.
#[derive(Clone, Debug)]
pub enum Annotation {
    Symbol(SymbolAnnotation),
    Line(LineAnnotation),
    Fill(FillAnnotation),
}

impl From<SymbolAnnotation> for Annotation {
    fn from(annotation: SymbolAnnotation) -> Self {
        Annotation::Symbol(annotation)
    }
}

impl From<LineAnnotation> for Annotation {
    fn from(annotation: LineAnnotation) -> Self {
        Annotation::Line(annotation)
    }
}

impl From<FillAnnotation> for Annotation {
    fn from(annotation: FillAnnotation) -> Self {
        Annotation::Fill(annotation)
    }
}
