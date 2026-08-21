use std::cmp::Ordering;

use zz_protocol::ServerError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TmuxSortOrder {
    Activity,
    Creation,
    Index,
    Modifier,
    Name,
    Order,
    Size,
    Z,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TmuxSort {
    order: Option<TmuxSortOrder>,
    reversed: bool,
}

impl TmuxSort {
    pub fn parse(
        value: Option<&str>,
        reversed: bool,
        default: Option<TmuxSortOrder>,
    ) -> Result<Self, ServerError> {
        let order = match value {
            Some(value) => Some(match value.to_ascii_lowercase().as_str() {
                "activity" => TmuxSortOrder::Activity,
                "creation" => TmuxSortOrder::Creation,
                "index" | "key" => TmuxSortOrder::Index,
                "modifier" => TmuxSortOrder::Modifier,
                "name" | "title" => TmuxSortOrder::Name,
                "order" => TmuxSortOrder::Order,
                "size" => TmuxSortOrder::Size,
                "z" => TmuxSortOrder::Z,
                _ => {
                    return Err(ServerError::InvalidCommand("invalid sort order".to_owned()));
                }
            }),
            None => default,
        };
        Ok(Self { order, reversed })
    }

    #[must_use]
    pub const fn order(self) -> Option<TmuxSortOrder> {
        self.order
    }

    #[must_use]
    pub const fn reversed(self) -> bool {
        self.reversed
    }

    pub fn apply<T>(self, values: &mut [T], compare: impl Fn(&T, &T) -> Ordering) {
        match self.order {
            None => {}
            Some(TmuxSortOrder::Order) => {
                if self.reversed {
                    values.reverse();
                }
            }
            Some(_) => values.sort_by(|left, right| {
                let ordering = compare(left, right);
                if self.reversed {
                    ordering.reverse()
                } else {
                    ordering
                }
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_are_case_insensitive_and_invalid_orders_match_tmux() {
        assert_eq!(
            TmuxSort::parse(Some("KEY"), false, None).unwrap().order(),
            Some(TmuxSortOrder::Index)
        );
        assert_eq!(
            TmuxSort::parse(Some("title"), false, None).unwrap().order(),
            Some(TmuxSortOrder::Name)
        );
        assert!(matches!(
            TmuxSort::parse(Some("nope"), false, None),
            Err(ServerError::InvalidCommand(message)) if message == "invalid sort order"
        ));
    }

    #[test]
    fn reversal_only_changes_an_explicit_or_default_order() {
        let mut untouched = vec![3, 1, 2];
        TmuxSort::parse(None, true, None)
            .unwrap()
            .apply(&mut untouched, Ord::cmp);
        assert_eq!(untouched, [3, 1, 2]);

        let mut ordered = vec![3, 1, 2];
        TmuxSort::parse(None, true, Some(TmuxSortOrder::Index))
            .unwrap()
            .apply(&mut ordered, Ord::cmp);
        assert_eq!(ordered, [3, 2, 1]);

        let mut reversed_input = vec![3, 1, 2];
        TmuxSort::parse(Some("order"), true, None)
            .unwrap()
            .apply(&mut reversed_input, Ord::cmp);
        assert_eq!(reversed_input, [2, 1, 3]);
    }
}
