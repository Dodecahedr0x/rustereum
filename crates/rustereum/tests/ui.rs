#[test]
fn unsupported_syntax_is_rejected() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
