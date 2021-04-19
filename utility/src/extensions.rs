pub trait VecExt<T> {
    fn sort_unstable_by_key_ref<F, K>(&mut self, key: F)
    where
        F: Fn(&T) -> &K,
        K: ?Sized + Ord;
}

impl<T> VecExt<T> for Vec<T> {
    fn sort_unstable_by_key_ref<F, K>(&mut self, key: F)
    where
        F: Fn(&T) -> &K,
        K: ?Sized + Ord,
    {
        self.sort_unstable_by(|x, y| key(x).cmp(key(y)));
    }
}
