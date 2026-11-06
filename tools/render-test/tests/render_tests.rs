// Every render and query test of the shared suite on the Rust renderer. The
// port's ignore file (ignores/rust-port.json) lists what is expected to fail.

mod suite;

#[test]
fn render_tests() {
    assert_eq!(suite::run("render_tests", &[], Some(render_test::map::factory())), 0);
}
