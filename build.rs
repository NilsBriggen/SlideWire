// Translations are compiled in by rust-i18n; rebuild when they change.
fn main() {
    println!("cargo:rerun-if-changed=locales");
}
