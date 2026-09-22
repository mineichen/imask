/// Fused 1-lookahead: `pending` is fetched eagerly in `new`, so
/// `pending: None` unambiguously means exhausted — no extra flag.
/// `peek`/`next` never poll `parent` after it returned `None`.
#[derive(Clone)]
pub(crate) struct Peekable<I: Iterator> {
    pub parent: I,
    pub pending: Option<I::Item>,
}

impl<I: Iterator> Peekable<I> {
    #[inline]
    pub(crate) fn new(mut parent: I) -> Self {
        let pending = parent.next();
        Self { parent, pending }
    }

    #[inline]
    pub(crate) fn next(&mut self) -> Option<I::Item> {
        let current = self.pending.take()?;
        self.pending = self.parent.next();
        Some(current)
    }

    #[inline]
    pub(crate) fn peek(&self) -> Option<&I::Item> {
        self.pending.as_ref()
    }

    #[inline]
    pub(crate) fn size_hint_total(&self) -> (usize, Option<usize>) {
        let (lo, hi) = self.parent.size_hint();
        let extra = usize::from(self.pending.is_some());
        (
            lo.saturating_add(extra),
            hi.map(|h| h.saturating_add(extra)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Panics if polled after returning `None` (i.e. not fused).
    struct Unfused<I> {
        inner: I,
        done: bool,
    }

    impl<I: Iterator> Iterator for Unfused<I> {
        type Item = I::Item;
        #[inline]
        fn next(&mut self) -> Option<I::Item> {
            assert!(!self.done, "polled after None");
            let item = self.inner.next();
            self.done = item.is_none();
            item
        }
    }

    #[test]
    fn never_polls_parent_after_none() {
        let mut p = Peekable::new(Unfused {
            inner: vec![1, 2].into_iter(),
            done: false,
        });
        assert_eq!(p.peek(), Some(&1));
        assert_eq!(p.next(), Some(1));
        assert_eq!(p.next(), Some(2));
        assert_eq!(p.next(), None);
        assert_eq!(p.next(), None);
        assert_eq!(p.peek(), None);
    }
}
