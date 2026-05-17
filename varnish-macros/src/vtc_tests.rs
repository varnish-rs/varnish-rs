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
            let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
                .expect("CARGO_MANIFEST_DIR set by cargo during macro expansion");
            PathBuf::from(manifest_dir)
                .join(p)
                .to_string_lossy()
                .into_owned()
        }
    };

    let mut matched: Vec<PathBuf> = match glob::glob(&pattern_str) {
        Ok(paths) => paths.filter_map(Result::ok).collect(),
        Err(e) => {
            return syn::Error::new_spanned(
                &parsed.glob_pattern,
                format!("failed to parse glob pattern {pattern_str:?}: {e}"),
            )
            .into_compile_error();
        }
    };
    matched.sort();

    if matched.is_empty() {
        // Mirror prior runtime behaviour without breaking the doctest in
        // `varnish/src/lib.rs`: cfg(test) is unset in rustdoc's synthetic crate.
        let msg = format!("no VTC test files matched pattern {pattern_str:?}");
        return quote! {
            #[cfg(test)]
            #[test]
            fn vtc_no_files_found() { panic!(#msg); }
        };
    }

    let debug = parsed.debug;
    let mut used: HashMap<String, u32> = HashMap::new();
    let entries: Vec<(Ident, String)> = matched
        .iter()
        .map(|p| {
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap();
            let base = sanitize_ident(stem);
            let n = used.entry(base.clone()).or_insert(0);
            let suffix = if *n == 0 {
                String::new()
            } else {
                format!("_{n}")
            };
            *n += 1;
            let ident = Ident::new(&format!("vtc_{base}{suffix}"), pm2::Span::call_site());
            (ident, p.to_string_lossy().into_owned())
        })
        .collect();

    let tests = entries.iter().map(|(ident, path)| {
        quote! {
            #[cfg(test)]
            #[test]
            fn #ident() {
                if let Err(err) = ::varnish::varnishtest::run_one_test(
                    env!("CARGO_PKG_NAME"),
                    env!("LD_LIBRARY_PATH"),
                    ::std::path::Path::new(#path),
                    option_env!("VARNISHTEST_DURATION").unwrap_or("5s"),
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
