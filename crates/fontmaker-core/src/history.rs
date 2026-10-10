//! Undo/redo.
//!
//! Commands are stored as closures paired with an inverse closure. Glyph edits
//! are small and local, so storing the "before" value of the touched glyph is
//! both cheaper and far less bug-prone than replaying a command log.
//!
//! Drag operations coalesce: a whole mouse-down to mouse-up gesture becomes a
//! single entry. Call [`History::begin`] at gesture start, mutate freely, then
//! [`History::commit`] at gesture end.

use std::collections::HashMap;

use crate::document::Document;
use crate::glyph::Glyph;

/// One undoable change to the document.
pub enum Edit {
    /// Replace a glyph with new contents.
    SetGlyph {
        name: String,
        before: Option<Glyph>,
        after: Option<Glyph>,
    },
    /// Insert a new glyph that did not exist before.
    AddGlyph(Glyph),
    /// Remove a glyph that will be restored on undo.
    RemoveGlyph {
        name: String,
        /// Position in `glyph_order` at removal time.
        index: usize,
        glyph: Glyph,
    },
    /// Change one field of a glyph in place.
    SetAdvance { name: String, before: f64, after: f64 },
    SetLsb { name: String, before: f64, after: f64 },
    /// Reorder or rename the glyph order. Captured as the whole vector because
    /// it is tiny and this keeps the logic simple.
    SetGlyphOrder { before: Vec<String>, after: Vec<String> },
    /// Snapshot of a wholesale document change (script switch, form generation,
    /// metrics change). Larger, but rare.
    SetDocument { before: Box<Document>, after: Box<Document> },
}

impl Edit {
    fn apply(&self, doc: &mut Document) {
        match self {
            Edit::SetGlyph { name, after, .. } => {
                match after {
                    Some(g) => {
                        doc.glyphs.insert(name.clone(), g.clone());
                    }
                    None => {
                        doc.glyphs.remove(name);
                    }
                }
            }
            Edit::AddGlyph(g) => {
                doc.add_glyph(g.clone());
            }
            Edit::RemoveGlyph { name, index, glyph } => {
                if doc.glyphs.remove(name).is_some() {
                    let i = (*index).min(doc.glyph_order.len());
                    doc.glyph_order.insert(i, name.clone());
                }
                doc.glyphs.insert(name.clone(), glyph.clone());
            }
            Edit::SetAdvance { name, after, .. } => {
                if let Some(g) = doc.glyph_mut(name) {
                    g.advance = *after;
                }
            }
            Edit::SetLsb { name, after, .. } => {
                if let Some(g) = doc.glyph_mut(name) {
                    g.lsb = *after;
                }
            }
            Edit::SetGlyphOrder { after, .. } => {
                doc.glyph_order = after.clone();
            }
            Edit::SetDocument { after, .. } => {
                *doc = (**after).clone();
            }
        }
    }

    fn revert(&self, doc: &mut Document) {
        match self {
            Edit::SetGlyph { name, before, .. } => {
                match before {
                    Some(g) => {
                        doc.glyphs.insert(name.clone(), g.clone());
                    }
                    None => {
                        doc.glyphs.remove(name);
                    }
                }
            }
            Edit::AddGlyph(g) => {
                doc.remove_glyph(&g.name);
            }
            Edit::RemoveGlyph { name, .. } => {
                doc.remove_glyph(name);
            }
            Edit::SetAdvance { name, before, .. } => {
                if let Some(g) = doc.glyph_mut(name) {
                    g.advance = *before;
                }
            }
            Edit::SetLsb { name, before, .. } => {
                if let Some(g) = doc.glyph_mut(name) {
                    g.lsb = *before;
                }
            }
            Edit::SetGlyphOrder { before, .. } => {
                doc.glyph_order = before.clone();
            }
            Edit::SetDocument { before, .. } => {
                *doc = (**before).clone();
            }
        }
    }
}

/// A gesture touching more than this many glyphs is recorded as one document
/// snapshot instead of one edit per glyph. Keeps undo at one step for
/// operations that change a whole script at once.
const COLLAPSE_THRESHOLD: usize = 2;

/// Bounded undo stack.
pub struct History {
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// Snapshot taken by [`History::begin`], applied on commit.
    pending: Option<PendingEdit>,
    limit: usize,
}

struct PendingEdit {
    /// Glyph contents captured when the gesture began.
    glyphs: HashMap<String, Option<Glyph>>,
    order: Vec<String>,
    doc_snapshot: Box<Document>,
}

impl Default for History {
    fn default() -> Self {
        Self::new(200)
    }
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            pending: None,
            limit,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|_| "")
    }

    /// Start coalescing a gesture. Capture `doc`'s current state; any glyph the
    /// caller touches afterwards records its pre-gesture contents.
    pub fn begin(&mut self, doc: &Document) {
        self.pending = Some(PendingEdit {
            glyphs: HashMap::new(),
            order: doc.glyph_order.clone(),
            doc_snapshot: Box::new(doc.clone()),
        });
    }

    /// Record the pre-gesture state of a glyph, once per gesture.
    pub fn touch(&mut self, doc: &Document, name: &str) {
        if let Some(p) = &mut self.pending {
            p.glyphs
                .entry(name.to_string())
                .or_insert_with(|| doc.glyph(name).cloned());
        }
    }

    /// Close the gesture and push an undo entry if anything actually changed.
    pub fn commit(&mut self, doc: &Document) -> bool {
        let Some(pending) = self.pending.take() else { return false };
        let mut edits: Vec<Edit> = Vec::new();

        for (name, before) in &pending.glyphs {
            let after = doc.glyph(name);
            if after != before.as_ref() {
                edits.push(Edit::SetGlyph {
                    name: name.clone(),
                    before: before.clone(),
                    after: after.cloned(),
                });
            }
        }
        if pending.order != doc.glyph_order {
            edits.push(Edit::SetGlyphOrder {
                before: pending.order.clone(),
                after: doc.glyph_order.clone(),
            });
        }

        // A wholesale change (script switch, form generation) touches many glyphs
        // at once; collapse that into one snapshot so undo is a single step.
        if edits.len() > COLLAPSE_THRESHOLD {
            edits.clear();
            if *pending.doc_snapshot != *doc {
                edits.push(Edit::SetDocument {
                    before: pending.doc_snapshot,
                    after: Box::new(doc.clone()),
                });
            }
        }

        if edits.is_empty() {
            return false;
        }
        self.push(edits);
        true
    }

    /// Abandon the current gesture without recording anything.
    pub fn abort(&mut self) {
        self.pending = None;
    }

    /// Record a single edit outside of a gesture: apply it to `doc` and make it
    /// undoable.
    pub fn record(&mut self, doc: &mut Document, edit: Edit) {
        edit.apply(doc);
        self.push(vec![edit]);
    }

    fn push(&mut self, edits: Vec<Edit>) {
        for e in edits {
            self.undo.push(e);
        }
        if self.undo.len() > self.limit {
            let excess = self.undo.len() - self.limit;
            self.undo.drain(0..excess);
        }
        self.redo.clear();
    }

    /// Undo the most recent change, returning `false` when there is nothing to undo.
    pub fn undo(&mut self, doc: &mut Document) -> bool {
        let Some(edit) = self.undo.pop() else { return false };
        edit.revert(doc);
        self.redo.push(edit);
        true
    }

    /// Redo the most recently undone change.
    pub fn redo(&mut self, doc: &mut Document) -> bool {
        let Some(edit) = self.redo.pop() else { return false };
        edit.apply(doc);
        self.undo.push(edit);
        true
    }

    pub fn depth(&self) -> usize {
        self.undo.len()
    }

    /// Drop all history. Called after loading a project.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyph::{Contour, Form};

    fn doc_with_a() -> Document {
        let mut doc = Document::default();
        let mut g = Glyph::new("A").with_char('A');
        g.contours.push(Contour::new(vec![], true));
        doc.add_glyph(g);
        doc
    }

    #[test]
    fn undo_and_redo_set_glyph() {
        let mut doc = doc_with_a();
        let mut h = History::default();

        h.begin(&doc);
        h.touch(&doc, "A");
        doc.glyph_mut("A").unwrap().advance = 700.0;
        assert!(h.commit(&doc));

        assert_eq!(doc.glyph("A").unwrap().advance, 700.0);
        assert!(h.undo(&mut doc));
        assert_eq!(doc.glyph("A").unwrap().advance, 500.0);
        assert!(h.redo(&mut doc));
        assert_eq!(doc.glyph("A").unwrap().advance, 700.0);
    }

    #[test]
    fn unchanged_gesture_records_nothing() {
        let doc = doc_with_a();
        let mut h = History::default();
        h.begin(&doc);
        h.touch(&doc, "A");
        assert!(!h.commit(&doc));
        assert_eq!(h.depth(), 0);
    }

    #[test]
    fn many_glyph_edits_collapse_to_one_snapshot() {
        let mut doc = doc_with_a();
        for c in "BCD".chars() {
            doc.add_glyph(Glyph::new(c.to_string()).with_char(c));
        }
        let mut h = History::default();
        h.begin(&doc);
        for c in "BCD".chars() {
            h.touch(&doc, &c.to_string());
            let g = doc.glyph_mut(&c.to_string()).unwrap();
            g.form = Form::Medi;
            g.advance = 111.0;
        }
        assert!(h.commit(&doc));
        assert_eq!(h.depth(), 1, "should be a single undo step");
        assert!(h.undo(&mut doc));
        assert_eq!(doc.glyph("B").unwrap().advance, 500.0);
    }

    #[test]
    fn add_and_remove_glyph_undo() {
        let mut doc = doc_with_a();
        let mut h = History::default();
        h.record(&mut doc, Edit::AddGlyph(Glyph::new("Z").with_char('Z')));
        assert!(doc.glyph("Z").is_some());
        assert!(h.undo(&mut doc));
        assert!(doc.glyph("Z").is_none());
        assert!(h.redo(&mut doc));
        assert!(doc.glyph("Z").is_some());
    }

    #[test]
    fn redo_is_cleared_by_a_new_edit() {
        let mut doc = doc_with_a();
        let mut h = History::default();
        h.record(&mut doc, Edit::SetAdvance { name: "A".into(), before: 500.0, after: 600.0 });
        assert!(h.undo(&mut doc));
        assert!(h.can_redo());
        h.record(&mut doc, Edit::SetLsb { name: "A".into(), before: 0.0, after: 10.0 });
        assert!(!h.can_redo());
    }

    #[test]
    fn limit_drops_oldest_entries() {
        let mut doc = doc_with_a();
        let mut h = History::new(3);
        for i in 0..10 {
            h.record(&mut doc, Edit::SetAdvance {
                name: "A".into(),
                before: 500.0,
                after: 500.0 + i as f64,
            });
        }
        assert_eq!(h.depth(), 3);
    }
}