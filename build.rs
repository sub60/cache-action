fn main() {
    pkg_config::Config::new()
        .statik(true)
        .probe("nix-store-c")
        .expect("could not find static Nix store libraries via pkg-config");
}
