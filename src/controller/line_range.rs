#[allow(unused_imports)]
use zwrite::{write, writeln};

use std::cmp::Ordering;
use std::error::Error as StdError;
use std::fmt::{self, Display};
use std::ops::{Bound, RangeBounds};

use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct LineRangeParseError {
    pub value: String,
}

impl Display for LineRangeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to parse line range '{:s}'", self.value)
    }
}

impl StdError for LineRangeParseError {}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct LineRange(Bound<usize>, Bound<usize>);

impl RangeBounds<usize> for LineRange {
    fn start_bound(&self) -> Bound<&usize> {
        self.0.as_ref()
    }

    fn end_bound(&self) -> Bound<&usize> {
        self.1.as_ref()
    }

    fn contains<U>(&self, item: &U) -> bool
    where
        usize: PartialOrd<U>,
        U: ?Sized + PartialOrd<usize>,
    {
        let left = match self.0 {
            Bound::Unbounded => true,
            Bound::Included(ref v) => v <= item,
            Bound::Excluded(ref v) => v < item,
        };
        let right = match self.1 {
            Bound::Unbounded => true,
            Bound::Included(ref v) => item <= v,
            Bound::Excluded(ref v) => item < v,
        };
        left && right
    }
}

impl Default for LineRange {
    fn default() -> Self {
        LineRange(Bound::Unbounded, Bound::Unbounded)
    }
}

impl LineRange {
    pub fn parse(range_raw: &str) -> Result<LineRange, LineRangeParseError> {
        let mut new_range = LineRange::default();

        let invalid = || LineRangeParseError {
            value: range_raw.to_owned(),
        };

        if let Some(upper) = range_raw.strip_prefix(':') {
            new_range.1 = Bound::Included(upper.parse().map_err(|_| invalid())?);
            return Ok(new_range);
        } else if let Some(lower) = range_raw.strip_suffix(':') {
            new_range.0 = Bound::Included(lower.parse().map_err(|_| invalid())?);
            return Ok(new_range);
        }

        let mut iter = range_raw.split(':');
        let line_numbers = (iter.next(), iter.next());
        if iter.next().is_some() {
            return Err(invalid());
        }

        match line_numbers {
            (Some(number), None) => {
                let number = number.parse().map_err(|_| invalid())?;
                new_range.0 = Bound::Included(number);
                new_range.1 = Bound::Included(number);
                Ok(new_range)
            }
            (Some(left), Some(right)) => {
                let lower = left.parse().map_err(|_| invalid())?;
                new_range.0 = Bound::Included(lower);

                if let Some(upper) = right.strip_prefix('+') {
                    let upper = upper.parse().map_err(|_| invalid())?;
                    let upper = lower.checked_add(upper).ok_or_else(invalid)?;
                    new_range.1 = Bound::Included(upper);
                } else if let Some(upper) = right.strip_prefix('-') {
                    if upper.strip_prefix('+').is_some() {
                        return Err(invalid());
                    }
                    let upper = upper.parse().map_err(|_| invalid())?;
                    let upper = lower.checked_sub(upper).ok_or_else(invalid)?;
                    new_range.0 = Bound::Included(upper);
                    new_range.1 = Bound::Included(lower);
                } else {
                    let upper = right.parse().map_err(|_| invalid())?;
                    new_range.1 = Bound::Included(upper);
                }

                Ok(new_range)
            }
            _ => Err(invalid()),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum RangeCheckResult {
    // Within one of the given ranges
    InRange,

    // Before the first range or within two ranges
    BeforeOrBetweenRanges,

    // Line number is outside of all ranges and larger than the last range.
    AfterLastRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LineRanges(Vec<LineRange>);

impl LineRanges {
    pub fn none() -> LineRanges {
        LineRanges::from(vec![])
    }

    pub fn all() -> LineRanges {
        LineRanges::from(vec![LineRange::default()])
    }

    pub fn from(mut ranges: Vec<LineRange>) -> LineRanges {
        ranges.sort_by(|a, b| match (a.end_bound(), b.end_bound()) {
            (Bound::Unbounded, Bound::Unbounded) => Ordering::Equal,
            (_, Bound::Unbounded) => Ordering::Less,
            (Bound::Unbounded, _) => Ordering::Greater,
            (Bound::Included(left), Bound::Included(right))
            | (Bound::Excluded(left), Bound::Excluded(right)) => left.cmp(right),
            (Bound::Included(left), Bound::Excluded(right)) => left
                .checked_add(1)
                .map_or(Ordering::Greater, |left| left.cmp(right)),
            (Bound::Excluded(left), Bound::Included(right)) => right
                .checked_add(1)
                .map_or(Ordering::Less, |right| left.cmp(&right)),
        });
        LineRanges(ranges)
    }

    pub(crate) fn check(&self, line: usize) -> RangeCheckResult {
        if self.0.iter().any(|r| r.contains(&line)) {
            RangeCheckResult::InRange
        } else if match self.0.last().map(|range| range.1) {
            None => false,
            Some(Bound::Included(upper)) => line <= upper,
            Some(Bound::Excluded(upper)) => line < upper,
            Some(Bound::Unbounded) => true,
        } {
            RangeCheckResult::BeforeOrBetweenRanges
        } else {
            RangeCheckResult::AfterLastRange
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HighlightedLineRanges(pub LineRanges);

impl Default for HighlightedLineRanges {
    fn default() -> Self {
        HighlightedLineRanges(LineRanges::none())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VisibleLines(pub LineRanges);

impl Default for VisibleLines {
    fn default() -> Self {
        VisibleLines(LineRanges::all())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::ops::Bound::*;

    #[test]
    fn test_parse_full() {
        let range = LineRange::parse("40:50").expect("Shouldn't fail on test!");
        assert_eq!(Included(40), range.0);
        assert_eq!(Included(50), range.1);
    }

    #[test]
    fn test_parse_partial_min() {
        let range = LineRange::parse(":50").expect("Shouldn't fail on test!");
        assert_eq!(Unbounded, range.0);
        assert_eq!(Included(50), range.1);
    }

    #[test]
    fn test_parse_partial_max() {
        let range = LineRange::parse("40:").expect("Shouldn't fail on test!");
        assert_eq!(Included(40), range.0);
        assert_eq!(Unbounded, range.1);
    }

    #[test]
    fn test_parse_single() {
        let range = LineRange::parse("40").expect("Shouldn't fail on test!");
        assert_eq!(Included(40), range.0);
        assert_eq!(Included(40), range.1);
    }

    #[test]
    fn test_parse_fail() {
        let range = LineRange::parse("40:50:80");
        assert!(range.is_err());
        let range = LineRange::parse("40::80");
        assert!(range.is_err());
        let range = LineRange::parse(":40:");
        assert!(range.is_err());
    }

    #[test]
    fn test_parse_plus() {
        let range = LineRange::parse("40:+10").expect("Shouldn't fail on test!");
        assert_eq!(Included(40), range.0);
        assert_eq!(Included(50), range.1);
    }

    #[test]
    fn test_parse_plus_overflow() {
        let range = LineRange::parse(&format!("{}:+1", usize::MAX));
        assert!(range.is_err());
    }

    #[test]
    fn test_parse_plus_fail() {
        let range = LineRange::parse("40:+z");
        assert!(range.is_err());
        let range = LineRange::parse("40:+-10");
        assert!(range.is_err());
        let range = LineRange::parse("40:+");
        assert!(range.is_err());
    }

    #[test]
    fn test_parse_minus_success() {
        let range = LineRange::parse("40:-10").expect("Shouldn't fail on test!");
        assert_eq!(Included(30), range.0);
        assert_eq!(Included(40), range.1);
    }

    #[test]
    fn test_parse_minus_edge_cases_success() {
        let range = LineRange::parse("5:-4").expect("Shouldn't fail on test!");
        assert_eq!(Included(1), range.0);
        assert_eq!(Included(5), range.1);
        let range = LineRange::parse("5:-5").expect("Shouldn't fail on test!");
        assert_eq!(Included(0), range.0);
        assert_eq!(Included(5), range.1);
        let range = LineRange::parse("5:-100");
        assert!(range.is_err());
    }

    #[test]
    fn test_parse_minus_fail() {
        let range = LineRange::parse("40:-z");
        assert!(range.is_err());
        let range = LineRange::parse("40:-+10");
        assert!(range.is_err());
        let range = LineRange::parse("40:-");
        assert!(range.is_err());
    }

    fn ranges(rs: &[&str]) -> LineRanges {
        LineRanges::from(rs.iter().map(|r| LineRange::parse(r).unwrap()).collect())
    }

    #[test]
    fn test_ranges_simple() {
        let ranges = ranges(&["3:8"]);

        assert_eq!(RangeCheckResult::BeforeOrBetweenRanges, ranges.check(2));
        assert_eq!(RangeCheckResult::InRange, ranges.check(5));
        assert_eq!(RangeCheckResult::AfterLastRange, ranges.check(9));
    }

    #[test]
    fn test_ranges_advanced() {
        let ranges = ranges(&["3:8", "11:20", "25:30"]);

        assert_eq!(RangeCheckResult::BeforeOrBetweenRanges, ranges.check(2));
        assert_eq!(RangeCheckResult::InRange, ranges.check(5));
        assert_eq!(RangeCheckResult::BeforeOrBetweenRanges, ranges.check(9));
        assert_eq!(RangeCheckResult::InRange, ranges.check(11));
        assert_eq!(RangeCheckResult::BeforeOrBetweenRanges, ranges.check(22));
        assert_eq!(RangeCheckResult::InRange, ranges.check(28));
        assert_eq!(RangeCheckResult::AfterLastRange, ranges.check(31));
    }

    #[test]
    fn test_ranges_open_low() {
        let ranges = ranges(&["3:8", ":5"]);

        assert_eq!(RangeCheckResult::InRange, ranges.check(1));
        assert_eq!(RangeCheckResult::InRange, ranges.check(3));
        assert_eq!(RangeCheckResult::InRange, ranges.check(7));
        assert_eq!(RangeCheckResult::AfterLastRange, ranges.check(9));
    }

    #[test]
    fn test_ranges_open_high() {
        let ranges = ranges(&["3:", "2:5"]);

        assert_eq!(RangeCheckResult::BeforeOrBetweenRanges, ranges.check(1));
        assert_eq!(RangeCheckResult::InRange, ranges.check(3));
        assert_eq!(RangeCheckResult::InRange, ranges.check(5));
        assert_eq!(RangeCheckResult::InRange, ranges.check(9));
    }

    #[test]
    fn test_ranges_all() {
        let ranges = LineRanges::all();

        assert_eq!(RangeCheckResult::InRange, ranges.check(1));
    }

    #[test]
    fn test_ranges_none() {
        let ranges = LineRanges::none();

        assert_ne!(RangeCheckResult::InRange, ranges.check(1));
    }
}
