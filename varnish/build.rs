fn main() {
    println!("cargo::rustc-check-cfg=cfg(varnishsys_6)");
    println!("cargo::rustc-check-cfg=cfg(varnishsys_6_priv_free_f)");

    let ver = std::env::var("DEP_VARNISHAPI_VERSION_NUMBER")
        .expect("DEP_VARNISHAPI_VERSION_NUMBER not set");
    if ver == "trunk" {
        // Treat trunk as latest Varnish
        // Add any config options for latest Varnish here
        return;
    }
    let (major, _minor) = parse_version(&ver);
    if major < 7 {
        println!("cargo::rustc-cfg=varnishsys_6");
        println!("cargo::rustc-cfg=varnishsys_6_priv_free_f");
    }
}

fn parse_version(version: &str) -> (u32, u32) {
    // version string usually looks like "7.5.0"
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
