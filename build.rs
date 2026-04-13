fn main() {
    pkg_config::Config::new()
        .statik(true)
        .probe("nix-expr-c")
        .expect("could not find static Nix libraries via pkg-config");
}
