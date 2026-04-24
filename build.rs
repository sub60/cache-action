#[cfg(not(any(feature = "noop-cache", feature = "sub60-cache")))]
const AT_LEAST_ONE_CACHE_ENABLED_CHECK: () = compile_error!(
    "You must enable one of the features: noop-cache, sub60-cache"
);

#[expect(clippy::non_minimal_cfg)]
#[rustfmt::skip]
#[cfg(any(
    all(feature = "noop-cache", any(feature = "sub60-cache")),
))]
const NO_MORE_THAN_ONE_CACHE_ENABLED_CHECK: () = compile_error!(
    "You can only enable one of the features: noop-cache, sub60-cache"
);

fn main() {
    pkg_config::Config::new()
        .statik(true)
        .probe("nix-store-c")
        .expect("could not find static Nix store libraries via pkg-config");
}
