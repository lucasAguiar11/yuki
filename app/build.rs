fn main() {
    // `i18n!` embeds locales/ at compile time but doesn't tell cargo to watch it.
    println!("cargo:rerun-if-changed=locales");
}
