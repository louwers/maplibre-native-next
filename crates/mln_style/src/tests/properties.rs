// Ports test/style/properties.test.cpp.

use crate::map::zoom_history::ZoomHistory;
use crate::properties::Transitioning;
use crate::property_value::PropertyValue;
use crate::renderer::property_evaluation_parameters::PropertyEvaluationParameters;
use crate::expression::dsl;
use crate::property_expression::PropertyExpression;
use crate::renderer::data_driven_property_evaluator::DataDrivenPropertyEvaluator;
use crate::renderer::possibly_evaluated_property_value::PossiblyEvaluatedPropertyValue;
use crate::renderer::property_evaluator::PropertyEvaluator;
use crate::transition_options::TransitionOptions;
use mln_util::chrono::{Duration, TimePoint, milliseconds};

fn evaluate(property: &Transitioning<PropertyValue<f32>>, delta: Duration) -> f32 {
    let mut zoom_history = ZoomHistory::default();
    zoom_history.update(0.0, &(TimePoint::min() + delta));

    let parameters = PropertyEvaluationParameters::with_zoom_history(
        zoom_history,
        TimePoint::min() + delta,
        Duration::zero(),
    );

    let evaluator = PropertyEvaluator::new(&parameters, 0.0_f32);

    property.evaluate(&evaluator, parameters.now)
}

/// gtest `ASSERT_FLOAT_EQ`: within 4 ULPs.
fn assert_float_eq(expected: f32, actual: f32) {
    let diff = (expected.to_bits() as i64 - actual.to_bits() as i64).abs();
    assert!(diff <= 4, "expected {expected}, got {actual}");
}

// gtest: TransitioningPropertyValue.EvaluateDefaultValue
#[test]
fn evaluate_default_value() {
    let property = Transitioning::<PropertyValue<f32>>::default();
    assert_eq!(0.0_f32, evaluate(&property, Duration::zero()));
}

// gtest: TransitioningPropertyValue.EvaluateUntransitionedConstant
#[test]
fn evaluate_untransitioned_constant() {
    let property = Transitioning::with_prior(
        PropertyValue::Constant(1.0_f32),
        Transitioning::default(),
        &TransitionOptions::default(),
        TimePoint::min(),
    );

    assert_eq!(1.0_f32, evaluate(&property, Duration::zero()));
}

// gtest: TransitioningPropertyValue.EvaluateTransitionedConstantWithoutDelay
#[test]
fn evaluate_transitioned_constant_without_delay() {
    let mut transition = TransitionOptions::default();
    transition.duration = Some(milliseconds(1000));

    let t0 = Transitioning::with_prior(
        PropertyValue::Constant(0.0_f32),
        Transitioning::default(),
        &TransitionOptions::default(),
        TimePoint::min(),
    );

    let t1 = Transitioning::with_prior(PropertyValue::Constant(1.0_f32), t0, &transition, TimePoint::min());

    assert_float_eq(0.0, evaluate(&t1, milliseconds(0)));
    assert_float_eq(0.823099, evaluate(&t1, milliseconds(500)));
    assert_float_eq(1.0, evaluate(&t1, milliseconds(1500)));
}

// gtest: TransitioningPropertyValue.EvaluateTransitionedConstantWithDelay
#[test]
fn evaluate_transitioned_constant_with_delay() {
    let mut transition = TransitionOptions::default();
    transition.delay = Some(milliseconds(1000));
    transition.duration = Some(milliseconds(1000));

    let t0 = Transitioning::with_prior(
        PropertyValue::Constant(0.0_f32),
        Transitioning::default(),
        &TransitionOptions::default(),
        TimePoint::min(),
    );

    let t1 = Transitioning::with_prior(PropertyValue::Constant(1.0_f32), t0, &transition, TimePoint::min());

    assert_float_eq(0.0, evaluate(&t1, milliseconds(0)));
    assert_float_eq(0.0, evaluate(&t1, milliseconds(500)));
    assert_float_eq(0.0, evaluate(&t1, milliseconds(612)));
    assert_float_eq(0.823099, evaluate(&t1, milliseconds(1500)));
    assert_float_eq(1.0, evaluate(&t1, milliseconds(2500)));
}

fn evaluate_data_expression(
    property: &Transitioning<PropertyValue<f32>>,
    delta: Duration,
) -> PossiblyEvaluatedPropertyValue<f32> {
    let mut zoom_history = ZoomHistory::default();
    zoom_history.update(0.0, &(TimePoint::min() + delta));

    let parameters = PropertyEvaluationParameters::with_zoom_history(
        zoom_history,
        TimePoint::min() + delta,
        Duration::zero(),
    );

    let evaluator = DataDrivenPropertyEvaluator::<f32>::new(&parameters, 0.0_f32);

    property.evaluate(&evaluator, parameters.now)
}

// gtest: TransitioningDataDrivenPropertyValue.Evaluate
#[test]
fn transitioning_data_driven_property_value_evaluate() {
    let mut transition = TransitionOptions::default();
    transition.delay = Some(milliseconds(1000));
    transition.duration = Some(milliseconds(1000));

    let t0 = Transitioning::with_prior(
        PropertyValue::Constant(0.0_f32),
        Transitioning::default(),
        &TransitionOptions::default(),
        TimePoint::min(),
    );

    let expression = PropertyExpression::<f32>::from_box(dsl::number(dsl::get_str("property_name"), None), None);

    let t1 = Transitioning::with_prior(PropertyValue::Expression(expression), t0.clone(), &transition, TimePoint::min());

    assert!(evaluate_data_expression(&t0, milliseconds(0)).is_constant());
    assert!(
        !evaluate_data_expression(&t1, milliseconds(0)).is_constant(),
        "A paint property transition to a data-driven evaluates immediately to the final value (see \
         https://github.com/mapbox/mapbox-gl-native/issues/8237)."
    );
}

#[test]
fn transition_completes_and_drops_prior() {
    // Not a C++ test: checks the `mutable prior` reset that hasTransition()
    // observes (properties.hpp).
    let mut transition = TransitionOptions::default();
    transition.duration = Some(milliseconds(1000));
    let t0 = Transitioning::new(PropertyValue::Constant(0.0_f32));
    let t1 = Transitioning::with_prior(PropertyValue::Constant(1.0_f32), t0, &transition, TimePoint::min());
    assert!(t1.has_transition());
    assert_float_eq(0.823099, evaluate(&t1, milliseconds(500)));
    assert!(t1.has_transition());
    assert_eq!(1.0, evaluate(&t1, milliseconds(1000)));
    assert!(!t1.has_transition());
}

/// `PossiblyEvaluated::evaluate<P>(z, feature, ...)` (properties.hpp), as the
/// circle layer and the line bucket call it.
#[test]
fn possibly_evaluated_evaluate_per_feature() {
    use crate::layers::circle_layer_properties::{CirclePitchAlignment, CircleRadius};
    use crate::layers::line_layer_properties::LineJoin;
    use crate::layers::{CirclePaintProperties, LineLayoutProperties};
    use crate::property_expression::PropertyExpression;
    use crate::renderer::possibly_evaluated_property_value::PossiblyEvaluatedPropertyValue;
    use crate::tests::util::{StubGeometryTileFeature, property_map};
    use mln_expression::dsl::{get_str, number};
    use mln_style_types::{AlignmentType, LineJoinType};
    use mln_tile_id::CanonicalTileID;

    let feature = StubGeometryTileFeature::new(property_map(&[("r", feature::Value::Double(7.5))]));

    let mut paint = CirclePaintProperties::PossiblyEvaluated::default();
    // A constant.
    paint.circle_radius = PossiblyEvaluatedPropertyValue::Constant(3.0);
    assert_eq!(3.0, paint.evaluate::<CircleRadius>(10.0, &feature));
    // A feature expression, with the property's default for a missing value.
    paint.circle_radius =
        PossiblyEvaluatedPropertyValue::Expression(PropertyExpression::from_box(number(get_str("r"), None), None));
    assert_eq!(7.5, paint.evaluate::<CircleRadius>(10.0, &feature));
    let other = StubGeometryTileFeature::new(property_map(&[]));
    assert_eq!(5.0, paint.evaluate::<CircleRadius>(10.0, &other));
    // A plain (non-data-driven) property.
    paint.circle_pitch_alignment = AlignmentType::Map;
    assert_eq!(AlignmentType::Map, paint.evaluate::<CirclePitchAlignment>(10.0, &feature));

    let layout = LineLayoutProperties::PossiblyEvaluated::default();
    assert_eq!(
        LineJoinType::Miter,
        layout.evaluate_canonical::<LineJoin>(10.0, &feature, &CanonicalTileID::new(0, 0, 0))
    );
}
