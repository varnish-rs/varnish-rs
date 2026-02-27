use std::fs::read_to_string;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use insta::{assert_snapshot, with_settings};
use proc_macro2::TokenStream;
use quote::quote;
use regex::{Captures, Regex};
use syn::{Data, DeriveInput, ItemMod, ItemStruct};

use crate::gen_docs::gen_doc_content;
use crate::generator::render_model;
use crate::metrics::derive_vsc_metric;
use crate::parser::tokens_to_model;
use crate::parser_utils::remove_attr;

static RE_VARNISH_VER: LazyLock<Regex> = LazyLock::new(|| {
    // Use regex to remove "Varnish 7.5.0 eef25264e5ca5f96a77129308edb83ccf84cb1b1" and similar.
    // Also removes any pre-builds and other versions because we assume a double-quote at the end.
    Regex::new(r"Varnish (\d+\.|trunk )[-+. 0-9a-z]+").unwrap()
});

static RE_STR_BLOB: LazyLock<Regex> = LazyLock::new(|| {
    // Use regex to remove "const JSON: &CStr = c\"...\";"  and  "const cproto: &CStr = c\"...\";"
    Regex::new(r#"const ([a-zA-Z0-9_]+): &CStr = c"([^\n]+)";\n"#).unwrap()
});

/// Read the content of the `../../varnish/tests/pass` directory that should also pass full compilation tests,
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
    let version = env!("VARNISHAPI_VERSION_NUMBER");
    let snapshot_path = format!("../../varnish/snapshots{version}");
    with_settings!({ snapshot_path => snapshot_path, omit_expression => true, prepend_module_to_snapshot => false }, {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
        for file in glob::glob(path.to_str().unwrap()).unwrap() {
            test_file(&file.unwrap());
        }
    });
}

fn test_file(filepath: &Path) {
    let file = filepath.to_str().unwrap();
    eprintln!("Processing file: {file}");
    let src = read_to_string(filepath).expect("unable to read file");
    let syntax = syn::parse_file(&src).expect("unable to parse file");
    let mut has_vmod = false;
    let mut has_metrics = false;
    for item in syntax.items {
        if let syn::Item::Mod(mut item) = item {
            assert!(!has_vmod, "Multiple vmod modules found in {file}");
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
        } else if let syn::Item::Struct(item) = item {
            // Check if this has #[derive(VscMetric)]
            for attr in &item.attrs {
                // Check for derive attribute
                if let syn::Meta::List(meta_list) = &attr.meta {
                    if meta_list.path.is_ident("derive") {
                        let tokens = meta_list.tokens.to_string();
                        if tokens.contains("VscMetric") {
                            assert!(!has_metrics, "Multiple VscMetric structs found in {file}");
                            has_metrics = true;
                            let name = format!(
                                "{}_{}",
                                filepath.file_stem().unwrap().to_string_lossy(),
                                item.ident
                            );
                            test_metric(&name, item);
                            break;
                        }
                    }
                }
            }
        }
    }
    assert!(
        has_vmod || has_metrics,
        "No vmod modules or derive(VscMetric) found in file {file}"
    );
}

fn test_metric(name: &str, item: ItemStruct) {
    let ItemStruct {
        attrs,
        vis,
        struct_token,
        ident,
        generics,
        fields,
        semi_token,
    } = item;

    let input = DeriveInput {
        attrs,
        vis,
        ident,
        generics,
        data: Data::Struct(syn::DataStruct {
            struct_token,
            fields,
            semi_token,
        }),
    };

    let Ok(code) = derive_vsc_metric(&input).map_err(|err| {
        // On error, save the error output as a snapshot and return early.
        let err = err.into_compile_error();
        with_settings!({ snapshot_suffix => "error" }, { assert_snapshot!(name, err) });
    }) else {
        return;
    };
    let code = parse_gen_code(name, &code.to_string());
    with_settings!({ snapshot_suffix => "code" }, { assert_snapshot!(name, code) });
}

fn test(name: &str, args: TokenStream, mut item_mod: ItemMod) {
    let Ok(info) = tokens_to_model(args, &mut item_mod).map_err(|err| {
        // On error, save the error output as a snapshot and return early.
        let err = err.into_compile_error();
        with_settings!({ snapshot_suffix => "error" }, { assert_snapshot!(name, err) });
    }) else {
        return;
    };

    with_settings!({ snapshot_suffix => "model" }, { assert_snapshot!(name, format!("{info:#?}")) });
    with_settings!({ snapshot_suffix => "docs" }, { assert_snapshot!(name, gen_doc_content(&info)) });

    let file = render_model(item_mod, &info).to_string();
    let generated = parse_gen_code(name, &file);

    let mut code = RE_VARNISH_VER
        .replace_all(&generated, "Varnish (version) (hash)")
        .to_string();

    // Extract a JSON and cproto strings, and remove them from the original to avoid recording it twice
    let replacement = |caps: &Captures| -> String {
        let func = caps.get(1).unwrap().as_str().to_string();
        let value = caps.get(2).unwrap().as_str().to_string();

        let value = &value
            .replace("\\\"", "\"")
            .replace("\\u{2}", "\u{2}")
            .replace("\\u{3}", "\u{3}")
            .replace("\\\\", "\\")
            // this is a bit of a hack because the double-escaping gets somewhat incorrectly parsed
            .replace("\\n", "\n");

        let func_lower = func.to_lowercase();
        with_settings!({ snapshot_suffix => &func_lower }, { assert_snapshot!(name, value) });

        format!("const {func}: &CStr = c\"(moved to @{func_lower}.snap files)\";\n")
    };

    code = RE_STR_BLOB
        .replace_all(code.as_str(), &replacement)
        .to_string();

    with_settings!({ snapshot_suffix => "code" }, { assert_snapshot!(name, code) });
}

fn parse_gen_code(name: &str, file: &str) -> String {
    let parsed = match syn::parse_file(file) {
        Ok(v) => v,
        Err(e) => {
            // We still save the generated code, but fail the test. Allows easier error debugging.
            with_settings!({ snapshot_suffix => "code" }, { assert_snapshot!(name, file) });
            panic!("Failed to parse generated code in test {name}: {e}");
        }
    };

    prettyplease::unparse(&parsed)
}
