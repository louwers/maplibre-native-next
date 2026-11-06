// Runs the whole expression corpus. The test fails only on regressions:
// tests that fail but are not listed in expected_failures.txt. Tests listed
// there that pass are reported so the list can shrink.

use expression_test::{find_metrics_dir, print_stats, run};
use std::collections::BTreeSet;

#[test]
fn expression_corpus() {
    let metrics = find_metrics_dir().expect("expression-test-files in the runfiles");
    let stats = run(&metrics, &[], None, false);
    print_stats(&stats);

    let expected_path = std::path::Path::new(&std::env::var("TEST_SRCDIR").unwrap())
        .join("_main/tools/expression-test/expected_failures.txt");
    let expected_text = std::fs::read_to_string(&expected_path).expect("expected_failures.txt");
    let expected: BTreeSet<&str> =
        expected_text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();

    let mut regressions = Vec::new();
    for t in stats.failed.iter().chain(&stats.errored) {
        if !expected.contains(t.id.as_str()) {
            regressions.push(t);
        }
    }
    let newly_passing: Vec<&str> =
        stats.passed.iter().map(|t| t.id.as_str()).filter(|id| expected.contains(id)).collect();

    println!(
        "expression corpus: {} of {} passed ({} ignored, {} failed, {} errored); {} expected failures",
        stats.passed.len() + stats.ignore_passed.len(),
        stats.test_count(),
        stats.ignore_failed.len() + stats.ignore_passed.len(),
        stats.failed.len(),
        stats.errored.len(),
        expected.len()
    );
    for id in &newly_passing {
        println!("now passing (remove from expected_failures.txt): {id}");
    }
    for t in &regressions {
        println!("* REGRESSION {}\n{}", t.id, t.text);
    }
    assert!(regressions.is_empty(), "{} expression tests regressed", regressions.len());
}
