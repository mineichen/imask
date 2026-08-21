use std::{
    convert::Infallible,
    num::{IntErrorKind, TryFromIntError},
};

/// Error returned by fallible span-iterator combinators during construction.
///
/// Span-iterator combinators that may fail while being built — because they need
/// at least one element to seed their state, or because the produced ranges do
/// not fit the chosen representation — return `Result<_, PipelineError>`.
///
/// Construction can return [`PipelineError::Empty`] if the iterator depends on a
/// pending value (e.g. the first span needed to seed a merge window) but none is
/// available. This way [`Iterator::next`] knows that a previous value always
/// exists once construction succeeded.
///
/// To tolerate an empty input while still propagating real errors, use
/// [`PipelineError::allow_empty`] together with [`Result::or_else`]:
///
/// ```ignore
/// use imask::PipelineError;
///
/// let opt: Option<MyIter> = MyIter::new(it)
///     .map(Some)
///     .or_else(PipelineError::allow_empty)?;
/// ```
#[derive(Debug, PartialEq, Eq, Clone, Copy, thiserror::Error)]
pub enum PipelineError {
    /// The iterator did not produce the element required to seed the combinator.
    #[error("iterator produced no elements to seed the combinator")]
    Empty,
    /// The chosen output type cannot represent the (size of the) ranges produced
    /// by the iterator.
    #[error(transparent)]
    IncompatibleSize(#[from] IncompatibleSizeError),
}

impl From<std::convert::Infallible> for PipelineError {
    fn from(_value: std::convert::Infallible) -> Self {
        unreachable!("Infallible cannot be constructed")
    }
}

impl PipelineError {
    /// Converts an [`PipelineError::Empty`] into `Ok(None)` while propagating an
    /// [`PipelineError::IncompatibleSize`] as `Err`.
    ///
    /// This is intended to be used with [`Result::or_else`] (after
    /// [`Result::map`]`(Some)`) so that callers which accept empty ranges can
    /// keep going, while real errors are still propagated:
    ///
    /// `combinator::new(it).map(Some).or_else(PipelineError::allow_empty)?`
    pub fn allow_empty<T>(self) -> Result<Option<T>, IncompatibleSizeError> {
        match self {
            PipelineError::Empty => Ok(None),
            PipelineError::IncompatibleSize(error) => Err(error),
        }
    }
}

pub trait PipelineResult<T> {
    fn allow_empty(self) -> Result<Option<T>, IncompatibleSizeError>;
}
impl<T, E: Into<PipelineError>> PipelineResult<T> for Result<T, E> {
    fn allow_empty(self) -> Result<Option<T>, IncompatibleSizeError> {
        match self.map_err(Into::into) {
            Ok(x) => Ok(Some(x)),
            Err(PipelineError::Empty) => Ok(None),
            Err(PipelineError::IncompatibleSize(x)) => Err(x),
        }
    }
}

impl From<TryFromIntError> for PipelineError {
    fn from(value: TryFromIntError) -> Self {
        IncompatibleSizeError::from(value).into()
    }
}

impl From<IntErrorKind> for PipelineError {
    fn from(value: IntErrorKind) -> Self {
        IncompatibleSizeError::from(value).into()
    }
}

/// The numeric type cannot represent all ranges within `ImageDimension`
///
/// This should be checked during the construction of the iterator-combinator, so Iterator::next() has no arithmetic overflows
/// Combinators which can never be empty may return `Result<_, IncompatibleSizeError>`
/// directly instead of the full [`PipelineError`].
#[derive(Debug, PartialEq, Eq, thiserror::Error, Clone, Copy)]
#[error("incompatible size: {kind}")]
pub struct IncompatibleSizeError {
    kind: IncompatibleSizeErrorKind,
}
#[derive(Debug, PartialEq, Eq, thiserror::Error, Clone, Copy)]
enum IncompatibleSizeErrorKind {
    #[error(transparent)]
    TryFromInt(#[from] TryFromIntError),
    #[error("{0:?}")]
    IntErrorKind(IntErrorKind),
}

impl From<IntErrorKind> for IncompatibleSizeError {
    fn from(value: IntErrorKind) -> Self {
        IncompatibleSizeErrorKind::IntErrorKind(value).into()
    }
}

impl From<IncompatibleSizeErrorKind> for IncompatibleSizeError {
    fn from(kind: IncompatibleSizeErrorKind) -> Self {
        Self { kind }
    }
}

impl From<TryFromIntError> for IncompatibleSizeError {
    fn from(value: TryFromIntError) -> Self {
        IncompatibleSizeErrorKind::TryFromInt(value).into()
        //Self(value.to_string())
    }
}

impl From<Infallible> for IncompatibleSizeError {
    fn from(_value: Infallible) -> Self {
        unreachable!("Invallible cannot be constructed")
    }
}
