#[macro_export]
macro_rules! hash_of {
    ($x:expr) => {{
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        $x.hash(&mut h);
        h.finish()
    }};
}
