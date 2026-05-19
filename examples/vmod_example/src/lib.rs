/// An example vmod
///
/// All public functions in a module tagged with `#[varnish::vmod]` will be exported
/// as Varnish VMOD functions.  The name of the module will be the name of the VMOD.
///
/// See also <https://varnish-cache.org/docs/trunk/reference/vmod.html>
#[varnish::vmod(docs = "README.md")]
mod example {
    use std::io::Write as _;

    use varnish::ffi::VCL_STRING;
    use varnish::vcl::{VclError, Workspace};
    /// This will tell you if a number is even, isn't that odd?
    ///
    /// A simple function that returns true if the number is even, false otherwise.
    pub fn is_even(
        /// The number to check for evenness
        n: i64,
    ) -> bool {
        n % 2 == 0
    }

    /// Produce a string explaining which number you provided as argument.
    ///
    /// This function can be called without arguments, or with an integer:
    ///
    /// ```vcl
    /// set resp.http.Obvious = example.captain_obvious();
    /// set resp.http.Obvious-Number = example.captain_obvious(42);
    /// ```
    pub fn captain_obvious(
        /// Optional number to provide as argument
        opt: Option<i64>,
    ) -> String {
        // we need to first "match" to know if a number was provided, if not,
        // return a default message, otherwise, build a custom one
        match opt {
            // no need to return, we are the last expression of the function!
            None => "I was called without an argument".to_string(),
            // pattern matching FTW!
            Some(n) => format!("I was given {n} as argument"),
        }
    }

    /// Prefix a user-given string and do the concatenation directly in the
    /// workspace.
    /// Returns a VCL error if the string is empty or not UTF8, the latter
    /// being handled automatically before reaching this function.
    pub unsafe fn hello(ws: &mut Workspace, s: &str) -> Result<VCL_STRING, VclError> {
        if s.is_empty() {
            return Err("hello: empty string".into());
        }
        let mut buf = ws.vcl_string_builder()?;
        write!(buf, "Hello, {s}").map_err(|e| VclError::new(e.to_string()))?;
        Ok(buf.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::example::*;

    // run all VTC tests
    varnish::run_vtc_tests!("tests/*.vtc");

    #[test]
    fn even_test() {
        assert!(is_even(0));
        assert!(is_even(1024));
        assert!(!is_even(421_321));
    }

    #[test]
    fn obviousness() {
        assert_eq!("I was called without an argument", captain_obvious(None));
        assert_eq!(
            "I was given 975322 as argument",
            captain_obvious(Some(975_322))
        );
    }
}
