use symphonia::core::sample::Sample;

pub(crate) struct ChannelBuffer<T> {
    inner: Vec<Vec<T>>,
    capacity: usize,
    channels: usize,
    current_chan: usize,
}

impl<T: Sample + Clone> ChannelBuffer<T> {
    pub(crate) fn new(capacity: usize, channels: usize) -> Self {
        Self {
            inner: vec![Vec::with_capacity(capacity); channels],
            capacity,
            channels,
            current_chan: 0,
        }
    }

    fn len(&self) -> usize {
        self.inner[self.channels - 1].len()
    }

    fn last(&self) -> &Vec<T> {
        self.inner.last().expect("Vec should not be empty")
    }

    fn next_chan(&mut self) -> &mut Vec<T> {
        let chan = &mut self.inner[self.current_chan];
        self.current_chan = (self.current_chan + 1) % self.channels;
        chan
    }

    pub(crate) fn position(&self) -> usize {
        self.last().len()
    }

    pub(crate) fn is_full(&self) -> bool {
        self.last().len() == self.last().capacity()
    }

    pub(crate) fn reset(&mut self) {
        for chan in &mut self.inner {
            chan.clear();
        }
        self.current_chan = 0;
    }

    pub(crate) fn inner(&self) -> &[Vec<T>] {
        &self.inner
    }

    pub(crate) fn silence_remainder(&mut self) {
        while self.len() < self.capacity {
            self.next_chan().push(T::MID);
        }
    }

    pub(crate) fn fill_from_slice(&mut self, data: &[T]) -> usize {
        let mut i = 0;
        while self.len() < self.capacity && i < data.len() {
            self.next_chan().push(data[i]);
            i += 1;
        }
        i
    }
}
