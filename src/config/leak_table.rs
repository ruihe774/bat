use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

const TABLE_SIZE: usize = 1021; // have to be a prime number
static TABLE: Mutex<[Option<&str>; TABLE_SIZE]> = Mutex::new([None; TABLE_SIZE]);

pub fn leak_string(s: String) -> &'static str {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    let digest = hasher.finish();
    #[allow(clippy::cast_possible_truncation)]
    let idx = (digest % (TABLE_SIZE as u64)) as usize;

    let mut table = TABLE.lock().unwrap();
    match &mut table.as_mut()[idx] {
        Some(r) if *r == s => r,
        p => p.insert(s.leak()),
    }
}
