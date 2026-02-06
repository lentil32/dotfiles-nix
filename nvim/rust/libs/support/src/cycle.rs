pub fn next_index<T: PartialEq>(items: &[T], current: &T) -> Option<usize> {
    if items.len() <= 1 {
        return None;
    }
    let idx = items.iter().position(|item| item == current)?;
    Some((idx + 1) % items.len())
}

pub fn next_item<'a, T: PartialEq>(items: &'a [T], current: &T) -> Option<&'a T> {
    let idx = next_index(items, current)?;
    items.get(idx)
}

#[cfg(test)]
mod tests {
    use super::{next_index, next_item};

    #[test]
    fn next_index_wraps() {
        let items = [1, 2, 3];
        assert_eq!(next_index(&items, &2), Some(2));
        assert_eq!(next_index(&items, &3), Some(0));
    }

    #[test]
    fn next_index_none_when_missing() {
        let items = [1, 2, 3];
        assert_eq!(next_index(&items, &9), None);
    }

    #[test]
    fn next_item_returns_wrapped_value() {
        let items = [1, 2, 3];
        assert_eq!(next_item(&items, &3), Some(&1));
    }
}
