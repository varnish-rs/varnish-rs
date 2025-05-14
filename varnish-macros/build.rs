fn main() {
    println!("cargo::rustc-check-cfg=cfg(varnishsys_6)");
    println!("cargo::rustc-check-cfg=cfg(varnishsys_6_priv_free_f)");
    println!("cargo::rustc-check-cfg=cfg(varnishsys_77_vmod_data)");
    println!("cargo::rustc-check-cfg=cfg(varnishsys_vmod_meta_1_0)");

    let ver = std::env::var("DEP_VARNISHAPI_VERSION_NUMBER");
    let (major, minor, patch) = parse_version(&ver.expect("DEP_VARNISHAPI_VERSION_NUMBER not set"));
    println!("cargo::rustc-env=VARNISHAPI_VERSION_NUMBER={major}.{minor}.{patch}");

    if major < 7 {
        println!("cargo::rustc-cfg=varnishsys_6");
        println!("cargo::rustc-cfg=varnishsys_6_priv_free_f");
    } else if major >= 7 && minor >= 7 {
        println!("cargo::rustc-cfg=varnishsys_77_vmod_data");
    }

    if major <= 6
        || (major == 7 && minor == 7 && patch < 1)
        || (major == 7 && minor == 6 && patch < 3)
        || (major == 7 && minor <= 5)
    {
        println!("cargo::rustc-cfg=varnishsys_vmod_meta_1_0");
    }
}

fn parse_version(version: &str) -> (u32, u32, u32) {
    // version string usually looks like "7.5.0"
    let mut parts = version.split('.');
    (
        parse_next_int(&mut parts, "major"),
        parse_next_int(&mut parts, "minor"),
        parse_next_int(&mut parts, "patch"),
    )
}

fn parse_next_int(parts: &mut std::str::Split<char>, name: &str) -> u32 {
    let val = parts
        .next()
        .unwrap_or_else(|| panic!("varnishapi invalid version {name}"));
    val.parse::<u32>()
        .unwrap_or_else(|_| panic!("varnishapi invalid version - {name} value is '{val}'"))
}
