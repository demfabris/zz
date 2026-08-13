//! Row data and the collection behind a [`crate::select::Select`].

use gpui::SharedString;

/// One row of a [`crate::select::Select`]: a label to show and a value to
/// report. Implemented for the string types.
pub trait SelectItem: Clone {
    /// What the select reports when this row is picked, and what identifies it.
    type Value: Clone + PartialEq + 'static;

    /// The row's label, also shown in the trigger once picked.
    fn title(&self) -> SharedString;

    fn value(&self) -> &Self::Value;
}

impl SelectItem for String {
    type Value = Self;

    fn title(&self) -> SharedString {
        SharedString::from(self.clone())
    }

    fn value(&self) -> &Self::Value {
        self
    }
}

impl SelectItem for SharedString {
    type Value = Self;

    fn title(&self) -> SharedString {
        self.clone()
    }

    fn value(&self) -> &Self::Value {
        self
    }
}

impl SelectItem for &'static str {
    type Value = Self;

    fn title(&self) -> SharedString {
        SharedString::from(*self)
    }

    fn value(&self) -> &Self::Value {
        self
    }
}

/// The collection of rows behind a [`crate::select::Select`].
pub trait SelectDelegate: 'static {
    type Item: SelectItem + 'static;

    fn items_count(&self) -> usize;

    fn item(&self, ix: usize) -> Option<&Self::Item>;

    fn position(&self, value: &<Self::Item as SelectItem>::Value) -> Option<usize> {
        (0..self.items_count()).find(|ix| self.item(*ix).is_some_and(|item| item.value() == value))
    }
}

impl<T: SelectItem + 'static> SelectDelegate for Vec<T> {
    type Item = T;

    fn items_count(&self) -> usize {
        self.len()
    }

    fn item(&self, ix: usize) -> Option<&Self::Item> {
        self.as_slice().get(ix)
    }
}

#[cfg(test)]
mod tests {
    use super::SelectDelegate as _;

    #[test]
    fn vec_delegate_positions_by_value() {
        let items = vec!["alpha", "beta"];

        assert_eq!(items.items_count(), 2);
        assert_eq!(items.position(&"beta"), Some(1));
        assert_eq!(items.position(&"gamma"), None);
    }
}
