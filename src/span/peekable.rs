#[derive(Clone)]
pub(crate) struct Peekable<I: Iterator> {
    pub parent: I,
    pub pending: Option<I::Item>,
}

impl<I: Iterator> Peekable<I> {
    pub(crate) fn pending_or_fetch(&mut self) -> Option<I::Item> {
        self.pending.take().or_else(|| self.parent.next())
    }

    pub(crate) fn next(&mut self) -> Option<I::Item> {
        let mut pending = self.parent.next();

        #[cfg(debug_assertions)]
        {
            if pending.is_some() {
                assert!(
                    self.pending.is_some(),
                    "Expects, that peek() is called before"
                );
            }
        }
        std::mem::swap(&mut pending, &mut self.pending);
        pending
    }
    pub(crate) fn peek(&mut self) -> Option<&I::Item> {
        match &mut self.pending {
            Some(x) => Some(x),
            r => {
                *r = self.parent.next();
                r.as_ref()
            }
        }
    }

    pub(crate) fn size_hint_total(&self) -> (usize, Option<usize>) {
        let (lo, hi) = self.parent.size_hint();
        let extra = self.pending.is_some() as usize;
        (lo.saturating_add(extra), hi.map(|h| h.saturating_add(extra)))
    }
}
