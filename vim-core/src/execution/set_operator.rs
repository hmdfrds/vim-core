//! The `+=`, `-=` and `^=` operators of `:set`, and Vim's number syntax.
//!
//! Pure functions over option values. The executor reads the current value
//! from the layer the command targets, computes the new one here, and
//! writes it back through the normal scope routing.
//!
//! Semantics follow `:help :set+=`, `:help :set-=` and `:help :set^=`, and
//! the edge cases were checked against headless Vim 9.1.

use compact_str::CompactString;

use crate::primitives::{FormatFlags, OptionKind, OptionValue};

/// Which `:set` operator to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetOperator {
    /// `+=`
    Append,
    /// `-=`
    Remove,
    /// `^=`
    Prepend,
}

impl SetOperator {
    /// The operator as typed, for error messages.
    pub(crate) const fn symbol(self) -> &'static str {
        match self {
            Self::Append => "+=",
            Self::Remove => "-=",
            Self::Prepend => "^=",
        }
    }
}

/// Parse an operand of `+=`, `-=` and `^=` the way Vim does: decimal,
/// `0x` hex, `0o` or leading-zero octal, with an optional minus sign. The
/// whole string must be the number. A plain `=` does not use this.
fn parse_vim_number(s: &str) -> Option<i64> {
    let (negative, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    if digits.is_empty() {
        return None;
    }
    // from_str_radix() takes a leading sign, which Vim does not after the
    // prefix, so the digits are checked first.
    let magnitude = if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        i64::from_str_radix(hex, 16).ok()?
    } else if let Some(oct) = digits
        .strip_prefix("0o")
        .or_else(|| digits.strip_prefix("0O"))
    {
        if !oct.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
            return None;
        }
        i64::from_str_radix(oct, 8).ok()?
    } else if digits.len() > 1
        && digits.starts_with('0')
        && digits.bytes().all(|b| (b'0'..=b'7').contains(&b))
    {
        i64::from_str_radix(digits, 8).ok()?
    } else if digits.bytes().all(|b| b.is_ascii_digit()) {
        digits.parse::<i64>().ok()?
    } else {
        return None;
    };
    Some(if negative { -magnitude } else { magnitude })
}

/// Convert a parsed number into the [`OptionValue`] variant `like` uses.
///
/// # Errors
///
/// Returns Vim's E487 message for a negative value on an unsigned option.
fn number_value(n: i64, like: &OptionValue) -> Result<OptionValue, &'static str> {
    match like {
        OptionValue::Signed(_) => Ok(OptionValue::Signed(n)),
        _ => usize::try_from(n)
            .map(OptionValue::Unsigned)
            .map_err(|_| "E487: Argument must be positive"),
    }
}

fn as_i64(value: &OptionValue) -> i64 {
    match value {
        OptionValue::Unsigned(v) => i64::try_from(*v).unwrap_or(i64::MAX),
        OptionValue::Signed(v) => *v,
        OptionValue::Bool(v) => i64::from(*v),
        OptionValue::Str(_) => 0,
    }
}

/// Apply `op` with operand `value` to the current value `base` of an
/// option of the given `kind`.
///
/// `flag_items` marks a comma list whose items are flags (Vim's
/// `whichwrap`): adding an item that is already there moves it to the new
/// position instead of being refused.
///
/// String results are returned unvalidated; the caller runs the option's
/// own value check (E539 for `formatoptions`, E524/E525 for `comments`).
///
/// # Errors
///
/// Returns Vim's message without the trailing `: {arg}`: E474 for a boolean
/// option, E521 when a number option gets something that is not a number,
/// E487 when an unsigned option would go negative.
pub(crate) fn apply_set_operator(
    kind: OptionKind,
    flag_items: bool,
    base: &OptionValue,
    op: SetOperator,
    value: &str,
) -> Result<OptionValue, &'static str> {
    match kind {
        OptionKind::Bool => Err("E474: Invalid argument"),
        OptionKind::Number => {
            let operand = parse_vim_number(value).ok_or("E521: Number required after =")?;
            let current = as_i64(base);
            let result = match op {
                SetOperator::Append => current.saturating_add(operand),
                SetOperator::Remove => current.saturating_sub(operand),
                SetOperator::Prepend => current.saturating_mul(operand),
            };
            number_value(result, base)
        }
        kind => {
            let OptionValue::Str(current) = base else {
                return Err("E474: Invalid argument");
            };
            Ok(OptionValue::Str(CompactString::from(apply_to_string(
                kind, flag_items, current, op, value,
            ))))
        }
    }
}

/// The string half of [`apply_set_operator`].
fn apply_to_string(
    kind: OptionKind,
    flag_items: bool,
    current: &str,
    op: SetOperator,
    value: &str,
) -> String {
    // An empty operand never changes the value.
    if value.is_empty() {
        return current.to_owned();
    }
    match kind {
        OptionKind::FlagList => {
            let combined = match op {
                SetOperator::Append => format!("{current}{value}"),
                SetOperator::Prepend => format!("{value}{current}"),
                // The operand must appear exactly as written: on "tcq",
                // `-=tc` removes "tc" but `-=ct` changes nothing.
                SetOperator::Remove => current.replacen(value, "", 1),
            };
            FormatFlags::normalize(&combined)
        }
        OptionKind::CommaList => {
            // A list of flags keeps a repeated item at its last position;
            // the other comma lists refuse duplicates.
            let present = find_item(current, value).is_some();
            match op {
                SetOperator::Append | SetOperator::Prepend if present && !flag_items => {
                    current.to_owned()
                }
                SetOperator::Append | SetOperator::Prepend => {
                    let combined = match (op, current.is_empty()) {
                        (_, true) => value.to_owned(),
                        (SetOperator::Append, false) => format!("{current},{value}"),
                        _ => format!("{value},{current}"),
                    };
                    if flag_items {
                        dedupe_items_keep_last(&combined)
                    } else {
                        combined
                    }
                }
                SetOperator::Remove => remove_item(current, value),
            }
        }
        OptionKind::String | OptionKind::Bool | OptionKind::Number => match op {
            SetOperator::Append => format!("{current}{value}"),
            SetOperator::Prepend => format!("{value}{current}"),
            SetOperator::Remove => current.replacen(value, "", 1),
        },
    }
}

/// Find `item` in a comma list as a whole run of items: it must start at
/// the beginning or after a comma and end at the end or before a comma.
/// `item` may itself contain commas (`:set com-=:a,:b`).
fn find_item(list: &str, item: &str) -> Option<usize> {
    list.match_indices(item).map(|(pos, _)| pos).find(|&pos| {
        let starts = pos == 0 || list.as_bytes().get(pos - 1) == Some(&b',');
        let end = pos + item.len();
        let ends = end == list.len() || list.as_bytes().get(end) == Some(&b',');
        starts && ends
    })
}

/// Remove the first whole-item occurrence of `item`, with one comma: the
/// one after it when it is first, otherwise the one before it.
fn remove_item(list: &str, item: &str) -> String {
    let Some(pos) = find_item(list, item) else {
        return list.to_owned();
    };
    let end = pos + item.len();
    let (cut_start, cut_end) = if pos == 0 {
        (0, (end + 1).min(list.len()))
    } else {
        (pos - 1, end)
    };
    let mut out = String::with_capacity(list.len());
    out.push_str(list.get(..cut_start).unwrap_or(""));
    out.push_str(list.get(cut_end..).unwrap_or(""));
    out
}

/// Drop items that appear again later in the list.
fn dedupe_items_keep_last(list: &str) -> String {
    let items: Vec<&str> = list.split(',').collect();
    items
        .iter()
        .enumerate()
        .filter(|&(i, item)| !items.iter().skip(i + 1).any(|later| later == item))
        .map(|(_, item)| *item)
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::OptionId;

    fn s(v: &str) -> OptionValue {
        OptionValue::Str(CompactString::from(v))
    }

    fn apply_id(
        id: OptionId,
        base: &OptionValue,
        op: SetOperator,
        value: &str,
    ) -> Result<OptionValue, &'static str> {
        apply_set_operator(id.kind(), id == OptionId::WhichWrap, base, op, value)
    }

    fn apply_str(id: OptionId, base: &str, op: SetOperator, value: &str) -> String {
        match apply_id(id, &s(base), op, value).unwrap() {
            OptionValue::Str(v) => v.to_string(),
            other => panic!("not a string: {other:?}"),
        }
    }

    use SetOperator::{Append, Prepend, Remove};

    #[test]
    fn vim_number_syntax() {
        assert_eq!(parse_vim_number("10"), Some(10));
        assert_eq!(parse_vim_number("0"), Some(0));
        assert_eq!(parse_vim_number("-3"), Some(-3));
        assert_eq!(parse_vim_number("0x10"), Some(16));
        assert_eq!(parse_vim_number("010"), Some(8));
        assert_eq!(parse_vim_number("0o10"), Some(8));
        // Not octal when a digit is 8 or 9.
        assert_eq!(parse_vim_number("08"), Some(8));
        assert_eq!(parse_vim_number(""), None);
        assert_eq!(parse_vim_number("-"), None);
        assert_eq!(parse_vim_number("5x"), None);
        assert_eq!(parse_vim_number(" 5"), None);
        assert_eq!(parse_vim_number("x"), None);
        // Vim gives E521 for a sign after the prefix.
        assert_eq!(parse_vim_number("0x+5"), None);
        assert_eq!(parse_vim_number("0x-5"), None);
        assert_eq!(parse_vim_number("0o+7"), None);
        assert_eq!(parse_vim_number("0o-7"), None);
        assert_eq!(parse_vim_number("0x"), None);
        assert_eq!(parse_vim_number("-0x1f"), Some(-31));
    }

    // Vim 9.1, tw=10: `tw+=4` 14, `tw-=4` 6, `tw^=4` 40, `tw^=0` 0,
    // `tw+=-3` 7, `tw-=-3` 13, `tw+=0x10` 26, `tw-=40` E487, `tw+=x` E521.
    #[test]
    fn number_operators() {
        let tw = OptionValue::Unsigned(10);
        let id = OptionId::TextWidth;
        for (op, value, want) in [
            (Append, "4", 14),
            (Remove, "4", 6),
            (Prepend, "4", 40),
            (Prepend, "0", 0),
            (Append, "-3", 7),
            (Remove, "-3", 13),
            (Append, "0x10", 26),
        ] {
            assert_eq!(
                apply_id(id, &tw, op, value),
                Ok(OptionValue::Unsigned(want)),
                "tw{}{value}",
                op.symbol()
            );
        }
        assert_eq!(
            apply_id(id, &tw, Remove, "40"),
            Err("E487: Argument must be positive")
        );
        assert_eq!(
            apply_id(id, &tw, Append, "x"),
            Err("E521: Number required after =")
        );
        assert_eq!(
            apply_id(id, &tw, Append, ""),
            Err("E521: Number required after =")
        );
    }

    // Vim 9.1: sts=0 then `sts-=2` gives -2; ul=5 then `ul-=10` gives -5.
    #[test]
    fn signed_number_may_go_negative() {
        assert_eq!(
            apply_id(OptionId::SoftTabStop, &OptionValue::Signed(0), Remove, "2"),
            Ok(OptionValue::Signed(-2))
        );
    }

    #[test]
    fn bool_option_rejects_operators() {
        assert_eq!(
            apply_id(OptionId::AutoIndent, &OptionValue::Bool(true), Append, "1"),
            Err("E474: Invalid argument")
        );
    }

    // Vim 9.1 results for each operation, starting from fo=tcq.
    #[test]
    fn formatoptions_operators_match_vim() {
        let fo = OptionId::FormatOptions;
        for (op, value, want) in [
            (Remove, "t", "cq"),
            (Remove, "c", "tq"),
            (Remove, "qt", "tcq"),
            (Remove, "tc", "q"),
            (Remove, "ct", "tcq"),
            (Remove, "x", "tcq"),
            (Remove, "", "tcq"),
            (Append, "t", "cqt"),
            (Append, "tt", "cqt"),
            (Append, "tr", "cqtr"),
            (Append, "rt", "cqrt"),
            (Append, "r", "tcqr"),
            (Append, "ro", "tcqro"),
            (Append, "c,q", "tc,q"),
            (Append, "", "tcq"),
            (Prepend, "r", "rtcq"),
            (Prepend, "q", "tcq"),
            (Prepend, "ct", "tcq"),
        ] {
            assert_eq!(
                apply_str(fo, "tcq", op, value),
                want,
                "fo{}{value}",
                op.symbol()
            );
        }
        assert_eq!(apply_str(fo, "", Append, "c"), "c");
        assert_eq!(apply_str(fo, "", Prepend, "c"), "c");
        assert_eq!(apply_str(fo, "", Remove, "c"), "");
    }

    // Vim 9.1 results, starting from com=:a,:b,:c.
    #[test]
    fn comma_list_operators_match_vim() {
        let com = OptionId::Comments;
        for (op, value, want) in [
            (Remove, ":b", ":a,:c"),
            (Remove, ":a", ":b,:c"),
            (Remove, ":c", ":a,:b"),
            (Remove, ":a,:b", ":c"),
            (Remove, ":b,:c", ":a"),
            (Remove, ":a,:c", ":a,:b,:c"),
            (Remove, ":x", ":a,:b,:c"),
            (Remove, "b", ":a,:b,:c"),
            (Remove, ":", ":a,:b,:c"),
            (Remove, "", ":a,:b,:c"),
            (Append, ":b", ":a,:b,:c"),
            (Append, ":a", ":a,:b,:c"),
            (Append, ":a,:b", ":a,:b,:c"),
            (Append, ":d,:a", ":a,:b,:c,:d,:a"),
            (Append, ":a,:d", ":a,:b,:c,:a,:d"),
            (Append, "", ":a,:b,:c"),
            (Prepend, ":c", ":a,:b,:c"),
            (Prepend, ":b", ":a,:b,:c"),
            (Prepend, ":d,:a", ":d,:a,:a,:b,:c"),
            (Prepend, "", ":a,:b,:c"),
        ] {
            assert_eq!(
                apply_str(com, ":a,:b,:c", op, value),
                want,
                "com{}{value}",
                op.symbol()
            );
        }
        assert_eq!(apply_str(com, ":ab,:b", Remove, ":b"), ":ab");
        assert_eq!(apply_str(com, ":b,:ab", Remove, ":b"), ":ab");
        assert_eq!(apply_str(com, ":b,:b", Remove, ":b"), ":b");
        assert_eq!(apply_str(com, "b:#", Remove, "b:#"), "");
        assert_eq!(apply_str(com, "", Append, "b:#"), "b:#");
        assert_eq!(apply_str(com, "", Prepend, "b:#"), "b:#");
    }

    #[test]
    fn default_comments_operators_match_vim() {
        let com = OptionId::Comments;
        let default = crate::primitives::DEFAULT_COMMENTS;
        assert_eq!(
            apply_str(com, default, Remove, "b:#"),
            "s1:/*,mb:*,ex:*/,://,:%,:XCOMM,n:>,fb:-"
        );
        assert_eq!(
            apply_str(com, default, Append, "b:##"),
            "s1:/*,mb:*,ex:*/,://,b:#,:%,:XCOMM,n:>,fb:-,b:##"
        );
        assert_eq!(
            apply_str(com, default, Prepend, "b:##"),
            "b:##,s1:/*,mb:*,ex:*/,://,b:#,:%,:XCOMM,n:>,fb:-"
        );
        assert_eq!(apply_str(com, default, Append, "b:#"), default);
        assert_eq!(apply_str(com, default, Prepend, "b:#"), default);
    }

    // Vim 9.1: ww=b,s then `ww+=b` "s,b", `ww+=h` "b,s,h", `ww+=h,b`
    // "s,h,b", `ww-=b` "s"; bs=indent,eol then `bs+=eol` unchanged.
    #[test]
    fn whichwrap_and_backspace_match_vim() {
        let ww = OptionId::WhichWrap;
        assert_eq!(apply_str(ww, "b,s", Append, "b"), "s,b");
        assert_eq!(apply_str(ww, "b,s", Append, "h"), "b,s,h");
        assert_eq!(apply_str(ww, "b,s", Append, "h,b"), "s,h,b");
        assert_eq!(apply_str(ww, "b,s", Remove, "b"), "s");
        let bs = OptionId::Backspace;
        assert_eq!(
            apply_str(bs, "indent,eol", Append, "start"),
            "indent,eol,start"
        );
        assert_eq!(apply_str(bs, "indent,eol", Append, "eol"), "indent,eol");
        // isk=@,48-57: `isk+=-` "@,48-57,-", `isk-=@` "48-57".
        let isk = OptionId::IsKeyword;
        assert_eq!(apply_str(isk, "@,48-57", Append, "-"), "@,48-57,-");
        assert_eq!(apply_str(isk, "@,48-57", Remove, "@"), "48-57");
    }

    // Vim 9.1, cms=x%s: `cms+=y` "x%sy", `cms^=y` "yx%s", `cms-=x` "%s".
    #[test]
    fn plain_string_operators_match_vim() {
        let cms = OptionId::CommentString;
        assert_eq!(apply_str(cms, "x%s", Append, "y"), "x%sy");
        assert_eq!(apply_str(cms, "x%s", Prepend, "y"), "yx%s");
        assert_eq!(apply_str(cms, "x%s", Remove, "x"), "%s");
        assert_eq!(apply_str(cms, "x%s", Remove, "q"), "x%s");
    }
}
