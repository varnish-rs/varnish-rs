use std::collections::HashMap;
use std::path::{Path, PathBuf};

use proc_macro2 as pm2;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitBool, LitStr};

struct VtcTestsInput {
    glob_pattern: LitStr,
    debug: bool,
}

impl Parse for VtcTestsInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let glob_pattern: LitStr = input.parse()?;
        let debug = if input.parse::<syn::Token![,]>().is_ok() {
            input.parse::<LitBool>()?.value()
        } else {
            false
        };
        Ok(VtcTestsInput {
            glob_pattern,
            debug,
        })
    }
}

#[allow(clippy::too_many_lines)]
pub fn generate(input: pm2::TokenStream) -> pm2::TokenStream {
    let parsed: VtcTestsInput = match syn::parse2(input) {
        Ok(v) => v,
        Err(e) => return e.into_compile_error(),
    };

    let pattern_str = {
        let value = parsed.glob_pattern.value();
        let p = Path::new(&value);
        if p.is_absolute() {
            value
        } else {
            let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") else {
                return syn::Error::new_spanned(
                    &parsed.glob_pattern,
                    "CARGO_MANIFEST_DIR is not set; run_vtc_tests! must be expanded by cargo",
                )
                .into_compile_error();
            };
            PathBuf::from(manifest_dir)
                .join(p)
                .to_string_lossy()
                .into_owned()
        }
    };

    let glob_iter = match glob::glob(&pattern_str) {
        Ok(paths) => paths,
        Err(e) => {
            return syn::Error::new_spanned(
                &parsed.glob_pattern,
                format!("failed to parse glob pattern {pattern_str:?}: {e}"),
            )
            .into_compile_error();
        }
    };

    let mut matched: Vec<PathBuf> = Vec::new();
    let mut glob_errors: Vec<String> = Vec::new();
    for entry in glob_iter {
        match entry {
            Ok(p) => matched.push(p),
            Err(e) => glob_errors.push(format!("{}: {}", e.path().display(), e.error())),
        }
    }
    if !glob_errors.is_empty() {
        return syn::Error::new_spanned(
            &parsed.glob_pattern,
            format!(
                "errors while expanding glob {pattern_str:?}: {}",
                glob_errors.join("; ")
            ),
        )
        .into_compile_error();
    }
    matched.sort();

    if matched.is_empty() {
        // Mirror prior runtime behaviour without breaking the doctest in
        // `varnish/src/lib.rs`: cfg(test) is unset in rustdoc's synthetic crate.
        let msg = format!("no VTC test files matched pattern {pattern_str:?}");
        return quote! {
            #[cfg(test)]
            #[test]
            fn vtc_no_files_found() { panic!("{}", #msg); }
        };
    }

    let debug = parsed.debug;
    let mut used: HashMap<String, u32> = HashMap::new();
    let mut entries: Vec<(Ident, String)> = Vec::with_capacity(matched.len());
    for p in &matched {
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
            return syn::Error::new_spanned(
                &parsed.glob_pattern,
                format!(
                    "matched file {:?} has no valid UTF-8 file stem",
                    p.display()
                ),
            )
            .into_compile_error();
        };
        let Some(path_str) = p.to_str() else {
            return syn::Error::new_spanned(
                &parsed.glob_pattern,
                format!("matched file path {:?} is not valid UTF-8", p.display()),
            )
            .into_compile_error();
        };
        let base = sanitize_ident(stem);
        let n = used.entry(base.clone()).or_insert(0);
        let suffix = if *n == 0 {
            String::new()
        } else {
            format!("_{n}")
        };
        *n += 1;
        let ident = Ident::new(&format!("vtc_{base}{suffix}"), pm2::Span::call_site());
        entries.push((ident, path_str.to_string()));
    }

    let tests = entries.iter().map(|(ident, path)| {
        quote! {
            #[cfg(test)]
            #[test]
            fn #ident() {
                // Cargo names the dylib search path differently per platform
                // (DYLD_FALLBACK_LIBRARY_PATH on macOS, LD_LIBRARY_PATH elsewhere), so read
                // it at runtime rather than via env!() — which would fail to compile on macOS.
                let dylib_path_var = if cfg!(target_os = "macos") {
                    "DYLD_FALLBACK_LIBRARY_PATH"
                } else {
                    "LD_LIBRARY_PATH"
                };
                let dylib_path = ::std::env::var(dylib_path_var).unwrap_or_default();
                if let Err(err) = ::varnish::varnishtest::run_one_test(
                    &dylib_path,
                    env!("CARGO_PKG_NAME"),
                    ::std::path::Path::new(#path),
                    option_env!("VARNISHTEST_DURATION").unwrap_or("10s"),
                    #debug,
                ) {
                    panic!("{err}");
                }
            }
        }
    });

    // `include_bytes!` forces cargo to re-expand the macro when a matched file
    // is edited. Adding a new file still requires touching the invoking source.
    let anchors = entries.iter().enumerate().map(|(i, (_, path))| {
        let ident = Ident::new(&format!("_VTC_REBUILD_ANCHOR_{i}"), pm2::Span::call_site());
        quote! {
            #[cfg(test)]
            #[allow(dead_code)]
            const #ident: &[u8] = include_bytes!(#path);
        }
    });

    quote! {
        #(#anchors)*
        #(#tests)*
    }
}

fn sanitize_ident(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::sanitize_ident;

    #[test]
    fn alphanumeric_unchanged() {
        assert_eq!(sanitize_ident("hello_world"), "hello_world");
        assert_eq!(sanitize_ident("abc123"), "abc123");
        assert_eq!(sanitize_ident("ABC"), "ABC");
    }

    #[test]
    fn dashes_become_underscores() {
        assert_eq!(sanitize_ident("has-dashes"), "has_dashes");
        assert_eq!(sanitize_ident("a-b-c"), "a_b_c");
    }

    #[test]
    fn dots_become_underscores() {
        assert_eq!(sanitize_ident("foo.bar"), "foo_bar");
    }

    #[test]
    fn spaces_become_underscores() {
        assert_eq!(sanitize_ident("foo bar"), "foo_bar");
    }

    #[test]
    fn numeric_prefix_gets_leading_underscore() {
        assert_eq!(sanitize_ident("01_numeric"), "_01_numeric");
        assert_eq!(sanitize_ident("123"), "_123");
        assert_eq!(sanitize_ident("9test"), "_9test");
    }

    #[test]
    fn empty_string_unchanged() {
        assert_eq!(sanitize_ident(""), "");
    }

    #[test]
    fn all_special_chars() {
        assert_eq!(sanitize_ident("---"), "___");
    }

    #[test]
    fn sanitize_ident_cases() {
        let cases = [
            "hello_world",
            "has-dashes",
            "a-b-c",
            "foo.bar",
            "foo bar",
            "01_numeric",
            "123",
            "9test",
            "",
            "---",
            "ABC",
            "abc123",
        ];
        let result = cases
            .iter()
            .map(|s| format!("{s:?} => {:?}", sanitize_ident(s)))
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!(result);
    }
}
