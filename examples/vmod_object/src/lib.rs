use dashmap::DashMap;

varnish::run_vtc_tests!("tests/*.vtc");

/// kv only contains one element: a String->String hashmap that can be used in parallel
#[allow(non_camel_case_types)]
pub struct kv {
    storage: DashMap<String, String>,
}

/// A simple string dictionary in your VCL
#[varnish::vmod(docs = "README.md")]
mod object {
    use dashmap::DashMap;

    use super::kv;

    // implementation needs the same methods as defined in the vcc, plus "new()"
    // corresponding to the constructor, which requires the context (_ctx) , and the
    // name of the object in VLC (_vcl_name)
    impl kv {
        /// Create a new key-value store, with an optional capacity.
        /// If `cap` is 0 or less, it will be ignored.
        pub fn new(cap: Option<i64>) -> Self {
            // depending on whether cap was actually passed, and on its value,
            // call a different constructor
            let storage = match cap {
                None => DashMap::new(),
                Some(n) if n <= 0 => DashMap::new(),
                Some(n) => DashMap::with_capacity(n as usize),
            };

            Self { storage }
        }

        /// Retrieve the value associated `key`, or an empty string if `key` didn't exist.
        ///
        /// There is currently an inefficiency in the implementation: when found, the value
        /// is a `&str`, so it must be converted to a `String` to be returned, which is then
        /// copied into an internal Varnish workspace. In the future, we will provide a way
        /// to avoid this double-copy.
        pub fn get(&self, key: &str) -> String {
            self.storage // access our member field
                .get(key) // look for key
                // If not found, create a new empty string (no memory allocation, so can use without lambda)
                // If found, convert the &str to a String
                .map_or(String::new(), |v| v.value().clone())
        }

        /// Insert a key-value pair into the store.
        ///
        /// Note that varnish-accessible functions use readonly `&self`,
        /// so the interior mutability pattern should be used to store data.
        pub fn set(&self, key: &str, value: &str) {
            self.storage.insert(key.to_owned(), value.to_owned());
        }
    }
}
