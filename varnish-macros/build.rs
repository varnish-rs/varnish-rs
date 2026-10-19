fn main() {
    // Only 8.0+ and trunk supported
    let ver = std::env::var("DEP_VARNISHAPI_VERSION_NUMBER")
        .expect("DEP_VARNISHAPI_VERSION_NUMBER not set");
    if ver == "trunk" {
        // Treat trunk as latest Varnish
        // Add any config options for latest Varnish here
        println!("cargo::rustc-env=VARNISHAPI_VERSION_NUMBER=trunk");
        return;
    }
    let ver = semver::Version::parse(&ver)
        .unwrap_or_else(|_| panic!("DEP_VARNISHAPI_VERSION_NUMBER is not a valid semver: {ver}"));
    println!(
        "cargo::rustc-env=VARNISHAPI_VERSION_NUMBER={}.{}.{}",
        ver.major, ver.minor, ver.patch
    );
}
