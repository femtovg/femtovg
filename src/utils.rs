// TODO: Remove this when drain_filter comes to stable rust

pub(crate) trait VecRetainMut<T> {
    fn retain_mut<F>(&mut self, f: F)
    where
        F: FnMut(&mut T) -> bool;
}

impl<T> VecRetainMut<T> for Vec<T> {
    // Adapted from libcollections/vec.rs in Rust
    // Primary author in Rust: Michael Darakananda
    fn retain_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut T) -> bool,
    {
        let len = self.len();
        let mut del = 0;
        {
            let v = &mut **self;

            for i in 0..len {
                if !f(&mut v[i]) {
                    del += 1;
                } else if del > 0 {
                    v.swap(i - del, i);
                }
            }
        }
        if del > 0 {
            self.truncate(len - del);
        }
    }
}
