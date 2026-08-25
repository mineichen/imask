use std::convert::Infallible;

use crate::{ImageDimension, PipelineError};

/// Unifies fallible and infallible sources: turns a value into a
/// `Result<Self::Ok, E>`, where infallible values are treated as [`Ok`].
///
/// The trait itself does not restrict [`MaybeResult::Ok`]; consumers add the
/// bounds they need. For example,
/// [`UnionAll::new`](crate::span::UnionAll::new) requires
/// `Ok: IntoIterator<Item = Span<T>>`, which is what allows
/// [`ImaskSet::union_all`](crate::ImaskSet::union_all) to operate on an outer
/// `Iterator` whose items are any of:
/// - a fallible `Result<_, E>` where `E: From<PipelineError>`
///   (see [`IntoOutput`]),
/// - any infallible [`Iterator`] that implements [`ImageDimension`]
///   (treated as `Ok`, can never fail),
///
pub trait MaybeResult {
    type Ok;
    type Err;
    fn into_result(self) -> Result<Self::Ok, Self::Err>;
}

/// Unifies the error type of a [`MaybeResult`] item ([`MaybeResult::Err`])
/// with [`PipelineError`] to determine the error type of the overall result.
///
/// - An item error type which can represent a [`PipelineError`] is kept
///   as-is, so no information is lost. Since [`From<PipelineError>`] alone
///   cannot tell whether an error merely signals that an input was empty
///   ([`PipelineError::Empty`]), such types implement [`IntoOutput`]
///   manually, mapping empty inputs to `None`:
///
/// ```
/// # use imask::{IntoPipelineOutput, PipelineError};
/// #[derive(Debug)]
/// enum PipelineErrorLeptos {
///     Pipeline(PipelineError),
///     ServerFn,
/// }
///
/// impl From<PipelineError> for PipelineErrorLeptos {
///     fn from(error: PipelineError) -> Self {
///         Self::Pipeline(error)
///     }
/// }
///
/// impl IntoPipelineOutput for PipelineErrorLeptos {
///     type Output = PipelineErrorLeptos;
///     fn into_output_if_not_empty(self) -> Option<Self::Output> {
///         match self {
///             // an empty input is skipped, not an error:
///             Self::Pipeline(PipelineError::Empty) => None,
///             error => Some(error),
///         }
///     }
/// }
/// ```
///
/// - [`Infallible`] item errors (infallible items) fall back to
///   [`PipelineError`], which the combinator can still produce on its own
///   (e.g. [`PipelineError::Empty`] when all inputs are empty).
///
/// This way [`UnionAll::new`](crate::span::UnionAll::new) returns a
/// `Result<_, PipelineError>` for an outer iterator of plain iterators or of
/// `Result<_, PipelineError>`, but a `Result<_, E>` for an outer iterator of
/// `Result<_, E>` where `E: From<PipelineError>`.
pub trait IntoPipelineOutput: Sized {
    /// The error type of the overall result.
    type Output: From<PipelineError>;

    /// Converts an item error into the error of the overall result.
    ///
    /// Returns `None` if the error merely signals that this input was empty
    /// ([`PipelineError::Empty`]): empty inputs are skipped instead of
    /// propagated (see [`UnionAll::new`](crate::span::UnionAll::new)).
    fn into_output_if_not_empty(self) -> Option<Self::Output>;
}

impl IntoPipelineOutput for PipelineError {
    type Output = PipelineError;
    fn into_output_if_not_empty(self) -> Option<Self::Output> {
        match self {
            PipelineError::Empty => None,
            error => Some(error),
        }
    }
}

impl IntoPipelineOutput for Infallible {
    type Output = PipelineError;
    fn into_output_if_not_empty(self) -> Option<Self::Output> {
        match self {}
    }
}

impl<TOk, TErr> MaybeResult for Result<TOk, TErr> {
    type Ok = TOk;
    type Err = TErr;
    fn into_result(self) -> Result<TOk, TErr> {
        self
    }
}

impl<'a, TOk, TErr> MaybeResult for &'a Result<TOk, TErr> {
    type Ok = &'a TOk;
    type Err = &'a TErr;

    fn into_result(self) -> Result<Self::Ok, Self::Err> {
        self.as_ref()
    }
}

impl<TOk> MaybeResult for TOk
where
    TOk: ImageDimension,
{
    type Ok = TOk;
    type Err = std::convert::Infallible;
    fn into_result(self) -> Result<TOk, Self::Err> {
        Ok(self)
    }
}
