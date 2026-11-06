// Ports test/style/conversion/source_options.test.cpp.

use mln_conversion::Convertible;
use mln_util::tileset::{RasterEncoding, VectorEncoding};

use super::util::parse_json;
use crate::conversion::tileset::SourceOptions;
use crate::conversion::{Converter, Error};

/// `convertJSON<SourceOptions>(json, error)`.
fn convert_json(json: &str, error: &mut Error) -> Option<SourceOptions> {
    let value = parse_json(json);
    <SourceOptions as Converter>::convert(&Convertible::from(&value), error)
}

// gtest: SourceOptions.Basic
#[test]
fn basic() {
    let mut error = Error::default();
    let converted = convert_json("{}", &mut error);
    assert!(converted.is_none());
}

// gtest: SourceOptions.ErrorHandling
#[test]
fn error_handling() {
    let mut error = Error::default();
    let converted = convert_json(
        r#"{
        "encoding": "this isn't a valid encoding"
    }"#,
        &mut error,
    );
    assert!(converted.is_none());
    assert_eq!(Some(0), error.message.find("invalid encoding"));
}

// gtest: SourceOptions.TerrariumEncodingParsed
#[test]
fn terrarium_encoding_parsed() {
    let mut error = Error::default();
    let converted = convert_json(r#"{"encoding": "terrarium"}"#, &mut error).unwrap();
    assert_eq!(converted.raster_encoding, Some(RasterEncoding::Terrarium));
    assert!(converted.vector_encoding.is_none());
}

// gtest: SourceOptions.MapboxEncodingParsed
#[test]
fn mapbox_encoding_parsed() {
    let mut error = Error::default();
    let converted = convert_json(r#"{"encoding": "mapbox"}"#, &mut error).unwrap();
    assert_eq!(converted.raster_encoding, Some(RasterEncoding::Mapbox));
    assert!(converted.vector_encoding.is_none());
}

// gtest: SourceOptions.MVTEncodingParsed
#[test]
fn mvt_encoding_parsed() {
    let mut error = Error::default();
    let converted = convert_json(r#"{"encoding": "mvt"}"#, &mut error).unwrap();
    assert_eq!(converted.vector_encoding, Some(VectorEncoding::Mapbox));
    assert!(converted.raster_encoding.is_none());
}

// gtest: SourceOptions.MLTEncodingParsed
#[test]
fn mlt_encoding_parsed() {
    let mut error = Error::default();
    let converted = convert_json(r#"{"encoding": "mlt"}"#, &mut error).unwrap();
    assert_eq!(converted.vector_encoding, Some(VectorEncoding::MLT));
    assert!(converted.raster_encoding.is_none());
}
