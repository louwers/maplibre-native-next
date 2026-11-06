// Ports include/mln/style/conversion/transition_options.hpp +
// src/mln/style/conversion/transition_options.cpp.

use mln_util::chrono::Duration;

use super::{Converter, Convertible, Error};
use crate::transition_options::TransitionOptions;

impl Converter for TransitionOptions {
    fn convert(value: &Convertible<'_>, error: &mut Error) -> Option<TransitionOptions> {
        if !value.is_object() {
            error.message = "transition must be an object".to_owned();
            return None;
        }

        let mut result = TransitionOptions::default();

        if let Some(duration) = value.object_member("duration") {
            let Some(number) = duration.to_number() else {
                error.message = "duration must be a number".to_owned();
                return None;
            };
            // std::chrono::milliseconds(int64_t(*number)): float -> int64
            // truncates (`as` saturates where C++ is undefined).
            result.duration = Some(Duration::from_millis(number as i64));
        }

        if let Some(delay) = value.object_member("delay") {
            let Some(number) = delay.to_number() else {
                error.message = "delay must be a number".to_owned();
                return None;
            };
            result.delay = Some(Duration::from_millis(number as i64));
        }

        Some(result)
    }
}
