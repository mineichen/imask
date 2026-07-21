use std::fmt::{self, Debug, Display, Formatter, write};

/// Size hint for [`IterVisualizer`] where the total number of items is known
/// upfront (e.g. from `Vec::len`). The stored value is the total count; the
/// "...and X more" message will display `total - N`.
#[derive(Clone, Copy)]
pub(crate) struct Known(pub usize);

/// Size hint for [`IterVisualizer`] where the total number of items is unknown.
/// The remaining count is determined by calling `.count()` on the rest of the
/// iterator after `N` items have been printed.
#[derive(Clone, Copy)]
struct Unknown;

trait IterSize: Copy + Sized {
    fn rest<I: Iterator>(&self, iter: I) -> usize;
}

impl IterSize for Known {
    fn rest<I: Iterator>(&self, _iter: I) -> usize {
        self.0
    }
}
impl IterSize for Unknown {
    fn rest<I: Iterator>(&self, iter: I) -> usize {
        iter.count()
    }
}

pub(crate) struct IterVisualizer<T, S, const N: usize> {
    iter: T,
    rest: S,
}

#[allow(unused)]
impl<T, const N: usize> IterVisualizer<T, Unknown, N> {
    pub fn new(iter: T) -> Self {
        Self {
            iter,
            rest: Unknown,
        }
    }
}

impl<T, const N: usize> IterVisualizer<T, Known, N> {
    pub fn new_with_size(iter: T, total: usize) -> Self {
        Self {
            iter,
            rest: Known(total.saturating_sub(N)),
        }
    }
}

fn format_iter<T, F, const N: usize>(
    iter: &mut T,
    f: &mut Formatter<'_>,
    remaining: F,
    writer: impl Fn(&mut Formatter<'_>, T::Item) -> fmt::Result,
) -> fmt::Result
where
    T: Iterator,
    F: IterSize,
{
    f.write_str("[")?;
    let mut written = 0usize;
    while written < N {
        match iter.next() {
            Some(item) => {
                if written > 0 {
                    f.write_str(", ")?;
                }
                writer(f, item)?;
                written += 1;
            }
            None => break,
        }
    }
    if written == N {
        let count = remaining.rest(iter);
        if count > 0 {
            return write!(f, ", ...and {count} more]");
        }
    }
    f.write_str("]")
}

impl<T, const N: usize, S: IterSize> Display for IterVisualizer<T, S, N>
where
    T: Iterator + Clone,
    T::Item: Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut iter = self.iter.clone();
        format_iter::<_, _, N>(&mut iter, f, self.rest, |f, i| {
            write(f, format_args!("{i}"))
        })
    }
}

impl<T, const N: usize, S: IterSize> Debug for IterVisualizer<T, S, N>
where
    T: Iterator + Clone,
    T::Item: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut iter = self.iter.clone();
        format_iter::<_, _, N>(&mut iter, f, self.rest, |f, i| {
            write(f, format_args!("{i:?}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_with_more() {
        let s = format!(
            "{}",
            IterVisualizer::<_, Unknown, 2>::new(["a", "b", "c", "d"].into_iter()),
        );
        assert_eq!(s, r#"[a, b, ...and 2 more]"#);
    }

    #[test]
    fn unknown_exact() {
        let s = format!(
            "{:?}",
            IterVisualizer::<_, Unknown, 2>::new(["a", "b"].into_iter()),
        );
        assert_eq!(s, r#"["a", "b"]"#);
    }

    #[test]
    fn unknown_fewer_than_n() {
        let s = format!(
            "{:?}",
            IterVisualizer::<_, Unknown, 5>::new(["a", "b"].into_iter()),
        );
        assert_eq!(s, r#"["a", "b"]"#);
    }

    #[test]
    fn unknown_empty() {
        let s = format!(
            "{}",
            IterVisualizer::<_, Unknown, 3>::new(std::iter::empty::<&str>()),
        );
        assert_eq!(s, "[]");
    }

    #[test]
    fn known_with_more() {
        let s = format!(
            "{:?}",
            IterVisualizer::<_, Known, 2>::new_with_size(["a", "b", "c", "d"].into_iter(), 4),
        );
        assert_eq!(s, r#"["a", "b", ...and 2 more]"#);
    }

    #[test]
    fn known_exact() {
        let s = format!(
            "{:?}",
            IterVisualizer::<_, Known, 2>::new_with_size(["a", "b"].into_iter(), 2),
        );
        assert_eq!(s, r#"["a", "b"]"#);
    }

    #[test]
    fn known_fewer_items_than_n() {
        let s = format!(
            "{}",
            IterVisualizer::<_, Known, 5>::new_with_size(["a", "b"].into_iter(), 2),
        );
        assert_eq!(s, r#"[a, b]"#);
    }

    #[test]
    fn known_zero_more() {
        let s = format!(
            "{}",
            IterVisualizer::<_, Known, 3>::new_with_size(["a", "b", "c"].into_iter(), 3),
        );
        assert_eq!(s, r#"[a, b, c]"#);
    }

    #[test]
    fn non_string_items_use_debug_format() {
        let s = format!("{}", IterVisualizer::<_, Unknown, 2>::new(0u32..10),);
        assert_eq!(s, r#"[0, 1, ...and 8 more]"#);
    }

    #[test]
    fn display_is_repeatable() {
        let v = IterVisualizer::<_, Unknown, 2>::new(["a", "b", "c", "d"].into_iter());
        assert_eq!(format!("{v}"), r#"[a, b, ...and 2 more]"#,);
        assert_eq!(format!("{v}"), r#"[a, b, ...and 2 more]"#,);
    }
}
