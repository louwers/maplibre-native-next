// Ports render-test/render_test.cpp (`mln::runRenderTests`).
//
// Additions for the port, which never writes into the C++ tree:
// - `--output <dir>`: where actual.png, diff.png and the HTML report go
//   (default: `$TEST_UNDECLARED_OUTPUTS_DIR` under `bazel test`). C++ writes
//   them next to the tests and the manifest.
// - `--self-check`: renders nothing; every test's "actual" image is its
//   expected image, so the run exercises manifest parsing, metadata and
//   operation parsing, comparison and reporting. Exits non-zero unless
//   every test that runs passes (ignored ones included).

use crate::filesystem;
use crate::log::{self, Event};
use crate::manifest_parser::ManifestParser;
use crate::metadata::{TestMetadata, TestStatistics};
use crate::parser::{create_result_page, parse_test_metadata, serialize_json_value};
use crate::runner::{ImplFactory, TestRunner, UpdateResults};
use std::path::{Path, PathBuf};

// SHOW_ANSI_COLORS is not defined in the C++ build either.
const ANSI_COLOR_RED: &str = "";
const ANSI_COLOR_GREEN: &str = "";
const ANSI_COLOR_YELLOW: &str = "";
const ANSI_COLOR_GRAY: &str = "";
const ANSI_COLOR_LIGHT_GRAY: &str = "";
const ANSI_COLOR_RESET: &str = "";

const HELP: &str = "  render-test {OPTIONS}

    MapLibre Native Test Runner

  OPTIONS:

      -h, --help                        Display this help menu
      -r, --recycle-map                 Toggle reusing the map object
      -s, --shuffle                     Toggle shuffling the tests order
      -o, --online                      Toggle online mode (by default tests
                                        will run offline)
      --seed=[seed]                     Shuffle seed (default: random)
      -p[manifestPath],
      --manifestPath=[manifestPath]     Test manifest file path
      -f[filter], --filter=[filter]     Test filter regex
      -u[update], --update=[update]     Test results update mode. Supported
                                        values are: \"default\" Updates generic
                                        render test expectations. \"platform\"
                                        Updates platform specific render test
                                        expectations. \"metrics\" Updates
                                        expected metrics for configuration
                                        defined by a manifest. \"rebaseline\"
                                        Updates or creates expected metrics for
                                        configuration defined by a manifest.
      --output=[dir]                    (port) Directory for actual.png,
                                        diff.png and the HTML report
      --self-check                      (port) Compare each test's expected
                                        image with itself instead of rendering
      --timeout=[seconds]               (port) Error a test that runs longer
                                        than this (default 60, 0: no limit)
";

/// `ArgumentsTuple` plus the port's options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arguments {
    pub recycle_map: bool,
    pub shuffle: bool,
    pub online: bool,
    pub seed: u32,
    pub manifest_path: String,
    pub update_results: UpdateResults,
    pub test_filter: String,
    pub self_check: bool,
    pub output: Option<String>,
    /// Port-only per-test wall-clock limit in milliseconds (0: none).
    pub timeout_millis: u64,
}

/// `parseArguments(argc, argv)` (args.hxx). C++ calls `exit(code)`; here
/// `Err(code)` is returned after the same messages.
pub fn parse_arguments(args: &[String]) -> Result<Arguments, i32> {
    let mut recycle_map = false;
    let mut shuffle = false;
    let mut online = false;
    let mut seed: Option<u32> = None;
    let mut manifest_path: Option<String> = None;
    let mut test_filter: Option<String> = None;
    let mut update: Option<String> = None;
    let mut self_check = false;
    let mut output: Option<String> = None;
    let mut timeout_millis: u64 = 60_000;

    let parse_error = |message: String| -> i32 {
        log::info(Event::General, HELP);
        log::error(Event::General, &message);
        1
    };

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        // Long options: --name or --name=value.
        if let Some(long) = arg.strip_prefix("--") {
            let (name, inline_value) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let mut take_value = |what: &str| -> Result<String, i32> {
                if let Some(v) = inline_value.clone() {
                    return Ok(v);
                }
                if i < args.len() {
                    i += 1;
                    return Ok(args[i - 1].clone());
                }
                Err(parse_error(format!("Flag '{what}' requires an argument but received none")))
            };
            match name {
                "help" => {
                    log::info(Event::General, HELP);
                    return Err(0);
                }
                "recycle-map" => recycle_map = true,
                "shuffle" => shuffle = true,
                "online" => online = true,
                "self-check" => self_check = true,
                "seed" => {
                    let value = take_value("--seed")?;
                    seed = Some(value.parse().map_err(|_| {
                        parse_error(format!("Argument 'seed' received invalid value type '{value}'"))
                    })?);
                }
                "manifestPath" => manifest_path = Some(take_value("--manifestPath")?),
                "filter" => test_filter = Some(take_value("--filter")?),
                "update" => update = Some(take_value("--update")?),
                "output" => output = Some(take_value("--output")?),
                "timeout" => {
                    let value = take_value("--timeout")?;
                    timeout_millis = value
                        .parse::<f64>()
                        .ok()
                        .filter(|t| t.is_finite() && *t >= 0.0)
                        .map(|t| (t * 1000.0) as u64)
                        .ok_or_else(|| {
                            parse_error(format!("Argument 'timeout' received invalid value type '{value}'"))
                        })?;
                }
                _ => return Err(parse_error(format!("Flag could not be matched: {name}"))),
            }
            continue;
        }
        // Short options: -x, -xvalue, -x value, grouped flags (-rs).
        if let Some(short) = arg.strip_prefix('-').filter(|s| !s.is_empty()) {
            let chars: Vec<char> = short.chars().collect();
            let mut j = 0;
            while j < chars.len() {
                let c = chars[j];
                j += 1;
                match c {
                    'h' => {
                        log::info(Event::General, HELP);
                        return Err(0);
                    }
                    'r' => recycle_map = true,
                    's' => shuffle = true,
                    'o' => online = true,
                    'p' | 'f' | 'u' => {
                        let value = if j < chars.len() {
                            let rest: String = chars[j..].iter().collect();
                            j = chars.len();
                            rest
                        } else if i < args.len() {
                            i += 1;
                            args[i - 1].clone()
                        } else {
                            return Err(parse_error(format!(
                                "Flag '{c}' requires an argument but received none"
                            )));
                        };
                        match c {
                            'p' => manifest_path = Some(value),
                            'f' => test_filter = Some(value),
                            _ => update = Some(value),
                        }
                    }
                    _ => return Err(parse_error(format!("Flag could not be matched: {c}"))),
                }
            }
            continue;
        }
        return Err(parse_error(format!("Passed in argument, but no positional arguments were ready to receive it: {arg}")));
    }

    let update_results = match update.as_deref() {
        None => UpdateResults::No,
        Some("default") => UpdateResults::Default,
        Some("platform") => UpdateResults::Platform,
        Some("metrics") => UpdateResults::Metrics,
        Some("rebaseline") => UpdateResults::Rebaseline,
        Some(other) => return Err(parse_error(format!("Could not find key '{other}' in map for arg 'update'"))),
    };

    let Some(manifest_path) = manifest_path else {
        log::info(Event::General, HELP);
        log::error(Event::General, "Option 'manifestPath' is required");
        return Err(2);
    };

    if !filesystem::exists(&manifest_path) || filesystem::filename(&manifest_path).is_empty() {
        log::error(
            Event::General,
            &format!("Provided test manifest file path '{manifest_path}' does not exist"),
        );
        return Err(3);
    }

    // The port's restrictions.
    if update_results != UpdateResults::No {
        log::error(
            Event::General,
            "TODO(port): --update is not supported: the port never writes baselines into \
             ../maplibre-native (add alternate expectations by hand, see PLAN.md)",
        );
        return Err(1);
    }

    Ok(Arguments {
        recycle_map,
        shuffle,
        online,
        seed: seed.unwrap_or(1),
        manifest_path,
        update_results,
        test_filter: test_filter.unwrap_or_default(),
        self_check,
        output,
        timeout_millis,
    })
}

/// `runWithAlternateSources(runner, metadata, errored, passed)`.
fn run_with_alternate_sources(
    runner: &mut TestRunner,
    metadata: &mut TestMetadata,
    errored: &mut bool,
    passed: &mut bool,
) {
    const REPLACEMENTS_KEY: &str = "source-replace";
    const TARGET_KEY: &str = "replace";
    const SOURCE_KEY: &str = "with";
    const SOURCES_KEY: &str = "sources";

    let Some(test_element) = metadata.test_value() else {
        return;
    };
    let Some(replaces_element) = test_element.get(REPLACEMENTS_KEY).and_then(|r| r.as_array()) else {
        return;
    };
    if replaces_element.is_empty() {
        return;
    }
    let replaces_element = replaces_element.clone();

    // Each item in the source replacements array...
    for entry in &replaces_element {
        let (Some(target_name), Some(source_name)) = (
            entry.get(TARGET_KEY).and_then(|v| v.as_str()),
            entry.get(SOURCE_KEY).and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        let swappable = |document: &serde_json::Value| {
            let sources = &document[SOURCES_KEY];
            sources.get(target_name).is_some_and(|v| v.is_object())
                && sources.get(source_name).is_some_and(|v| v.is_object())
        };
        if !swappable(&metadata.document) {
            continue;
        }

        // `Value::Swap`: the two members keep their positions.
        let swap = |document: &mut serde_json::Value| {
            let sources = &mut document[SOURCES_KEY];
            let target = sources[target_name].take();
            let source = std::mem::replace(&mut sources[source_name], target);
            sources[target_name] = source;
        };
        let original_json = std::mem::take(&mut metadata.document_json);

        // Swap the named sources
        swap(&mut metadata.document);
        metadata.document_json = serialize_json_value(&metadata.document);

        // run the test again with the new source
        runner.run(metadata);

        // Restore the original source
        swap(&mut metadata.document);
        metadata.document_json = original_json;

        // stop if the test fails
        *errored = metadata.metrics_errored != 0
            || metadata.render_errored != 0
            || metadata.label_cut_off_found != 0;
        *passed = !*errored && metadata.metrics_failed == 0 && metadata.render_failed == 0;
        if !*passed {
            break;
        }
    }
}

/// The output directory: `--output` (relative to the directory `bazel run`
/// was started in), else `$TEST_UNDECLARED_OUTPUTS_DIR`.
fn output_directory(output: Option<&str>) -> Option<PathBuf> {
    if let Some(output) = output {
        let path = Path::new(output);
        if path.is_relative() {
            if let Some(cwd) = std::env::var_os("BUILD_WORKING_DIRECTORY") {
                return Some(Path::new(&cwd).join(path));
            }
        }
        return Some(path.to_path_buf());
    }
    std::env::var_os("TEST_UNDECLARED_OUTPUTS_DIR").map(PathBuf::from)
}

/// What a run produced: the process exit code, the statistics and the
/// metadata of every test that was not skipped, in run order.
#[derive(Debug)]
pub struct RunResult {
    pub return_code: i32,
    pub stats: TestStatistics,
    pub metadatas: Vec<TestMetadata>,
}

/// `mln::runRenderTests(argc, argv, testStatus)`. `factory` creates the map
/// for a test (`None`: only `--self-check` can pass).
pub fn run_render_tests(args: &[String], factory: Option<ImplFactory>) -> i32 {
    match parse_arguments(args) {
        Ok(arguments) => run_render_tests_with(&arguments, factory).return_code,
        Err(code) => code,
    }
}

/// `runRenderTests` after argument parsing.
pub fn run_render_tests_with(arguments: &Arguments, factory: Option<ImplFactory>) -> RunResult {
    let mut return_code = 0;
    let early = |return_code: i32| RunResult {
        return_code,
        stats: TestStatistics::default(),
        metadatas: Vec::new(),
    };

    let Arguments {
        recycle_map, shuffle, online, seed, manifest_path, update_results, test_filter, ..
    } = arguments.clone();

    crate::file_source::ProxyFileSource::set_offline(!online);
    // TODO(port): NetworkStatus::Set(online ? Online : Offline) (online file
    // source).

    let Some(manifest_data) = ManifestParser::parse_manifest(&manifest_path, test_filter) else {
        return early(5);
    };

    let Some(output_path) = output_directory(arguments.output.as_deref()) else {
        log::error(
            Event::General,
            "No output directory: pass --output <dir> (or run under `bazel test`)",
        );
        return early(1);
    };

    // mln::util::RunLoop runLoop: created by the map factory
    // (render_test_map::factory), which this crate does not depend on.
    let mut runner = TestRunner::new(
        manifest_data,
        update_results,
        factory,
        arguments.self_check,
        output_path.clone(),
    );
    if !arguments.self_check && arguments.timeout_millis > 0 {
        runner.set_test_timeout(Some(std::time::Duration::from_millis(arguments.timeout_millis)));
    }
    if shuffle {
        // printf("%d", uint32_t)
        println!("{ANSI_COLOR_YELLOW}Shuffle seed: {}{ANSI_COLOR_RESET}", seed as i32);
        runner.do_shuffle(seed);
    }

    let ignores = runner.get_manifest().get_ignores().to_vec();
    let test_paths = runner.get_manifest().get_test_paths().to_vec();
    let root_length = runner.get_manifest().get_test_root_path().len();
    let mut metadatas: Vec<TestMetadata> = Vec::with_capacity(test_paths.len());

    let mut stats = TestStatistics::default();

    for test_path in &test_paths {
        let mut metadata = parse_test_metadata(test_path);

        if !recycle_map {
            runner.reset();
        }

        let default_expectations = test_path.default_expectations();
        metadata.id = default_expectations[root_length + 1..default_expectations.len() - 1].to_string();
        let id = metadata.id.clone();

        let mut should_ignore = false;
        let mut ignore_reason = String::new();

        // `pair.first == ignoreName` compares std::filesystem paths.
        let ignore_name = Path::new(&id);
        if let Some((_, reason)) = ignores.iter().find(|(name, _)| Path::new(name) == ignore_name) {
            metadata.ignored_test = true;
            should_ignore = true;
            ignore_reason = reason.clone();
            if ignore_reason.starts_with("skip") {
                println!("{ANSI_COLOR_GRAY}* skipped {id} ({ignore_reason}){ANSI_COLOR_RESET}");
                log::info(Event::General, &format!("* skipped {id}({ignore_reason})"));
                continue;
            }
        }

        if metadata.document_object_empty() {
            metadata.metrics_errored += 1;
            metadata.render_errored += 1;
        } else {
            runner.run(&mut metadata);
        }

        let mut errored = metadata.metrics_errored != 0
            || metadata.render_errored != 0
            || metadata.label_cut_off_found != 0;
        let mut passed = !errored && metadata.metrics_failed == 0 && metadata.render_failed == 0;

        if passed {
            run_with_alternate_sources(&mut runner, &mut metadata, &mut errored, &mut passed);
        }

        if should_ignore {
            if passed {
                metadata.status = "ignored passed".to_string();
                metadata.color = "#E8A408".to_string();
                stats.ignore_passed_tests += 1;
                println!("{ANSI_COLOR_YELLOW}* ignore {id} ({ignore_reason}){ANSI_COLOR_RESET}");
                log::info(Event::General, &format!("* ignore {id} ({ignore_reason})"));
            } else {
                metadata.status = "ignored failed".to_string();
                metadata.color = "#9E9E9E".to_string();
                stats.ignore_failed_tests += 1;
                println!("{ANSI_COLOR_LIGHT_GRAY}* ignore {id} ({ignore_reason}){ANSI_COLOR_RESET}");
                log::info(Event::General, &format!("* ignore {id} ({ignore_reason})"));
            }
        } else {
            // Only fail the bots on render errors, this is a CI limitation that
            // we need to succeed on metrics failed so the rebaseline bot can
            // run next in the pipeline and collect the new baselines. The
            // rebaseline bot will ultimately report the error and block the
            // patch from being merged.
            if metadata.render_errored != 0 || metadata.render_failed != 0 {
                return_code = 1;
            }

            if passed {
                metadata.status = "passed".to_string();
                metadata.color = "green".to_string();
                stats.passed_tests += 1;
                println!("{ANSI_COLOR_GREEN}* passed {id}{ANSI_COLOR_RESET}");
                log::info(Event::General, &format!("* passed {id}"));
            } else if errored {
                metadata.status = "errored".to_string();
                metadata.color = "red".to_string();
                stats.errored_tests += 1;
                return_code = 2;
                println!("{ANSI_COLOR_RED}* errored {id}{ANSI_COLOR_RESET}");
                println!("{ANSI_COLOR_RED}* error: {}{ANSI_COLOR_RESET}", metadata.error_message);
                log::info(Event::General, &format!("* errored {id}"));
                log::info(Event::General, &format!("* error {}", metadata.error_message));
            } else {
                metadata.status = "failed".to_string();
                metadata.color = "red".to_string();
                stats.failed_tests += 1;
                return_code = 3;
                println!("{ANSI_COLOR_RED}* failed {id}{ANSI_COLOR_RESET}");
                log::info(Event::General, &format!("* failed {id}"));
            }
        }

        metadatas.push(metadata);
    }

    let manifest_name = filesystem::stem(&manifest_path).to_string();
    let result_path = output_path.join(format!("{manifest_name}.html"));
    let results_html = create_result_page(&stats, &metadatas, shuffle, seed);
    if let Err(e) = filesystem::create_dir_all(&output_path).and_then(|()| filesystem::write(&result_path, results_html)) {
        log::error(Event::General, &format!("Failed to write {}: {e}", result_path.display()));
    }

    let count = stats.errored_tests
        + stats.failed_tests
        + stats.ignore_failed_tests
        + stats.ignore_passed_tests
        + stats.passed_tests;
    let percent = |n: u32| 100.0 * f64::from(n) / f64::from(count);

    if stats.passed_tests != 0 {
        println!(
            "{ANSI_COLOR_GREEN}{} passed ({:.1}%){ANSI_COLOR_RESET}",
            stats.passed_tests,
            percent(stats.passed_tests)
        );
        log::info(Event::General, &format!("{} passed tests", stats.passed_tests));
    }
    if stats.ignore_passed_tests != 0 {
        println!(
            "{ANSI_COLOR_YELLOW}{} passed but were ignored ({:.1}%){ANSI_COLOR_RESET}",
            stats.ignore_passed_tests,
            percent(stats.ignore_passed_tests)
        );
        log::info(
            Event::General,
            &format!("{} passed tests but were ignored", stats.ignore_passed_tests),
        );
    }
    if stats.ignore_failed_tests != 0 {
        println!(
            "{ANSI_COLOR_LIGHT_GRAY}{} ignored ({:.1}%){ANSI_COLOR_RESET}",
            stats.ignore_failed_tests,
            percent(stats.ignore_failed_tests)
        );
        log::info(Event::General, &format!("{} ignored tests", stats.ignore_failed_tests));
    }
    if stats.failed_tests != 0 {
        println!(
            "{ANSI_COLOR_RED}{} failed ({:.1}%){ANSI_COLOR_RESET}",
            stats.failed_tests,
            percent(stats.failed_tests)
        );
        log::info(Event::General, &format!("{} failed tests", stats.failed_tests));
    }
    if stats.errored_tests != 0 {
        println!(
            "{ANSI_COLOR_RED}{} errored ({:.1}%){ANSI_COLOR_RESET}",
            stats.errored_tests,
            percent(stats.errored_tests)
        );
        log::info(Event::General, &format!("{} errored tests", stats.errored_tests));
    }

    let canonical = std::fs::canonicalize(&result_path).unwrap_or(result_path);
    println!("Results at: {}", canonical.display());

    if arguments.self_check {
        let not_passed = stats.failed_tests + stats.errored_tests + stats.ignore_failed_tests;
        if count == 0 || not_passed != 0 {
            println!("self-check: {not_passed} of {count} tests did not pass");
            if return_code == 0 {
                return_code = 4;
            }
        } else {
            println!("self-check: all {count} tests passed");
        }
    }

    RunResult { return_code, stats, metadatas }
}
