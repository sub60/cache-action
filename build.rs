use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

fn main() {
    pkg_config::Config::new()
        .cargo_metadata(false)
        .statik(true)
        .probe("nix-store-c")
        .expect("could not find static Nix store libraries via pkg-config");

    emit_search_paths_from_nix_ldflags();
    emit_cpp_runtime();
    emit_static_pkg_config_libs("nix-store-c");

    // nix-store's pkg-config metadata does not include all of the Boost
    // libraries that libnixstore/libnixutil actually reference in static
    // builds on Darwin.
    for lib in ["boost_context", "boost_iostreams", "boost_regex", "boost_url"]
    {
        println!("cargo:rustc-link-lib=static={lib}");
    }
}

fn emit_static_pkg_config_libs(package: &str) {
    let pkg_config = env::var_os("PKG_CONFIG")
        .unwrap_or_else(|| OsString::from("pkg-config"));

    let output = Command::new(pkg_config)
        .args(["--libs", "--static", package])
        .output()
        .unwrap_or_else(|err| panic!("could not run pkg-config: {err}"));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("pkg-config failed for {package}: {stderr}");
    }

    let stdout = String::from_utf8(output.stdout)
        .expect("pkg-config emitted non-UTF-8 link flags");
    let mut tokens = stdout.split_whitespace();

    while let Some(token) = tokens.next() {
        if let Some(path) = token.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={path}");
            continue;
        }

        if token == "-framework" {
            let framework =
                tokens.next().expect("pkg-config ended after '-framework'");
            println!("cargo:rustc-link-lib=framework={framework}");
            continue;
        }

        if let Some(name) = token.strip_prefix("-l") {
            emit_link_lib(name);
            continue;
        }

        if token.starts_with('/') {
            emit_absolute_library(token);
        }
    }
}

fn emit_absolute_library(path: &str) {
    let path = Path::new(path);

    if let Some(parent) = path.parent() {
        println!("cargo:rustc-link-search=native={}", parent.display());
    }

    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("pkg-config emitted a non-UTF-8 library path");
    let name = stem.strip_prefix("lib").unwrap_or(stem);

    match path.extension().and_then(|ext| ext.to_str()) {
        Some("a") => println!("cargo:rustc-link-lib=static={name}"),
        Some("dylib") => {
            let static_path = path.with_extension("a");
            if static_path.is_file() {
                println!("cargo:rustc-link-lib=static={name}");
            } else {
                println!("cargo:rustc-link-lib=dylib={name}");
            }
        },
        _ => {},
    }
}

fn emit_link_lib(name: &str) {
    match name {
        "c" | "m" | "pthread" | "sandbox" => {
            println!("cargo:rustc-link-lib={name}");
        },
        _ => {
            println!("cargo:rustc-link-lib=static={name}");
        },
    }
}

fn emit_search_paths_from_nix_ldflags() {
    let Some(ldflags) = env::var_os("NIX_LDFLAGS") else {
        return;
    };

    let ldflags = ldflags.to_string_lossy();

    for flag in ldflags.split_whitespace() {
        if let Some(path) = flag.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={path}");
        }
    }
}

fn emit_cpp_runtime() {
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "msvc" {
        return;
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let cpp_runtime = match target_os.as_str() {
        "macos" | "ios" | "tvos" | "watchos" | "visionos" => "c++",
        _ => "stdc++",
    };

    println!("cargo:rustc-link-lib=dylib={cpp_runtime}");
}
