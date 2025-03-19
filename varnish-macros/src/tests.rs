use std::fs::read_to_string;
use std::path::PathBuf;

use insta::{assert_snapshot, with_settings};
use proc_macro2::TokenStream;
use quote::quote;
use regex::Regex;
use syn::ItemMod;

use crate::gen_docs::gen_doc_content;
use crate::generator::render_model;
use crate::parser::tokens_to_model;
use crate::parser_utils::remove_attr;

/// Read content of the ../../varnish/tests/pass directory that should also pass full compilation tests,
/// parse them and create snapshots of the model and the generated code.
#[test]
fn parse_pass_tests() {
    run_parse_tests("../varnish/tests/pass/*.rs");
}

#[test]
fn parse_pass_ffi_tests() {
    run_parse_tests("../varnish/tests/pass_ffi/*.rs");
}

fn run_parse_tests(path: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    for file in glob::glob(path.to_str().unwrap()).unwrap() {
        let filepath = file.unwrap();
        let file = filepath.to_str().unwrap();
        eprintln!("Processing file: {file}");
        let src = read_to_string(&filepath).expect("unable to read file");
        let syntax = syn::parse_file(&src).expect("unable to parse file");
        let mut has_vmod = false;
        for item in syntax.items {
            if let syn::Item::Mod(mut item) = item {
                assert!(!has_vmod, "Multiple vmod modules found in file {file}");
                has_vmod = true;
                // FIXME: use this attribute as an arg for the test
                let _arg = remove_attr(&mut item.attrs, "vmod").unwrap();
                let name = format!(
                    "{}_{}",
                    filepath.file_stem().unwrap().to_string_lossy(),
                    item.ident
                );
                test(&name, quote! {}, item);
                // FIXME: pass proper attribute info
                // test(&name, quote! { #arg }, item);
            }
        }
        assert!(has_vmod, "No vmod modules found in file {file}");
    }
}

fn test(name: &str, args: TokenStream, mut item_mod: ItemMod) {
    let version = if cfg!(varnishsys_6) {
        "_v6"
    } else if cfg!(varnishsys_77_vmod_data) {
        ""
    } else {
        "_v76"
    };
    let snapshot_path = format!("../../varnish/snapshots{version}");
    with_settings!({ snapshot_path => snapshot_path, omit_expression => true, prepend_module_to_snapshot => false }, {
        let Ok(info) = tokens_to_model(args, &mut item_mod).map_err(|err| {
            // On error, save the error output as a snapshot and return early.
            let err = err.into_compile_error();
            with_settings!({ snapshot_suffix => "error" }, { assert_snapshot!(name, err) });
        }) else { return };

        with_settings!({ snapshot_suffix => "model" }, { assert_snapshot!(name, format!("{info:#?}")) });
        with_settings!({ snapshot_suffix => "docs" }, { assert_snapshot!(name, gen_doc_content(&info)) });

        let file = render_model(item_mod, &info).to_string();
        let parsed = match syn::parse_file(&file) {
            Ok(v) => v,
            Err(e) => {
                // We still save the generated code, but fail the test. Allows easier error debugging.
                with_settings!({ snapshot_suffix => "code" }, { assert_snapshot!(name, file) });
                panic!("Failed to parse generated code in test {name}: {e}");
            }
        };

        let generated = prettyplease::unparse(&parsed);

        // Use regex to remove "Varnish 7.5.0 eef25264e5ca5f96a77129308edb83ccf84cb1b1" and similar.
        // Also removes any pre-builds and other versions because we assume a double-quote at the end.
        // TODO: Once MSRV is higher than 1.80, use static LazyLock<Regex>
        let re_varnish_ver = Regex::new(r"Varnish \d+\.[-+. 0-9a-z]+").unwrap();
        let code = re_varnish_ver.replace_all(&generated, "Varnish (version) (hash)");
        with_settings!({ snapshot_suffix => "code" }, { assert_snapshot!(name, code) });

        // Extract JSON string
        let pat = "const JSON: &CStr = c\"";
        let json = if let Some(pos) = code.find(pat) {
            let json = &code[pos + pat.len()..];
            json.split("\";\n").next()
        } else {
            None
        };

        let json = &json
            .unwrap_or("")
            .replace("\\\"", "\"")
            .replace("\\u{2}", "\u{2}")
            .replace("\\u{3}", "\u{3}")
            .replace("\\\\", "\\")
            // this is a bit of a hack because the double-escaping gets somewhat incorrectly parsed
            .replace("\\n", "\n");

        with_settings!({ snapshot_suffix => "json" }, { assert_snapshot!(name, json) });
    });
}
