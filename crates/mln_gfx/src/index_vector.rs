// Ports src/mln/gfx/index_vector.hpp

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use crate::draw_mode::DrawMode;

/// `gfx::IndexBufferBase`: backend buffer attached to a shared index vector.
pub trait IndexBufferBase: Any {
    fn as_any(&self) -> &dyn Any;
}

/// `gfx::IndexVectorBase`. `IndexVector<DrawMode>` only adds a grouped
/// `emplace_back`, see `emplace_back_group`.
#[derive(Default)]
pub struct IndexVectorBase {
    v: Vec<u16>,
    buffer: Option<Box<dyn IndexBufferBase>>,
    dirty: bool,
    released: bool,
}

/// `std::shared_ptr<IndexVectorBase>`; the upload pass attaches the buffer
/// through the shared pointer, hence the `RefCell`.
pub type IndexVectorBasePtr = Rc<RefCell<IndexVectorBase>>;

impl IndexVectorBase {
    pub fn new(indexes: Vec<u16>) -> Self {
        IndexVectorBase { v: indexes, buffer: None, dirty: true, released: false }
    }

    /// The C++ copy constructor: the buffer is not copied.
    pub fn copy(&self) -> Self {
        IndexVectorBase { v: self.v.clone(), buffer: None, dirty: true, released: false }
    }

    pub fn get_buffer(&self) -> Option<&dyn IndexBufferBase> {
        self.buffer.as_deref()
    }
    pub fn set_buffer(&mut self, value: Option<Box<dyn IndexBufferBase>>) {
        self.buffer = value;
    }
    pub fn get_dirty(&self) -> bool {
        self.dirty
    }
    pub fn set_dirty(&mut self, value: bool) {
        self.dirty = value;
    }
    pub fn is_released(&self) -> bool {
        self.released
    }
    pub fn reserve(&mut self, count: usize) {
        self.v.reserve(count);
    }
    pub fn extend(&mut self, n: usize, val: u16) {
        debug_assert!(!self.released);
        self.v.resize(self.v.len() + n, val);
        self.dirty = true;
    }
    pub fn at_mut(&mut self, n: usize) -> &mut u16 {
        debug_assert!(!self.released);
        self.dirty = true;
        &mut self.v[n]
    }
    pub fn at(&self, n: usize) -> u16 {
        self.v[n]
    }
    pub fn elements(&self) -> usize {
        self.v.len()
    }
    pub fn bytes(&self) -> usize {
        self.v.len() * std::mem::size_of::<u16>()
    }
    pub fn empty(&self) -> bool {
        self.v.is_empty()
    }
    pub fn clear(&mut self) {
        self.dirty = true;
        self.v.clear();
        self.buffer = None;
    }
    /// Indicate that this shared index vector will no longer be updated.
    pub fn release(&mut self) {
        // If we've already created a buffer, we don't need the raw data any more.
        if self.buffer.is_some() {
            self.v.clear();
        }
        self.released = true;
    }
    pub fn data(&self) -> &[u16] {
        &self.v
    }
    pub fn vector(&self) -> &Vec<u16> {
        &self.v
    }

    /// `IndexVector<DrawMode>::emplace_back(args...)`: the argument count must
    /// be a multiple of the draw mode's group size.
    pub fn emplace_back_group(&mut self, mode: DrawMode, indexes: &[u16]) {
        debug_assert!(indexes.len() % mode.buffer_group_size() == 0, "wrong buffer element count");
        debug_assert!(!self.released);
        self.v.extend_from_slice(indexes);
        self.dirty = true;
    }
}
