// Ports include/mln/style/expression/distance.hpp and
// src/mln/style/expression/distance.cpp.
//
// TODO(port): the "distance" expression (GeoJSON parsing, cheap-ruler
// distances to tile geometry) arrives with GeoJSON sources (M3). Until then
// it fails to parse with an explicit error.

use mln_conversion::Convertible;

use crate::parsing_context::{ParseResult, ParsingContext};

pub struct Distance;

impl Distance {
    pub fn parse(_value: &Convertible<'_>, ctx: &mut ParsingContext) -> ParseResult {
        ctx.error("TODO(port): the 'distance' expression is not ported yet.".to_owned());
        None
    }
}
