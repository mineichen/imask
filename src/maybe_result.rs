use crate::ImageDimension;

/// Unifies fallible and infallible sources: turns a value into a
/// `Result<Self::Ok, E>`, where infallible values are treated as [`Ok`].
///
/// The trait itself does not restrict [`MaybeResult::Ok`]; consumers add the
/// bounds they need. For example,
/// [`UnionAll::new_result`](crate::UnionAll::new_result) requires
/// `Ok: IntoIterator<Item = Span<T>>`, which is what allows
/// [`ImaskSet::union_all`](crate::ImaskSet::union_all) to operate on an outer
/// `Iterator` whose items are any of:
/// - a fallible `Result<_, E>` where `E: From<PipelineError>`,
/// - any infallible [`Iterator`] that implements [`ImageDimension`]
///   (treated as `Ok`, can never fail),
///
pub trait MaybeResult {
    type Ok;
    type Err;
    fn into_result(self) -> Result<Self::Ok, Self::Err>;
}

impl<TOk, TErr> MaybeResult for Result<TOk, TErr> {
    type Ok = TOk;
    type Err = TErr;
    fn into_result(self) -> Result<TOk, TErr> {
        self
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
