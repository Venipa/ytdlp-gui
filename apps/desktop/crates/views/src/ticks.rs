//! The rows ticked for a bulk action, and how clicks and keys change them.

use std::collections::HashSet;

use domain::DownloadId;

/// The ticked rows, and the row a Shift range starts from.
#[derive(Debug, Default)]
pub(crate) struct Ticks {
    set: HashSet<DownloadId>,
    /// The last row ticked or unticked on its own; Shift+click ticks from here.
    anchor: Option<DownloadId>,
}

impl Ticks {
    pub(crate) fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub(crate) fn contains(&self, id: DownloadId) -> bool {
        self.set.contains(&id)
    }

    /// The ticked rows in list order (`order` is the list's, so hidden rows come last).
    pub(crate) fn ids(&self, order: &[DownloadId]) -> Vec<DownloadId> {
        let mut ids: Vec<DownloadId> = order
            .iter()
            .copied()
            .filter(|id| self.set.contains(id))
            .collect();
        ids.extend(self.set.iter().copied().filter(|id| !order.contains(id)));
        ids
    }

    /// A click on the box, Ctrl+click, or a plain click while anything is ticked.
    pub(crate) fn toggle(&mut self, id: DownloadId) {
        if !self.set.remove(&id) {
            self.set.insert(id);
        }
        self.anchor = Some(id);
    }

    /// Shift+click: ticks every row between the anchor (else `from`, the open row) and `to`,
    /// in list order. With neither, it ticks `to` alone.
    pub(crate) fn range(&mut self, order: &[DownloadId], from: Option<DownloadId>, to: DownloadId) {
        let start = self
            .anchor
            .or(from)
            .and_then(|a| order.iter().position(|x| *x == a));
        let end = order.iter().position(|x| *x == to);
        match (start, end) {
            (Some(a), Some(b)) => self.set.extend(&order[a.min(b)..=a.max(b)]),
            _ => {
                self.set.insert(to);
                self.anchor = Some(to);
            }
        }
    }

    /// Shift+Up or Down from `at`: ticks it and the row `by` away; returns that row.
    pub(crate) fn extend(
        &mut self,
        order: &[DownloadId],
        at: Option<DownloadId>,
        by: isize,
    ) -> Option<DownloadId> {
        let here = at.and_then(|id| order.iter().position(|x| *x == id));
        let next = match here {
            None => 0,
            Some(i) => i.saturating_add_signed(by).min(order.len().checked_sub(1)?),
        };
        let next_id = *order.get(next)?;
        if let Some(i) = here {
            self.set.insert(order[i]);
            self.anchor.get_or_insert(order[i]);
        }
        self.set.insert(next_id);
        Some(next_id)
    }

    /// Ticks every row in `ids` (Ctrl+A, a group's box), or unticks them when all already are.
    pub(crate) fn all(&mut self, ids: &[DownloadId]) {
        match ids.iter().all(|id| self.set.contains(id)) {
            true => ids.iter().for_each(|id| {
                self.set.remove(id);
            }),
            false => self.set.extend(ids),
        }
    }

    /// Whether every row in `ids` is ticked (and there is one).
    pub(crate) fn has_all(&self, ids: &[DownloadId]) -> bool {
        !ids.is_empty() && ids.iter().all(|id| self.set.contains(id))
    }

    /// Drops rows that no longer exist.
    pub(crate) fn retain(&mut self, exists: impl Fn(DownloadId) -> bool) {
        self.set.retain(|id| exists(*id));
    }

    pub(crate) fn clear(&mut self) {
        self.set.clear();
        self.anchor = None;
    }
}

#[cfg(test)]
mod tests {
    use super::Ticks;
    use domain::DownloadId;

    fn order() -> Vec<DownloadId> {
        (1..=6).map(DownloadId).collect()
    }

    #[test]
    fn shift_ticks_the_range_from_the_last_toggle_either_way() {
        let order = order();
        let mut t = Ticks::default();
        t.toggle(DownloadId(2));
        t.range(&order, None, DownloadId(5));
        assert_eq!(t.ids(&order), [2, 3, 4, 5].map(DownloadId));
        let mut t = Ticks::default();
        t.toggle(DownloadId(5));
        t.range(&order, None, DownloadId(3));
        assert_eq!(t.ids(&order), [3, 4, 5].map(DownloadId));
        // nothing ticked yet: the open row is where the range starts
        let mut t = Ticks::default();
        t.range(&order, Some(DownloadId(1)), DownloadId(2));
        assert_eq!(t.ids(&order), [1, 2].map(DownloadId));
    }

    #[test]
    fn select_all_twice_clears_and_keys_extend() {
        let order = order();
        let mut t = Ticks::default();
        t.all(&order);
        assert!(t.has_all(&order));
        t.all(&order);
        assert!(t.is_empty());
        let at = t.extend(&order, Some(DownloadId(3)), 1);
        assert_eq!(at, Some(DownloadId(4)));
        assert_eq!(t.extend(&order, at, 1), Some(DownloadId(5)));
        assert_eq!(t.ids(&order), [3, 4, 5].map(DownloadId));
        assert_eq!(
            t.extend(&order, Some(DownloadId(6)), 1),
            Some(DownloadId(6))
        );
    }
}
