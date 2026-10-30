//! Compare two JSON reports produced by `parity`.
//!
//! Entity IDs are random per run and are ignored. Pages are compared by index;
//! geometry and confidence use small tolerances, and the tensor-derived
//! `text-mask` digest is reported as a warning rather than a failure.

use std::{fs, path::Path, path::PathBuf, process::ExitCode};

use anyhow::{Context as _, Result};
use clap::Parser;
use serde_json::{Value, json};

/// Bounds within this distance are considered equal.
const BOUNDS_TOLERANCE: f64 = 1.0;
/// Confidences within this distance are considered equal.
const CONFIDENCE_TOLERANCE: f64 = 0.02;

#[derive(Debug, Parser)]
#[command(version, about = "Compare two Koharu pipeline parity reports")]
struct Arguments {
    /// Report from the reference device.
    #[arg(value_name = "REFERENCE")]
    reference: PathBuf,

    /// Report from the device under test.
    #[arg(value_name = "CANDIDATE")]
    candidate: PathBuf,

    /// Treat a differing text-mask digest as a failure instead of a warning.
    #[arg(long)]
    strict_mask: bool,

    /// Print the summary as JSON.
    #[arg(long)]
    json: bool,
}

fn main() -> Result<ExitCode> {
    let arguments = Arguments::parse();
    let reference = read_report(&arguments.reference)?;
    let candidate = read_report(&arguments.candidate)?;
    let comparison = compare(&reference, &candidate, arguments.strict_mask)?;

    if arguments.json {
        println!("{}", serde_json::to_string_pretty(&comparison.document())?);
    } else {
        print!("{}", comparison.render());
    }

    Ok(if comparison.failed() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn read_report(path: &Path) -> Result<Value> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Section {
    Page,
    Regions,
    Texts,
    Source,
    TextMask,
}

impl Section {
    const ALL: [Section; 5] = [
        Self::Page,
        Self::Regions,
        Self::Texts,
        Self::Source,
        Self::TextMask,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Regions => "regions",
            Self::Texts => "texts",
            Self::Source => "source",
            Self::TextMask => "text-mask",
        }
    }
}

#[derive(Debug, Clone)]
struct Finding {
    page: Option<usize>,
    section: Section,
    message: String,
}

#[derive(Debug, Default)]
struct Comparison {
    pages: usize,
    failures: Vec<Finding>,
    warnings: Vec<Finding>,
}

impl Comparison {
    fn fail(&mut self, page: Option<usize>, section: Section, message: String) {
        self.failures.push(Finding {
            page,
            section,
            message,
        });
    }

    fn warn(&mut self, page: Option<usize>, section: Section, message: String) {
        self.warnings.push(Finding {
            page,
            section,
            message,
        });
    }

    fn failed(&self) -> bool {
        !self.failures.is_empty()
    }

    fn counts(findings: &[Finding]) -> Vec<(&'static str, usize)> {
        Section::ALL
            .into_iter()
            .filter_map(|section| {
                let count = findings
                    .iter()
                    .filter(|finding| finding.section == section)
                    .count();
                (count > 0).then(|| (section.name(), count))
            })
            .collect()
    }

    fn render(&self) -> String {
        let mut output = format!("parity: {} page(s) compared\n", self.pages);
        output.push_str(&format!("failures: {}", self.failures.len()));
        let counts = Self::counts(&self.failures);
        if !counts.is_empty() {
            let counts: Vec<_> = counts
                .into_iter()
                .map(|(name, count)| format!("{name} {count}"))
                .collect();
            output.push_str(&format!(" ({})", counts.join(", ")));
        }
        output.push('\n');
        for finding in &self.failures {
            output.push_str(&format!("  {}\n", describe(finding)));
        }

        if !self.warnings.is_empty() {
            output.push_str(&format!("warnings: {}\n", self.warnings.len()));
            for finding in &self.warnings {
                output.push_str(&format!("  {}\n", describe(finding)));
            }
        }

        output.push_str(if self.failed() {
            "parity failed\n"
        } else {
            "parity matched\n"
        });
        output
    }

    fn document(&self) -> Value {
        json!({
            "pages": self.pages,
            "matched": !self.failed(),
            "failures": findings_document(&self.failures),
            "warnings": findings_document(&self.warnings),
            "counts": {
                "failures": counts_document(&self.failures),
                "warnings": counts_document(&self.warnings),
            },
        })
    }
}

fn describe(finding: &Finding) -> String {
    match finding.page {
        Some(page) => format!(
            "page {page} {}: {}",
            finding.section.name(),
            finding.message
        ),
        None => format!("{}: {}", finding.section.name(), finding.message),
    }
}

fn findings_document(findings: &[Finding]) -> Value {
    Value::Array(
        findings
            .iter()
            .map(|finding| {
                json!({
                    "page": finding.page,
                    "section": finding.section.name(),
                    "message": finding.message,
                })
            })
            .collect(),
    )
}

fn counts_document(findings: &[Finding]) -> Value {
    let mut counts = serde_json::Map::new();
    for section in Section::ALL {
        let count = findings
            .iter()
            .filter(|finding| finding.section == section)
            .count();
        counts.insert(section.name().to_owned(), json!(count));
    }
    Value::Object(counts)
}

fn compare(reference: &Value, candidate: &Value, strict_mask: bool) -> Result<Comparison> {
    let reference_pages = pages(reference)?;
    let candidate_pages = pages(candidate)?;
    let mut comparison = Comparison::default();

    if reference_pages.len() != candidate_pages.len() {
        comparison.fail(
            None,
            Section::Page,
            format!(
                "page count {} != {}",
                reference_pages.len(),
                candidate_pages.len()
            ),
        );
    }

    for (index, (expected, actual)) in reference_pages.iter().zip(candidate_pages).enumerate() {
        comparison.pages += 1;
        compare_page(&mut comparison, index, expected, actual, strict_mask);
    }

    Ok(comparison)
}

fn pages(report: &Value) -> Result<&[Value]> {
    report
        .get("pages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .context("report has no `pages` array")
}

fn compare_page(
    comparison: &mut Comparison,
    page: usize,
    expected: &Value,
    actual: &Value,
    strict_mask: bool,
) {
    for dimension in ["width", "height"] {
        let expected_value = expected.get(dimension);
        let actual_value = actual.get(dimension);
        if expected_value != actual_value {
            comparison.fail(
                Some(page),
                Section::Page,
                format!(
                    "{dimension} {} != {}",
                    scalar(expected_value),
                    scalar(actual_value)
                ),
            );
        }
    }

    compare_regions(comparison, page, expected, actual);
    compare_texts(comparison, page, expected, actual);
    compare_assets(comparison, page, expected, actual, strict_mask);
}

fn scalar(value: Option<&Value>) -> String {
    value.map_or_else(|| "none".to_owned(), Value::to_string)
}

/// Rounds below the comparison tolerances so small differences keep a stable
/// order while the actual comparison still uses the tolerances above.
fn region_sort_key(region: &RegionView) -> (String, Option<String>, Option<[i64; 4]>, Option<i64>) {
    (
        region.kind.clone(),
        region.label.clone(),
        region
            .bounds
            .map(|bounds| bounds.map(|value| (value * 10.0).round() as i64)),
        region
            .confidence
            .map(|value| (value * 1000.0).round() as i64),
    )
}

#[derive(Debug, Clone, PartialEq)]
struct RegionView {
    kind: String,
    label: Option<String>,
    bounds: Option<[f64; 4]>,
    confidence: Option<f64>,
}

impl RegionView {
    fn read(value: &Value) -> Self {
        Self {
            kind: value["kind"].as_str().unwrap_or_default().to_owned(),
            label: value["label"].as_str().map(str::to_owned),
            bounds: bounds(value.get("bounds")),
            confidence: value["confidence"].as_f64(),
        }
    }
}

fn bounds(value: Option<&Value>) -> Option<[f64; 4]> {
    Some([
        value?.get(0)?.as_f64()?,
        value?.get(1)?.as_f64()?,
        value?.get(2)?.as_f64()?,
        value?.get(3)?.as_f64()?,
    ])
}

fn compare_regions(comparison: &mut Comparison, page: usize, expected: &Value, actual: &Value) {
    let expected = region_views(expected.get("regions"));
    let actual = region_views(actual.get("regions"));

    if expected.len() != actual.len() {
        comparison.fail(
            Some(page),
            Section::Regions,
            format!("count {} != {}", expected.len(), actual.len()),
        );
    }

    for (index, (expected, actual)) in expected.iter().zip(&actual).enumerate() {
        compare_region(comparison, page, index, expected, actual);
    }
}

fn region_views(value: Option<&Value>) -> Vec<RegionView> {
    let mut regions: Vec<_> = value
        .and_then(Value::as_array)
        .map(|regions| regions.iter().map(RegionView::read).collect())
        .unwrap_or_default();
    regions.sort_by_cached_key(region_sort_key);
    regions
}

fn compare_region(
    comparison: &mut Comparison,
    page: usize,
    index: usize,
    expected: &RegionView,
    actual: &RegionView,
) {
    let fail = |comparison: &mut Comparison, message: String| {
        comparison.fail(
            Some(page),
            Section::Regions,
            format!("region {index}: {message}"),
        );
    };

    if expected.kind != actual.kind {
        fail(
            comparison,
            format!("kind {} != {}", expected.kind, actual.kind),
        );
    }
    if expected.label != actual.label {
        fail(
            comparison,
            format!(
                "label {} != {}",
                optional(expected.label.as_deref()),
                optional(actual.label.as_deref())
            ),
        );
    }

    const AXES: [&str; 4] = ["left", "top", "right", "bottom"];
    match (expected.bounds, actual.bounds) {
        (Some(expected_bounds), Some(actual_bounds)) => {
            for axis in 0..AXES.len() {
                let expected_value = expected_bounds[axis];
                let actual_value = actual_bounds[axis];
                if !close(expected_value, actual_value, BOUNDS_TOLERANCE) {
                    fail(
                        comparison,
                        format!(
                            "bounds {} {expected_value:.2} != {actual_value:.2}",
                            AXES[axis]
                        ),
                    );
                }
            }
        }
        (None, None) => {}
        _ => fail(comparison, "bounds missing on one side".to_owned()),
    }

    match (expected.confidence, actual.confidence) {
        (Some(expected_confidence), Some(actual_confidence)) => {
            if !close(expected_confidence, actual_confidence, CONFIDENCE_TOLERANCE) {
                fail(
                    comparison,
                    format!("confidence {expected_confidence:.3} != {actual_confidence:.3}"),
                );
            }
        }
        (None, None) => {}
        _ => fail(comparison, "confidence missing on one side".to_owned()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TextView {
    source: String,
    translation: Option<String>,
    direction: Option<String>,
}

impl TextView {
    fn read(value: &Value) -> Self {
        Self {
            source: normalize_text(value["source"].as_str().unwrap_or_default()),
            translation: value["translation"].as_str().map(str::to_owned),
            direction: value["direction"].as_str().map(str::to_owned),
        }
    }
}

/// Collapses whitespace so OCR line wrapping does not count as a difference.
fn normalize_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn compare_texts(comparison: &mut Comparison, page: usize, expected: &Value, actual: &Value) {
    let expected = text_views(expected.get("texts"));
    let actual = text_views(actual.get("texts"));

    if expected.len() != actual.len() {
        comparison.fail(
            Some(page),
            Section::Texts,
            format!("count {} != {}", expected.len(), actual.len()),
        );
    }

    for (index, (expected, actual)) in expected.iter().zip(&actual).enumerate() {
        let fail = |comparison: &mut Comparison, message: String| {
            comparison.fail(
                Some(page),
                Section::Texts,
                format!("text {index}: {message}"),
            );
        };
        if expected.source != actual.source {
            fail(
                comparison,
                format!("source {:?} != {:?}", expected.source, actual.source),
            );
        }
        if expected.translation != actual.translation {
            fail(
                comparison,
                format!(
                    "translation {:?} != {:?}",
                    expected.translation, actual.translation
                ),
            );
        }
        if expected.direction != actual.direction {
            fail(
                comparison,
                format!(
                    "direction {:?} != {:?}",
                    expected.direction, actual.direction
                ),
            );
        }
    }
}

fn text_views(value: Option<&Value>) -> Vec<TextView> {
    let mut texts: Vec<_> = value
        .and_then(Value::as_array)
        .map(|texts| texts.iter().map(TextView::read).collect())
        .unwrap_or_default();
    texts.sort();
    texts
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssetView {
    media_type: Option<String>,
    width: Option<u64>,
    height: Option<u64>,
    bytes: Option<u64>,
    digest: Option<u64>,
}

impl AssetView {
    fn read(value: Option<&Value>) -> Option<Self> {
        let value = value.filter(|value| !value.is_null())?;
        Some(Self {
            media_type: value["media_type"].as_str().map(str::to_owned),
            width: value["width"].as_u64(),
            height: value["height"].as_u64(),
            bytes: value["bytes"].as_u64(),
            digest: value["digest"].as_u64(),
        })
    }
}

fn compare_assets(
    comparison: &mut Comparison,
    page: usize,
    expected: &Value,
    actual: &Value,
    strict_mask: bool,
) {
    compare_exact_asset(
        comparison,
        page,
        Section::Source,
        "source",
        AssetView::read(expected.pointer("/assets/source")).as_ref(),
        AssetView::read(actual.pointer("/assets/source")).as_ref(),
    );
    compare_mask(
        comparison,
        page,
        AssetView::read(expected.pointer("/assets/text-mask")).as_ref(),
        AssetView::read(actual.pointer("/assets/text-mask")).as_ref(),
        strict_mask,
    );
}

fn compare_exact_asset(
    comparison: &mut Comparison,
    page: usize,
    section: Section,
    name: &str,
    expected: Option<&AssetView>,
    actual: Option<&AssetView>,
) {
    let (Some(expected), Some(actual)) = (expected, actual) else {
        if expected.is_some() != actual.is_some() {
            comparison.fail(
                Some(page),
                section,
                format!("{name} present on only one side"),
            );
        }
        return;
    };

    if expected.media_type != actual.media_type {
        comparison.fail(
            Some(page),
            section,
            format!(
                "{name} media_type {} != {}",
                optional(expected.media_type.as_deref()),
                optional(actual.media_type.as_deref())
            ),
        );
    }
    for (field, expected_value, actual_value) in [
        ("width", expected.width, actual.width),
        ("height", expected.height, actual.height),
        ("bytes", expected.bytes, actual.bytes),
        ("digest", expected.digest, actual.digest),
    ] {
        if expected_value != actual_value {
            comparison.fail(
                Some(page),
                section,
                format!(
                    "{name} {field} {} != {}",
                    optional(expected_value),
                    optional(actual_value)
                ),
            );
        }
    }
}

fn compare_mask(
    comparison: &mut Comparison,
    page: usize,
    expected: Option<&AssetView>,
    actual: Option<&AssetView>,
    strict_mask: bool,
) {
    let (Some(expected), Some(actual)) = (expected, actual) else {
        if expected.is_some() != actual.is_some() {
            comparison.fail(
                Some(page),
                Section::TextMask,
                "text-mask present on only one side".to_owned(),
            );
        }
        return;
    };

    if expected.media_type != actual.media_type {
        comparison.fail(
            Some(page),
            Section::TextMask,
            format!(
                "text-mask media_type {} != {}",
                optional(expected.media_type.as_deref()),
                optional(actual.media_type.as_deref())
            ),
        );
    }
    for (field, expected_value, actual_value) in [
        ("width", expected.width, actual.width),
        ("height", expected.height, actual.height),
    ] {
        if expected_value != actual_value {
            comparison.fail(
                Some(page),
                Section::TextMask,
                format!(
                    "text-mask {field} {} != {}",
                    optional(expected_value),
                    optional(actual_value)
                ),
            );
        }
    }

    // The mask is tensor-derived, so small backend differences are expected.
    if expected.digest != actual.digest {
        let message = format!(
            "text-mask digest {} != {}",
            optional(expected.digest),
            optional(actual.digest)
        );
        if strict_mask {
            comparison.fail(Some(page), Section::TextMask, message);
        } else {
            comparison.warn(Some(page), Section::TextMask, message);
        }
    }
}

fn optional(value: Option<impl std::fmt::Display>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| value.to_string())
}

fn close(left: f64, right: f64, tolerance: f64) -> bool {
    (left - right).abs() <= tolerance
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(source: &str) -> Value {
        serde_json::from_str(source).expect("fixture is valid JSON")
    }

    const REFERENCE: &str = r#"{
        "pages": [{
            "id": "page-a",
            "label": "001.png",
            "width": 800.0,
            "height": 1200.0,
            "regions": [
                {"id": "r1", "kind": "text", "label": null, "bounds": [10.0, 20.0, 110.0, 60.0], "confidence": 0.9},
                {"id": "r2", "kind": "panel", "label": "panel", "bounds": [0.0, 0.0, 800.0, 400.0], "confidence": null}
            ],
            "texts": [
                {"id": "t1", "source": "  hello\n world ", "source_language": "ja-JP", "translation": "Hello world", "direction": "LeftToRight"}
            ],
            "assets": {
                "source": {"media_type": "image/png", "width": 800, "height": 1200, "bytes": 1234, "digest": 42},
                "text-mask": {"media_type": "image/png", "width": 800, "height": 1200, "bytes": 99, "digest": 7}
            }
        }]
    }"#;

    /// Different IDs, region order, whitespace, and within-tolerance values.
    const CANDIDATE: &str = r#"{
        "pages": [{
            "id": "page-b",
            "label": "001.png",
            "width": 800.0,
            "height": 1200.0,
            "regions": [
                {"id": "r9", "kind": "panel", "label": "panel", "bounds": [0.0, 0.0, 800.0, 400.0], "confidence": null},
                {"id": "r8", "kind": "text", "label": null, "bounds": [10.6, 20.4, 110.3, 60.8], "confidence": 0.91}
            ],
            "texts": [
                {"id": "t9", "source": "hello world", "source_language": "ja-JP", "translation": "Hello world", "direction": "LeftToRight"}
            ],
            "assets": {
                "source": {"media_type": "image/png", "width": 800, "height": 1200, "bytes": 1234, "digest": 42},
                "text-mask": {"media_type": "image/png", "width": 800, "height": 1200, "bytes": 99, "digest": 7}
            }
        }]
    }"#;

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(normalize_text("  hello \n\t world  "), "hello world");
        assert_eq!(normalize_text(""), "");
    }

    #[test]
    fn tolerances_are_inclusive() {
        assert!(close(1.0, 2.0, BOUNDS_TOLERANCE));
        assert!(!close(1.0, 2.01, BOUNDS_TOLERANCE));
        assert!(close(0.9, 0.91, CONFIDENCE_TOLERANCE));
        assert!(!close(0.9, 0.93, CONFIDENCE_TOLERANCE));
    }

    #[test]
    fn matching_reports_pass() {
        let comparison =
            compare(&report(REFERENCE), &report(CANDIDATE), false).expect("valid reports");
        assert_eq!(comparison.pages, 1);
        assert!(comparison.failures.is_empty());
        assert!(comparison.warnings.is_empty());
    }

    #[test]
    fn mismatching_report_reports_each_section() {
        let candidate = report(
            CANDIDATE
                .replace("800.0", "900.0")
                .replace(
                    r#""translation": "Hello world""#,
                    r#""translation": "Hello""#,
                )
                .as_str(),
        );
        let comparison = compare(&report(REFERENCE), &candidate, false).expect("valid reports");

        assert!(comparison.failed());
        for section in [Section::Page, Section::Regions, Section::Texts] {
            assert!(
                comparison
                    .failures
                    .iter()
                    .any(|finding| finding.section == section),
                "no failure for {}",
                section.name()
            );
        }
    }

    #[test]
    fn text_mask_digest_is_a_warning_unless_strict() {
        let candidate = report(CANDIDATE.replace("\"digest\": 7", "\"digest\": 8").as_str());

        let relaxed = compare(&report(REFERENCE), &candidate, false).expect("valid reports");
        assert!(!relaxed.failed());
        assert_eq!(relaxed.warnings.len(), 1);
        assert_eq!(relaxed.warnings[0].section, Section::TextMask);

        let strict = compare(&report(REFERENCE), &candidate, true).expect("valid reports");
        assert!(strict.failed());
    }

    #[test]
    fn source_digest_is_a_failure() {
        let candidate = report(
            CANDIDATE
                .replace("\"digest\": 42", "\"digest\": 43")
                .as_str(),
        );
        let comparison = compare(&report(REFERENCE), &candidate, false).expect("valid reports");
        assert!(comparison.failed());
        assert!(
            comparison
                .failures
                .iter()
                .any(|finding| finding.section == Section::Source)
        );
    }
}
