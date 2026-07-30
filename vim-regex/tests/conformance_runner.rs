//! Integration test that runs the Neovim conformance suite.

mod conformance;

#[test]
fn neovim_conformance_suite() {
    let (passed, failed, errors) = conformance::run_all();

    if failed > 0 {
        let summary = errors.join("\n\n");
        panic!("Conformance suite: {passed} passed, {failed} FAILED.\n\nFailures:\n{summary}");
    }

    assert!(
        passed > 0,
        "No conformance tests ran. Is the corpus directory populated?"
    );

    eprintln!("Conformance suite: {passed} passed, 0 failed.");
}
