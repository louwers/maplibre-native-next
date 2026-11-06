// Ports test/style/conversion/stringify.test.cpp.

use std::collections::HashMap;
use std::sync::Arc;

use feature::Value;
use mln_style_types::{AnchorOffsetPair, SymbolAnchorType, VariableAnchorOffsetCollection, VisibilityType};
use mln_util::{Color, Padding};

use crate::conversion::stringify::{Stringify, Writer};
use crate::expression::Value as ExpressionValue;
use crate::expression::dsl::{eq, exponential, get_str, interpolate, linear, literal, literal_str, number, zoom};
use crate::filter::Filter;
use crate::layers::SymbolLayoutProperties;
use crate::layers::symbol_layer_properties::{IconPadding, SymbolAvoidEdges};
use crate::property_expression::PropertyExpression;
use crate::property_value::PropertyValue;

fn stringify<T: Stringify + ?Sized>(t: &T) -> String {
    let mut writer = Writer::new();
    t.stringify(&mut writer);
    writer.into_string()
}

fn num(v: f64) -> Box<dyn crate::expression::Expression> {
    literal(ExpressionValue::Number(v))
}

// gtest: Stringify.NullValue
#[test]
fn null_value() {
    assert_eq!(stringify(&()), "null");
}

// gtest: Stringify.Boolean
#[test]
fn boolean() {
    assert_eq!(stringify(&true), "true");
    assert_eq!(stringify(&false), "false");
}

// gtest: Stringify.Uint64
#[test]
fn uint64() {
    assert_eq!(stringify(&0_u64), "0");
    assert_eq!(stringify(&1_u64), "1");
}

// gtest: Stringify.Int64
#[test]
fn int64() {
    assert_eq!(stringify(&0_i64), "0");
    assert_eq!(stringify(&-1_i64), "-1");
}

// gtest: Stringify.Double
#[test]
fn double() {
    assert_eq!(stringify(&0.0_f64), "0.0");
    assert_eq!(stringify(&10.1234_f64), "10.1234");
}

// gtest: Stringify.String
#[test]
fn string() {
    assert_eq!(stringify(&"test".to_owned()), "\"test\"");
}

// gtest: Stringify.Enum
#[test]
fn enum_() {
    assert_eq!(stringify(&VisibilityType::Visible), "\"visible\"");
}

// gtest: Stringify.Color
#[test]
fn color() {
    assert_eq!(stringify(&Color::blue()), "\"rgba(0,0,255,1)\"");
}

// gtest: Stringify.VariableAnchorOffsetCollection
#[test]
fn variable_anchor_offset_collection() {
    let test = vec![AnchorOffsetPair::new(SymbolAnchorType::Left, [1.0, 1.0])];
    assert_eq!(stringify(&VariableAnchorOffsetCollection::new(test)), "[\"left\",[1.0,1.0]]");
}

// gtest: Stringify.Array
#[test]
fn array() {
    assert_eq!(stringify(&[1.0_f32, 2.0]), "[1.0,2.0]");
}

// gtest: Stringify.Vector
#[test]
fn vector() {
    assert_eq!(stringify(&vec![1.0_f32, 2.0]), "[1.0,2.0]");
}

// gtest: Stringify.Map
#[test]
fn map() {
    let map: HashMap<String, f32> = [("a".to_owned(), 1.0_f32)].into_iter().collect();
    assert_eq!(stringify(&map), "{\"a\":1.0}");
}

// gtest: Stringify.Value
#[test]
fn value() {
    assert_eq!(stringify(&Value::Bool(true)), "true");
    assert_eq!(stringify(&Value::Uint(0)), "0");
    assert_eq!(stringify(&Value::Double(1.2)), "1.2");
}

// gtest: Stringify.Filter
#[test]
fn filter() {
    assert_eq!(stringify(&Filter::default()), "null");
    let filter = Filter::new(Some(Arc::from(eq(literal_str("a"), literal_str("b")))), None);
    assert_eq!(stringify(&filter), "[\"==\",\"a\",\"b\"]");
}

fn exponential_property() -> Box<dyn crate::expression::Expression> {
    interpolate(exponential(2.0), number(get_str("property"), None), vec![(0.0, num(1.0)), (1.0, num(2.0))])
}

// gtest: Stringify.PropertyExpression
#[test]
fn property_expression() {
    assert_eq!(
        stringify(&PropertyExpression::<f32>::from_box(
            interpolate(linear(), zoom(), vec![(0.0, num(1.0)), (1.0, num(2.0))]),
            None
        )),
        "[\"interpolate\",[\"linear\"],[\"zoom\"],0.0,1.0,1.0,2.0]"
    );

    assert_eq!(
        stringify(&PropertyExpression::<f32>::from_box(exponential_property(), None)),
        "[\"interpolate\",[\"exponential\",2.0],[\"number\",[\"get\",\"property\"]],0.0,1.0,1.0,2.0]"
    );

    assert_eq!(
        stringify(&PropertyExpression::<f32>::from_box(
            interpolate(linear(), zoom(), vec![(0.0, exponential_property()), (1.0, exponential_property())]),
            None
        )),
        concat!(
            "[\"interpolate\",",
            "[\"linear\"],",
            "[\"zoom\"],",
            "0.0,[\"interpolate\",[\"exponential\",2.0],[\"number\",[\"get\",",
            "\"property\"]],0.0,1.0,1.0,2.0],",
            "1.0,[\"interpolate\",[\"exponential\",2.0],[\"number\",[\"get\",",
            "\"property\"]],0.0,1.0,1.0,2.0]]"
        )
    );
}

// gtest: Stringify.PropertyValue
#[test]
fn property_value() {
    assert_eq!(stringify(&PropertyValue::Constant(1.0_f32)), "1.0");
    assert_eq!(
        stringify(&PropertyValue::Expression(PropertyExpression::<f32>::from_box(
            interpolate(exponential(2.0), zoom(), vec![(0.0, num(1.0)), (1.0, num(2.0))]),
            None
        ))),
        "[\"interpolate\",[\"exponential\",2.0],[\"zoom\"],0.0,1.0,1.0,2.0]"
    );
}

// gtest: Stringify.Layout
#[test]
fn layout() {
    let stringify = |layout: &SymbolLayoutProperties::Unevaluated| {
        let mut writer = Writer::new();
        layout.stringify(&mut writer);
        writer.into_string()
    };

    assert_eq!(stringify(&SymbolLayoutProperties::Unevaluated::default()), "{}");

    let mut layout = SymbolLayoutProperties::Unevaluated::default();
    *layout.get_mut::<SymbolAvoidEdges>() = PropertyValue::Constant(true);
    *layout.get_mut::<IconPadding>() = PropertyValue::Constant(Padding::uniform(2.0));
    assert_eq!(stringify(&layout), "{\"icon-padding\":[2.0,2.0,2.0,2.0],\"symbol-avoid-edges\":true}");
}

// gtest: Stringify.Padding
#[test]
fn padding() {
    assert_eq!("[3.5,7.0,9.0,11.0]", stringify(&Padding::new(3.5, 7.0, 9.0, 11.0)));
}
