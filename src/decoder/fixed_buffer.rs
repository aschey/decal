pub(crate) struct FixedBuffer<T> {
    inner: Vec<T>,
    position: usize,
}

impl<T> FixedBuffer<T>
where
    T: Copy,
{
    pub(crate) fn new(default_val: T, n: usize) -> Self {
        Self {
            inner: vec![default_val; n],
            position: 0,
        }
    }

    pub(crate) fn append_from_slice(&mut self, src: &[T]) {
        let len = src.len();
        self.inner[self.position..self.position + len].copy_from_slice(src);
        self.position += len;
    }

    pub(crate) fn reset(&mut self) {
        self.position = 0;
    }

    pub(crate) fn position(&self) -> usize {
        self.position
    }

    pub(crate) fn remaining(&self) -> usize {
        self.inner.len() - self.position
    }

    pub(crate) fn inner(&self) -> &[T] {
        &self.inner
    }
}
