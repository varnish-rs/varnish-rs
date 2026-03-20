fn main() {
    println!("cargo::rustc-check-cfg=cfg(varnishsys_77_vmod_data)");
    // Only 8.0+ and trunk supported
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
    let ver = semver::Version::parse(&ver)
        .unwrap_or_else(|_| panic!("DEP_VARNISHAPI_VERSION_NUMBER is not a valid semver: {ver}"));
    println!(
        "cargo::rustc-env=VARNISHAPI_VERSION_NUMBER={}.{}.{}",
        ver.major, ver.minor, ver.patch
    );

    if ver >= semver::Version::new(7, 7, 0) {
        println!("cargo::rustc-cfg=varnishsys_77_vmod_data");
    }
}
