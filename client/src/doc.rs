//! The Automerge document model: typed accessors over the CRDT root map, and
//! the body `Text` object that carries the rich-text content.

use crate::marks::{delta_spans, mark_fingerprint, MARK_NAMES};
use crate::protocol::{DOC_KEY_BODY, DOC_KEY_VERSION};
use crate::quill::Delta;
use automerge::{transaction::Transactable, AutoCommit, ObjId, ObjType, ReadDoc};
use web_sys::console::log_1;

pub struct Doc {
    inner: AutoCommit,
}

impl Doc {
    pub fn new() -> Self {
        Self {
            inner: AutoCommit::new(),
        }
    }

    pub fn get_heads(&mut self) -> Vec<automerge::ChangeHash> {
        self.inner.get_heads()
    }

    pub fn diff(
        &mut self,
        before: &[automerge::ChangeHash],
        after: &[automerge::ChangeHash],
    ) -> Vec<automerge::Patch> {
        self.inner.diff(before, after)
    }

    pub fn merge(&mut self, other: &mut AutoCommit) -> Result<(), automerge::AutomergeError> {
        self.inner.merge(other).map(|_| ())
    }

    pub fn replace_with(&mut self, other: AutoCommit) {
        self.inner = other;
    }

    pub fn save(&mut self) -> Vec<u8> {
        self.inner.save()
    }

    fn get_scalar(&self, key: &str) -> Option<automerge::ScalarValue> {
        match self.inner.get(automerge::ROOT, key).ok().flatten()?.0 {
            automerge::Value::Scalar(s) => Some(s.into_owned()),
            automerge::Value::Object(_) => None,
        }
    }

    pub fn get_str(&self, key: &str) -> String {
        match self.get_scalar(key) {
            Some(automerge::ScalarValue::Str(s)) => s.to_string(),
            _ => String::new(),
        }
    }

    pub fn get_u64(&self, key: &str) -> u64 {
        match self.get_scalar(key) {
            Some(automerge::ScalarValue::Uint(u)) => u,
            Some(automerge::ScalarValue::Int(i)) => i as u64,
            Some(automerge::ScalarValue::Counter(c)) => u64::from(&c),
            Some(automerge::ScalarValue::F64(f)) => f as u64,
            _ => 0,
        }
    }

    /// The body field is stored as an Automerge `Text` CRDT object (not a plain
    /// scalar string), so that local/remote edits can be applied as minimal
    /// character-level splices instead of whole-document replacement.
    ///
    /// Creating variant — only call where a write is actually intended; use
    /// [`Doc::body_obj`] for reads so that merely opening the editor doesn't
    /// record a CRDT op.
    pub fn body_obj_or_create(&mut self) -> ObjId {
        if let Some(id) = self.body_obj() {
            return id;
        }
        match self
            .inner
            .put_object(automerge::ROOT, DOC_KEY_BODY, ObjType::Text)
        {
            Ok(id) => id,
            Err(e) => {
                log_1(&format!("[doc] Failed to create body object: {:?}", e).into());
                ObjId::Root
            }
        }
    }

    pub fn body_obj(&self) -> Option<ObjId> {
        match self.inner.get(automerge::ROOT, DOC_KEY_BODY) {
            Ok(Some((automerge::Value::Object(ObjType::Text), id))) => Some(id),
            _ => None,
        }
    }

    pub fn get_body(&self) -> String {
        match self.body_obj() {
            Some(id) => self.inner.text(&id).unwrap_or_default(),
            None => String::new(),
        }
    }

    pub fn body_len(&self, body_obj: &ObjId) -> usize {
        self.inner
            .text(body_obj)
            .map(|t| t.chars().count())
            .unwrap_or(0)
    }

    pub fn marks(&self, body_obj: &ObjId) -> Vec<automerge::marks::Mark> {
        self.inner
            .marks(body_obj)
            .unwrap_or_default()
            .into_iter()
            .filter(|m| MARK_NAMES.contains(&m.name()))
            .collect()
    }

    /// The document version is an Automerge `Counter`, not a read-then-put on
    /// a last-write-wins register: concurrent bumps from two clients must both
    /// survive the merge rather than one silently overwriting the other.
    pub fn bump_version(&mut self) {
        if !matches!(
            self.get_scalar(DOC_KEY_VERSION),
            Some(automerge::ScalarValue::Counter(_))
        ) {
            // Seed (or migrate a legacy Uint) as a counter starting from the
            // value already recorded, so the pill doesn't jump backwards.
            let current = self.get_u64(DOC_KEY_VERSION);
            if let Err(e) = self.inner.put(
                automerge::ROOT,
                DOC_KEY_VERSION,
                automerge::ScalarValue::counter(current as i64),
            ) {
                log_1(&format!("[doc] Failed to seed version counter: {:?}", e).into());
                return;
            }
        }
        if let Err(e) = self.inner.increment(automerge::ROOT, DOC_KEY_VERSION, 1) {
            log_1(&format!("[doc] Failed to bump version: {:?}", e).into());
        }
    }

    /// Set a scalar document field. Returns whether the field actually changed;
    /// Automerge records an op even for a no-op `put`, so the equality guard is
    /// what keeps idle re-renders from bumping the version.
    pub fn set_field(&mut self, key: &'static str, value: String) -> bool {
        if self.get_str(key) == value {
            return false;
        }
        if let Err(e) = self.inner.put(automerge::ROOT, key, value) {
            log_1(&format!("[doc] Failed to set {}: {:?}", key, e).into());
            return false;
        }
        self.bump_version();
        true
    }

    pub fn update_text(&mut self, body_obj: &ObjId, text: &str) {
        if let Err(e) = self.inner.update_text(body_obj, text) {
            log_1(&format!("[doc] Failed to update body text: {:?}", e).into());
        }
    }

    /// Compares the formatting implied by a Quill Delta against the CRDT's
    /// current marks, per character rather than by raw span boundaries —
    /// Quill may split a run of identically-formatted text into several
    /// Delta ops that don't line up with Automerge's coalesced mark spans,
    /// so comparing spans directly would report spurious differences.
    pub fn marks_differ_from_delta(
        &self,
        body_obj: &ObjId,
        delta: &Delta,
        text_len: usize,
    ) -> bool {
        let desired = delta_spans(delta);
        let current: Vec<_> = self
            .marks(body_obj)
            .into_iter()
            .map(|m| (m.name().to_string(), m.value().to_owned(), m.start, m.end))
            .collect();
        mark_fingerprint(text_len, &desired) != mark_fingerprint(text_len, &current)
    }

    /// Replace all formatting marks on the body text with the spans implied
    /// by a Quill Delta's `attributes`. Marks don't change text length or
    /// position, so this never disturbs cursor placement.
    pub fn rewrite_marks(&mut self, body_obj: &ObjId, delta: &Delta) {
        let len = self.body_len(body_obj);
        for name in MARK_NAMES {
            let _ = self
                .inner
                .unmark(body_obj, name, 0, len, automerge::marks::ExpandMark::Both);
        }
        for (name, value, start, end) in delta_spans(delta) {
            let mark = automerge::marks::Mark::new(name, value, start, end);
            let _ = self
                .inner
                .mark(body_obj, mark, automerge::marks::ExpandMark::None);
        }
    }
}
