use std::env;
use std::path::PathBuf;

fn main() {
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("bindings.rs");

    println!("cargo:rerun-if-env-changed=VARNISH_INCLUDE_PATHS");
    let varnish_paths: Vec<PathBuf> = match env::var("VARNISH_INCLUDE_PATHS") {
        Ok(s) => s.split(':').map(PathBuf::from).collect(),
        Err(_) => {
            match pkg_config::Config::new()
                .atleast_version("7.5")
                .probe("varnishapi")
            {
                Ok(l) => l.include_paths,
                Err(e) => {
                    // FIXME: we should only do this with `docsrs` feature - it is pointless otherwise
                    println!("no system libvarnish found, using the pre-generated bindings {e}");
                    std::fs::copy("src/bindings.rs.saved", out_path).unwrap();
                    return;
                }
            }
        }
    };

    // Only link to varnishapi if we found it
    println!("cargo:rustc-link-lib=varnishapi");
    println!("cargo:rerun-if-changed=src/wrapper.h");
    let bindings = bindgen::Builder::default()
        .header("src/wrapper.h")
        .blocklist_item("FP_.*")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .clang_args(
            varnish_paths
                .iter()
                .map(|i| format!("-I{}", i.to_str().unwrap())),
        )
        .ctypes_prefix("::std::ffi")
        .derive_copy(true)
        .derive_debug(true)
        .derive_default(true)
        .generate_cstr(true)
        //
        // .new_type_alias("VCL_ACL") // *const vrt_acl
        // .new_type_alias("VCL_BACKEND") // *const director
        // .new_type_alias("VCL_BLOB") // *const vrt_blob
        // .new_type_alias("VCL_BODY") // *const ::std::os::raw::c_void
        .new_type_alias("VCL_BOOL") // ::std::os::raw::c_uint
        // .new_type_alias("VCL_BYTES") // i64
        // .new_type_alias("VCL_DURATION") // vtim_dur
        // .new_type_alias("VCL_ENUM") // *const ::std::os::raw::c_char
        // .new_type_alias("VCL_HEADER") // *const gethdr_s
        // .new_type_alias("VCL_HTTP") // *mut http
        // .new_type_alias("VCL_INSTANCE") // ::std::os::raw::c_void
        .new_type_alias("VCL_INT") // i64
        // .new_type_alias("VCL_IP") // *const suckaddr
        // .new_type_alias("VCL_PROBE") // *const vrt_backend_probe
        .new_type_alias("VCL_REAL") // f64
        // .new_type_alias("VCL_REGEX") // *const vre
        // .new_type_alias("VCL_STEVEDORE") // *const stevedore
        // .new_type_alias("VCL_STRANDS") // *const strands
        // .new_type_alias("VCL_STRING") // *const ::std::os::raw::c_char
        // .new_type_alias("VCL_SUB") // *const vcl_sub
        // .new_type_alias("VCL_TIME") // vtim_real
        // .new_type_alias("VCL_VCL") // *mut vcl
        // .new_type_alias("VCL_VOID") // ::std::os::raw::c_void
        //
        .generate()
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    bindings
        .write_to_file(out_path)
        .expect("Couldn't write bindings!");
}
