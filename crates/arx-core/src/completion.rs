//! Completion popup — inline code-completion overlay triggered by
//! LSP `textDocument/completion` or manual invocation.
//!
//! The popup is positioned near the cursor and displays a filtered
//! list of completion items. Accepting an item replaces the "prefix"
//! (the word the cursor sits on) with the completion text.
//!
//! # State lifecycle
//!
//! ```text
//!     closed ──show()──▶ open(items, anchor, selected=0)
//!        ▲                        │
//!        │ accept / dismiss       │ next / prev
//!        │                        ▼
//!        └──────────────── update selection
//! ```

/// A single entry in the completion popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    /// The text that will be inserted when this item is accepted.
    pub insert_text: String,
    /// The label shown in the popup (often the same as `insert_text`,
    /// but can include extra decoration like type annotations).
    pub label: String,
    /// Optional detail string shown beside the label (e.g. the
    /// function signature or type).
    pub detail: Option<String>,
    /// Kind indicator for the icon column: `"fn"`, `"var"`, `"mod"`,
    /// etc. Derived from `lsp_types::CompletionItemKind`.
    pub kind: Option<String>,
}

/// Editor-side state for the completion popup.
///
/// Always present on [`crate::Editor`] but normally closed (zero
/// allocations). [`show`](Self::show) switches it into the
/// selection-accepting state and [`dismiss`](Self::dismiss) resets it.
#[derive(Debug, Default)]
pub struct CompletionPopup {
    /// Whether the popup is currently visible.
    open: bool,
    /// The byte offset in the buffer where the completion prefix
    /// starts. Accepting a completion replaces `anchor..cursor` with
    /// the selected item's `insert_text`.
    anchor: usize,
    /// Index into `items` for the highlighted row.
    selected: usize,
    /// The completion items to display.
    items: Vec<CompletionItem>,
}

impl CompletionPopup {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The byte offset where the completion prefix starts.
    pub fn anchor(&self) -> usize {
        self.anchor
    }

    pub fn items(&self) -> &[CompletionItem] {
        &self.items
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn selected_item(&self) -> Option<&CompletionItem> {
        self.items.get(self.selected)
    }

    /// Open the popup with `items`, anchored at `anchor` byte offset.
    pub fn show(&mut self, items: Vec<CompletionItem>, anchor: usize) {
        self.open = true;
        self.items = items;
        self.anchor = anchor;
        self.selected = 0;
    }

    /// Close the popup and drop the item list.
    pub fn dismiss(&mut self) {
        self.open = false;
        self.items.clear();
        self.selected = 0;
        self.anchor = 0;
    }

    /// Move the selection down one row (saturates at the last item).
    pub fn select_next(&mut self) {
        if !self.items.is_empty() && self.selected + 1 < self.items.len() {
            self.selected += 1;
        }
    }

    /// Move the selection up one row (saturates at 0).
    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Move the selection down by `n` rows (saturates at the last item).
    pub fn select_next_n(&mut self, n: usize) {
        if self.items.is_empty() {
            return;
        }
        self.selected = (self.selected + n).min(self.items.len() - 1);
    }

    /// Move the selection up by `n` rows (saturates at 0).
    pub fn select_prev_n(&mut self, n: usize) {
        self.selected = self.selected.saturating_sub(n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(labels: &[&str]) -> Vec<CompletionItem> {
        labels
            .iter()
            .map(|l| CompletionItem {
                insert_text: l.to_string(),
                label: l.to_string(),
                detail: None,
                kind: None,
            })
            .collect()
    }

    #[test]
    fn show_opens_and_sets_items() {
        let mut p = CompletionPopup::new();
        p.show(items(&["foo", "bar"]), 5);
        assert!(p.is_open());
        assert_eq!(p.items().len(), 2);
        assert_eq!(p.anchor(), 5);
        assert_eq!(p.selected_index(), 0);
    }

    #[test]
    fn dismiss_resets() {
        let mut p = CompletionPopup::new();
        p.show(items(&["a"]), 0);
        p.dismiss();
        assert!(!p.is_open());
        assert!(p.items().is_empty());
    }

    #[test]
    fn selection_saturates() {
        let mut p = CompletionPopup::new();
        p.show(items(&["a", "b", "c"]), 0);
        p.select_next();
        p.select_next();
        p.select_next(); // saturates at 2
        assert_eq!(p.selected_index(), 2);
        p.select_prev();
        assert_eq!(p.selected_index(), 1);
        p.select_prev();
        p.select_prev(); // saturates at 0
        assert_eq!(p.selected_index(), 0);
    }
}
