//! Fails when the checked-in generated style code (crates/mln_style/src)
//! differs from what the generator writes for the C++ repo's v8.json.

use std::path::PathBuf;

fn runfiles() -> PathBuf {
    let base =
        std::env::var("RUNFILES_DIR").or_else(|_| std::env::var("TEST_SRCDIR")).expect("RUNFILES_DIR or TEST_SRCDIR");
    PathBuf::from(base)
}

#[test]
fn generated_code_is_up_to_date() {
    let spec_path = runfiles().join("maplibre+/scripts/style-spec-reference/v8.json");
    let spec = std::fs::read_to_string(&spec_path).unwrap_or_else(|e| panic!("{}: {e}", spec_path.display()));
    let root = runfiles().join("_main/crates/mln_style/src");
    let stale = style_codegen::check(&spec, &root).expect("generator");
    assert!(
        stale.is_empty(),
        "generated style code is out of date: {stale:?}\nRegenerate from the repository root with\n  \
         scripts/bazel build //tools/style-codegen\n  \
         bazel-bin/tools/style-codegen/style-codegen --spec ../maplibre-native/scripts/style-spec-reference/v8.json \
         --out crates/mln_style/src"
    );
}

#[test]
fn every_layer_type_is_generated() {
    let spec_path = runfiles().join("maplibre+/scripts/style-spec-reference/v8.json");
    let spec = std::fs::read_to_string(&spec_path).unwrap();
    let files = style_codegen::generate(&spec).unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    for t in [
        "fill",
        "line",
        "symbol",
        "circle",
        "heatmap",
        "fill_extrusion",
        "raster",
        "hillshade",
        "color_relief",
        "background",
        "location_indicator",
    ] {
        assert!(paths.contains(&format!("layers/{t}_layer.rs").as_str()), "{t}");
        assert!(paths.contains(&format!("layers/{t}_layer_properties.rs").as_str()), "{t}");
    }
    assert!(paths.contains(&"light.rs"));
    assert_eq!(files.len(), 23);
}
