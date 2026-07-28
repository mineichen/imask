use std::fmt::Debug;

/// Makes sure, that the underlying iterator is sorted by the given function,
/// without causing performance overhead in release builds
/// ```
/// use imask::DebugAssertSortedByIter;
///
/// let mut iter = DebugAssertSortedByIter::new(vec!["a", "aa", "aaa"], |x: &&str| x.len());
/// assert_eq!(iter.next(), Some("a"));
/// assert_eq!(iter.next(), Some("aa"));
/// assert_eq!(iter.next(), Some("aaa"));
/// assert_eq!(iter.next(), None);
/// ```
pub struct DebugAssertSortedByIter<T, TFn, TOrd>(T, Option<TOrd>, TFn);

impl<TIter, TFn, TOrd> DebugAssertSortedByIter<TIter, TFn, TOrd> {
    pub fn new(iter: impl IntoIterator<IntoIter = TIter>, func: TFn) -> Self {
        Self(iter.into_iter(), None, func)
    }
}

impl<T: Iterator, TFn: Fn(&T::Item) -> TOrd, TOrd: Ord + Eq + Debug> Iterator
    for DebugAssertSortedByIter<T, TFn, TOrd>
{
    type Item = T::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.0.next()?;
        #[cfg(debug_assertions)]
        {
            let ord_value = (self.2)(&value);
            if let Some(ord_last) = self.1.take() {
                assert!(ord_value >= ord_last, "{:?}>={:?}", ord_value, ord_last);
            }
            self.1 = Some(ord_value);
        }

        Some(value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}
