fn main() {
    println!("cargo:rustc-check-cfg=cfg(cargo_clippy)");
    println!("cargo:rustc-check-cfg=cfg(feature,values(\"cargo-clippy\"))");
}
