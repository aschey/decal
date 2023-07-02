pub(crate) trait VecExt<T> {
    fn fill_from_deinterleaved(&mut self, channels: &[Vec<T>]);
}

impl<T: Copy> VecExt<T> for Vec<T> {
    fn fill_from_deinterleaved(&mut self, deinterleaved: &[Vec<T>]) {
        let out_len = deinterleaved[0].len();
        self.clear();

        for i in 0..out_len {
            for chan in deinterleaved {
                self.push(chan[i]);
            }
        }
    }
}
