// Ports include/mln/style/expression/error.hpp.

use crate::expression::{
    Dependency, EvaluationContext, EvaluationError, EvaluationResult, Expression, ExpressionBase, Kind,
};
use crate::type_::Type;
use crate::value::Value;

pub struct Error {
    base: ExpressionBase,
    message: String,
}

impl Error {
    pub fn new(message: String) -> Self {
        Error { base: ExpressionBase::new(Kind::Error, Type::Error, Dependency::NONE), message }
    }
}

impl Expression for Error {
    fn base(&self) -> &ExpressionBase {
        &self.base
    }

    fn evaluate(&self, _: &EvaluationContext<'_>) -> EvaluationResult {
        Err(EvaluationError::new(self.message.clone()))
    }

    fn each_child<'a>(&'a self, _: &mut dyn FnMut(&'a dyn Expression)) {}

    fn equals(&self, e: &dyn Expression) -> bool {
        e.get_kind() == Kind::Error
    }

    fn possible_outputs(&self) -> Vec<Option<Value>> {
        Vec::new()
    }

    fn get_operator(&self) -> String {
        "error".to_owned()
    }
}
