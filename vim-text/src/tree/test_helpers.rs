/// Test helpers for the generic tree. Used across tree module tests.
#[cfg(test)]
pub(crate) mod test_items {
    use crate::tree::traits::*;

    /// Simple numeric item for testing. Each item is one "unit" containing a value.
    #[derive(Clone, Debug, PartialEq)]
    pub struct NumItem(pub u32);

    /// Summary for NumItem: tracks sum and count.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct NumSummary {
        pub sum: u32,
        pub count: u32,
    }

    impl Summary for NumSummary {
        fn compose(&mut self, other: &Self) {
            self.sum += other.sum;
            self.count += other.count;
        }

        fn base_len(&self) -> usize {
            self.count as usize
        }
    }

    impl InvertibleSummary for NumSummary {
        fn subtract(&mut self, other: &Self) {
            self.sum -= other.sum;
            self.count -= other.count;
        }
    }

    impl Item for NumItem {
        type Summary = NumSummary;
        const MIN_LEN: usize = 1;
        const MAX_LEN: usize = 1; // Each NumItem is atomic (cannot split)

        fn summary(&self) -> NumSummary {
            NumSummary {
                sum: self.0,
                count: 1,
            }
        }

        fn len(&self) -> usize {
            1
        }

        fn split_at(&mut self, _offset: usize) -> Self {
            panic!("NumItem is atomic — cannot split")
        }

        fn try_merge(&mut self, _other: &Self) -> bool {
            false // NumItems never merge
        }
    }

    /// Dimension: count of items (position by count).
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Count(pub u32);

    impl Dimension<NumSummary> for Count {
        fn from_summary(s: &NumSummary) -> Self {
            Count(s.count)
        }
        fn add_summary(&mut self, s: &NumSummary) {
            self.0 += s.count;
        }
    }

    /// Dimension: running sum.
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Sum(pub u32);

    impl Dimension<NumSummary> for Sum {
        fn from_summary(s: &NumSummary) -> Self {
            Sum(s.sum)
        }
        fn add_summary(&mut self, s: &NumSummary) {
            self.0 += s.sum;
        }
    }
}
