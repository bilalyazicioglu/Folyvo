#[test]
fn crate_exposes_its_version() {
    assert_eq!(folyvo_core::VERSION, env!("CARGO_PKG_VERSION"));
}
