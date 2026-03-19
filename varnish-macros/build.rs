fn main() {
    println!("cargo::rustc-check-cfg=cfg(varnishsys_77_vmod_data)");
    // Only 8.x and trunk supported
    let ver = std::env::var("DEP_VARNISHAPI_VERSION_NUMBER")
        .expect("DEP_VARNISHAPI_VERSION_NUMBER not set");
    if ver == "trunk" {
        // Treat trunk as latest Varnish
        // Add any config options for latest Varnish here
        // If you add new cfgs for 8.x, add them here too
        println!("cargo::rustc-cfg=varnishsys_77_vmod_data");
        println!("cargo::rustc-env=VARNISHAPI_VERSION_NUMBER=trunk");
        return;
    }
    let (major, minor, patch) = parse_version(&ver);
    println!("cargo::rustc-env=VARNISHAPI_VERSION_NUMBER={major}.{minor}.{patch}");

    if (major == 7 && minor >= 7) || major >= 8 {
        println!("cargo::rustc-cfg=varnishsys_77_vmod_data");
    }
}

fn parse_version(version: &str) -> (u32, u32, u32) {
    // version string usually looks like "8.0.0"
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
