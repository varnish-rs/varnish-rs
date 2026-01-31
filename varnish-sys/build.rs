use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::{env, fs};

use bindgen_helpers as bindgen;
use bindgen_helpers::{rename_enum, Renamer};

static BINDINGS_FILE: &str = "bindings.for-docs";
static BINDINGS_FILE_VER: &str = "7.7.1";

struct VarnishInfo {
    bindings: PathBuf,
    varnish_paths: Vec<PathBuf>,
    version: String,
    defines: Vec<&'static str>,
}

impl VarnishInfo {
    fn parse(bindings: PathBuf, varnish_paths: Vec<PathBuf>, version: String) -> Self {
        let (major, _minor) = parse_version(&version);

        // >= 8.0
        if major >= 8 {
            println!("cargo::rustc-cfg=varnishsys_80_io_vdp");
        }

        if major < 7 {
            println!("cargo::rustc-cfg=varnishsys_6");
        }

        if major < 6 || major > 8 {
            println!(
                "cargo::warning=Varnish {version} is not supported and may not work with this crate"
            );
        }

        let mut defines = vec![];
        if major < 7 {
            defines.push("VARNISH_RS_6_0");
        } else {
            defines.push("VARNISH_RS_HTTP_CONN");
        }

        // TODO: This should become conditional once this PR merges, and we know its version
        //     https://github.com/varnishcache/varnish-cache/pull/4303 merges
        defines.push("VARNISH_RS_ALLOC_VARIADIC");

        Self {
            bindings,
            varnish_paths,
            version,
            defines,
        }
    }
}

impl Display for VarnishInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.version)
    }
}

fn main() {
    if let Some(info) = &detect_varnish() {
        generate_bindings(info);
        build_c_wrapper(info);
    }
}

fn detect_varnish() -> Option<VarnishInfo> {
    // All varnishsys_* flags are used to enable some features that are not available in all versions.
    // The crate must compile for the latest supported version with none of these flags enabled.
    // By convention, the version number is the last version where the feature was available.

    // 8.0 adds a few fields to the vdp struct
    println!("cargo::rustc-check-cfg=cfg(varnishsys_80_io_vdp)");
    // 6.0 support
    println!("cargo::rustc-check-cfg=cfg(varnishsys_6)");

    let bindings = PathBuf::from(env::var("OUT_DIR").unwrap()).join("bindings.rs");

    println!("cargo:rerun-if-env-changed=VARNISH_INCLUDE_PATHS");
    let (varnish_paths, version) = find_include_dir(&bindings)?;

    println!("cargo::metadata=version_number={version}");

    Some(VarnishInfo::parse(bindings, varnish_paths, version))
}

fn generate_bindings(info: &VarnishInfo) {
    let mut ren = Renamer::default();
    rename_enum!(ren, "VSL_tag_e" => "VslTag", remove: "SLT_"); // SLT_Debug
    rename_enum!(ren, "boc_state_e" => "BocState", remove: "BOS_"); // BOS_INVALID
    rename_enum!(ren, "director_state_e" => "DirectorState", remove: "DIR_S_", "HDRS" => "Headers"); // DIR_S_NULL
    rename_enum!(ren, "gethdr_e" => "GetHeader", remove: "HDR_"); // HDR_REQ_TOP
    rename_enum!(ren, "sess_attr" => "SessionAttr", remove: "SA_"); // SA_TRANSPORT
    rename_enum!(ren, "lbody_e" => "Body", remove: "LBODY_"); // LBODY_SET_STRING
    rename_enum!(ren, "task_prio" => "TaskPriority", remove: "TASK_QUEUE_"); // TASK_QUEUE_BO
    rename_enum!(ren, "vas_e" => "Vas", remove: "VAS_"); // VAS_WRONG
    rename_enum!(ren, "vcl_event_e" => "VclEvent", remove: "V(CL|DI)_EVENT_"); // VCL_EVENT_LOAD
    rename_enum!(ren, "vcl_func_call_e" => "VclFuncCall", remove: "VSUB_"); // VSUB_STATIC
    rename_enum!(ren, "vcl_func_fail_e" => "VclFuncFail", remove: "VSUB_E_"); // VSUB_E_OK
    rename_enum!(ren, "vdp_action" => "VdpAction", remove: "VDP_"); // VDP_NULL
    rename_enum!(ren, "vfp_status" => "VfpStatus", remove: "VFP_"); // VFP_ERROR

    println!("cargo::rustc-link-lib=varnishapi");
    println!("cargo::rerun-if-changed=c_code/wrapper.h");
    let mut bindings_builder = bindgen::Builder::default()
        .header("c_code/wrapper.h")
        .blocklist_item("FP_.*")
        .blocklist_item("FILE")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .clang_args(
            info.varnish_paths
                .iter()
                .map(|i| format!("-I{}", i.to_str().unwrap())),
        )
        .ctypes_prefix("::std::ffi")
        .derive_copy(true)
        .derive_debug(true)
        .derive_default(true)
        .generate_cstr(true)
        //
        // These two types are set to `c_void`, which is not copyable.
        // Plus the new wrapped empty type might be pointless... or not?
        .type_alias("VCL_VOID")
        .type_alias("VCL_INSTANCE")
        .new_type_alias("VCL_.*")
        .new_type_alias("vtim_.*") // VCL_DURATION = vtim_dur = f64
        //
        // FIXME: some enums should probably be done as rustified_enum (exhaustive)
        .rustified_non_exhaustive_enum(ren.get_regex_str())
        .parse_callbacks(Box::new(ren));

    for define in &info.defines {
        bindings_builder = bindings_builder.clang_args(&["-D", define]);
    }

    let bindings = bindings_builder
        .generate()
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    bindings
        .write_to_file(&info.bindings)
        .expect("Couldn't write bindings!");

    // Compare generated file to the checked-in `bindings.for-docs` file,
    // and if they differ, raise a warning.
    let generated = fs::read_to_string(&info.bindings).unwrap();
    let checked_in = fs::read_to_string(BINDINGS_FILE).unwrap_or_default();
    if generated != checked_in {
        println!(
            "cargo::warning=Generated bindings from Varnish {info} differ from checked-in {BINDINGS_FILE}. Update with   cp {} varnish-sys/{BINDINGS_FILE}",
            info.bindings.display()
        );
    } else if BINDINGS_FILE_VER != info.version {
        println!(
            r#"cargo::warning=Generated bindings **version** from Varnish {info} differ from checked-in {BINDINGS_FILE}. Update `build.rs` file with   BINDINGS_FILE_VER = "{info}""#
        );
    }
}

fn build_c_wrapper(info: &VarnishInfo) {
    let mut builder = cc::Build::new();
    for define in &info.defines {
        builder.define(define, None);
    }
    builder
        .file("c_code/wrapper.c")
        .includes(&info.varnish_paths)
        .compile("varnish_wrapper");
}

fn find_include_dir(out_path: &PathBuf) -> Option<(Vec<PathBuf>, String)> {
    if let Ok(s) = env::var("VARNISH_INCLUDE_PATHS") {
        // FIXME: If the user has set the VARNISH_INCLUDE_PATHS environment variable, use that.
        //    At the moment we have no way to detect which version it is.
        //    vmod_abi.h  seems to have this line, which can be used in the future.
        //    #define VMOD_ABI_Version "Varnish 7.7.0 eef25264e5ca5f96a77129308edb83ccf84cb1b1"
        println!("cargo::warning=Using VARNISH_INCLUDE_PATHS='{s}' env var, and assume it is the latest supported version {BINDINGS_FILE_VER}");
        return Some((
            s.split(':').map(PathBuf::from).collect(),
            BINDINGS_FILE_VER.into(), // Assume manual version is the latest supported
        ));
    }

    let pkg = pkg_config::Config::new();
    match pkg.probe("varnishapi") {
        Ok(l) => Some((l.include_paths, l.version)),
        Err(e) => {
            // See https://docs.rs/about/builds#detecting-docsrs
            if env::var("DOCS_RS").is_ok() {
                eprintln!("libvarnish not found, using saved bindings for the doc.rs: {e}");
                fs::copy(BINDINGS_FILE, out_path).unwrap();
                println!("cargo::metadata=version_number={BINDINGS_FILE_VER}");
                None
            } else {
                // FIXME: we should give a URL describing how to install varnishapi
                // I tried to find it, but failed to find a clear URL for this.
                panic!("pkg_config failed to find varnishapi, make sure it is installed: {e:?}");
            }
        }
    }
}

fn parse_version(version: &str) -> (u32, u32) {
    // version string usually looks like "7.7.0"
    let mut parts = version.split('.');
    (
        parse_next_int(&mut parts, "major"),
        parse_next_int(&mut parts, "minor"),
    )
}

fn parse_next_int(parts: &mut std::str::Split<char>, name: &str) -> u32 {
    let val = parts
        .next()
        .unwrap_or_else(|| panic!("varnishapi invalid version {name}"));
    val.parse::<u32>()
        .unwrap_or_else(|_| panic!("varnishapi invalid version - {name} value is '{val}'"))
}
