//! SARIF 2.1.0 rendering, used only by [`super::render`]'s [`super::Format::Sarif`] arm.
//!
//! GitHub's `github/codeql-action/upload-sarif` consumes this format;
//! `::error` workflow commands cap out at 10 per severity per step and are
//! silently dropped beyond that, which is why issue #11 asked for this
//! instead. The model here is deliberately small and typed rather than built
//! through `serde_json::json!` — every field below is required or
//! deliberately omitted, not guessed.
//!
//! `originalUriBaseIds` is omitted entirely: this renderer has no project
//! root to anchor `%SRCROOT%` to (GitHub resolves it to the checkout when
//! the file is uploaded from CI), and there is no description registry
//! reachable from a [`LintResult`], so `rules[].shortDescription.text` is
//! the rule name, not a fuller description.

use arch_lint_core::{Label, LintResult, Location, Severity, Violation};
use serde::Serialize;
use std::collections::BTreeMap;

const SCHEMA_URL: &str = "https://json.schemastore.org/sarif-2.1.0.json";
const SARIF_VERSION: &str = "2.1.0";
const DRIVER_NAME: &str = "arch-lint";
const INFORMATION_URI: &str = "https://github.com/ynishi/arch-lint";
const SRC_ROOT_URI_BASE_ID: &str = "%SRCROOT%";
const FINGERPRINT_KEY: &str = "arch-lint/fingerprint/v1";

/// Renders `result` as a pretty-printed SARIF 2.1.0 document, trailing
/// newline included — same convention as `Format::Json`.
///
/// # Errors
///
/// Returns a `serde_json::Error` if serialization fails (this should not
/// happen for the well-formed model built here).
pub(super) fn render(result: &LintResult) -> Result<String, serde_json::Error> {
    let document = Document::from_result(result);
    Ok(format!("{}\n", serde_json::to_string_pretty(&document)?))
}

#[derive(Serialize)]
struct Document {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: &'static str,
    runs: Vec<Run>,
}

#[derive(Serialize)]
struct Run {
    tool: Tool,
    results: Vec<SarifResult>,
}

#[derive(Serialize)]
struct Tool {
    driver: Driver,
}

#[derive(Serialize)]
struct Driver {
    name: &'static str,
    #[serde(rename = "semanticVersion")]
    semantic_version: &'static str,
    #[serde(rename = "informationUri")]
    information_uri: &'static str,
    rules: Vec<ReportingDescriptor>,
}

#[derive(Serialize)]
struct ReportingDescriptor {
    id: String,
    name: String,
    #[serde(rename = "shortDescription")]
    short_description: Message,
}

#[derive(Serialize)]
struct Message {
    text: String,
}

#[derive(Serialize)]
struct SarifResult {
    #[serde(rename = "ruleId")]
    rule_id: String,
    #[serde(rename = "ruleIndex")]
    rule_index: usize,
    level: &'static str,
    message: Message,
    locations: Vec<LocationWrapper>,
    #[serde(rename = "relatedLocations", skip_serializing_if = "Vec::is_empty")]
    related_locations: Vec<RelatedLocation>,
    #[serde(rename = "partialFingerprints")]
    partial_fingerprints: BTreeMap<&'static str, String>,
    properties: Properties,
}

#[derive(Serialize)]
struct LocationWrapper {
    #[serde(rename = "physicalLocation")]
    physical_location: PhysicalLocation,
}

#[derive(Serialize)]
struct RelatedLocation {
    #[serde(rename = "physicalLocation")]
    physical_location: PhysicalLocation,
    message: Message,
}

#[derive(Serialize)]
struct PhysicalLocation {
    #[serde(rename = "artifactLocation")]
    artifact_location: ArtifactLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<Region>,
}

#[derive(Serialize)]
struct ArtifactLocation {
    uri: String,
    #[serde(rename = "uriBaseId")]
    uri_base_id: &'static str,
}

#[derive(Serialize)]
struct Region {
    #[serde(rename = "startLine")]
    start_line: usize,
    #[serde(rename = "startColumn", skip_serializing_if = "Option::is_none")]
    start_column: Option<usize>,
}

#[derive(Serialize)]
struct Properties {
    severity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggestion: Option<String>,
}

impl Document {
    fn from_result(result: &LintResult) -> Self {
        // One rule per distinct (code, rule) pair, sorted by `code` (the
        // map key) so the output — and `ruleIndex` below — is deterministic
        // regardless of violation order.
        let mut rule_names: BTreeMap<&str, &str> = BTreeMap::new();
        for violation in &result.violations {
            rule_names
                .entry(violation.code.as_str())
                .or_insert(violation.rule.as_str());
        }

        let rules: Vec<ReportingDescriptor> = rule_names
            .iter()
            .map(|(id, name)| ReportingDescriptor {
                id: (*id).to_string(),
                name: (*name).to_string(),
                short_description: Message {
                    text: (*name).to_string(),
                },
            })
            .collect();

        let rule_index: BTreeMap<&str, usize> = rule_names
            .keys()
            .enumerate()
            .map(|(index, id)| (*id, index))
            .collect();

        let results = result
            .violations
            .iter()
            .map(|violation| {
                // Every violation's code was inserted into `rule_names`
                // above, so this always hits; `unwrap_or(0)` only avoids an
                // `expect`/`unwrap` call, it is not a real fallback path.
                let index = rule_index
                    .get(violation.code.as_str())
                    .copied()
                    .unwrap_or(0);
                SarifResult::from_violation(violation, index)
            })
            .collect();

        Self {
            schema: SCHEMA_URL,
            version: SARIF_VERSION,
            runs: vec![Run {
                tool: Tool {
                    driver: Driver {
                        name: DRIVER_NAME,
                        semantic_version: env!("CARGO_PKG_VERSION"),
                        information_uri: INFORMATION_URI,
                        rules,
                    },
                },
                results,
            }],
        }
    }
}

impl SarifResult {
    fn from_violation(violation: &Violation, rule_index: usize) -> Self {
        let related_locations = violation
            .labels
            .iter()
            .map(RelatedLocation::from_label)
            .collect();

        let mut partial_fingerprints = BTreeMap::new();
        partial_fingerprints.insert(FINGERPRINT_KEY, crate::baseline::fingerprint(violation));

        Self {
            rule_id: violation.code.clone(),
            rule_index,
            level: level_of(violation.severity),
            message: Message {
                text: violation.message.clone(),
            },
            locations: vec![LocationWrapper {
                physical_location: PhysicalLocation::from_location(&violation.location),
            }],
            related_locations,
            partial_fingerprints,
            properties: Properties {
                severity: violation.severity.to_string(),
                suggestion: violation
                    .suggestion
                    .as_ref()
                    .map(|suggestion| suggestion.message.clone()),
            },
        }
    }
}

impl RelatedLocation {
    fn from_label(label: &Label) -> Self {
        Self {
            physical_location: PhysicalLocation::from_location(&label.location),
            message: Message {
                text: label.message.clone(),
            },
        }
    }
}

impl PhysicalLocation {
    fn from_location(location: &Location) -> Self {
        Self {
            artifact_location: ArtifactLocation {
                uri: root_relative_uri(location),
                uri_base_id: SRC_ROOT_URI_BASE_ID,
            },
            // SARIF's `region.startLine` is 1-based and the schema requires
            // `minimum: 1`; `line == 0` is a project-level violation (see
            // `ProjectRule`'s doc example, `Location::new(root, 0, 0)` — no
            // line to point at), so `region` is omitted entirely rather
            // than emitting a `0` the schema rejects. `artifactLocation`
            // alone is still a valid SARIF location.
            region: if location.line == 0 {
                None
            } else {
                Some(Region {
                    start_line: location.line,
                    start_column: if location.column == 0 {
                        None
                    } else {
                        Some(location.column)
                    },
                })
            },
        }
    }
}

/// `Violation::location.file` is already root-relative (see `baseline`'s
/// module docs); this only normalizes path separators to `/` so the URI is
/// stable regardless of the platform arch-lint ran on. It assumes the path
/// is already root-relative — it has no project root to strip from an
/// absolute path, so an absolute `location.file` passes through unchanged
/// (callers are expected to hand `Violation::location.file` as-is, never an
/// absolute path).
fn root_relative_uri(location: &Location) -> String {
    location.file.to_string_lossy().replace('\\', "/")
}

/// SARIF `level`: `Severity::Info` maps to `"note"` (SARIF has no `"info"`
/// level), the other two map to their own name.
fn level_of(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "note",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::baseline;
    use arch_lint_core::Suggestion;
    use serde_json::Value;
    use std::path::PathBuf;

    fn two_violations() -> LintResult {
        let mut result = LintResult::new();
        result.files_checked = 2;
        result.violations.push(
            Violation::new(
                "ALD009",
                "acyclic",
                Severity::Error,
                Location::new(PathBuf::from("src/domain/a.rs"), 1, 23),
                "Cycle between scopes: domain \u{2192} infra \u{2192} domain",
            )
            .with_suggestion(Suggestion::new("Break the cycle"))
            .with_label(Label::new(
                Location::new(PathBuf::from("src/infra/b.rs"), 2, 29),
                "infra \u{2192} domain",
            ))
            .with_label(Label::new(
                Location::new(PathBuf::from("src/domain/c.rs"), 3, 0),
                "domain \u{2192} infra",
            )),
        );
        result.violations.push(Violation::new(
            "AL002",
            "no-sync-io",
            Severity::Warning,
            Location::new(PathBuf::from("src/io.rs"), 4, 1),
            "Blocking IO in async fn",
        ));
        result
    }

    fn render_value(result: &LintResult) -> Value {
        let out = render(result).expect("render");
        serde_json::from_str(&out).expect("valid json")
    }

    #[test]
    fn has_schema_and_version() {
        let doc = render_value(&two_violations());
        assert_eq!(
            doc["$schema"],
            Value::String("https://json.schemastore.org/sarif-2.1.0.json".to_string())
        );
        assert_eq!(doc["version"], Value::String("2.1.0".to_string()));
    }

    #[test]
    fn has_exactly_one_run() {
        let doc = render_value(&two_violations());
        assert_eq!(doc["runs"].as_array().expect("array").len(), 1);
    }

    #[test]
    fn driver_name_and_version() {
        let doc = render_value(&two_violations());
        let driver = &doc["runs"][0]["tool"]["driver"];
        assert_eq!(driver["name"], Value::String("arch-lint".to_string()));
        assert_eq!(
            driver["semanticVersion"],
            Value::String(env!("CARGO_PKG_VERSION").to_string())
        );
        assert_eq!(
            driver["informationUri"],
            Value::String("https://github.com/ynishi/arch-lint".to_string())
        );
    }

    #[test]
    fn rules_are_sorted_and_deduplicated() {
        let mut result = two_violations();
        // A second ALD009 violation must not duplicate the rule entry.
        result.violations.push(Violation::new(
            "ALD009",
            "acyclic",
            Severity::Error,
            Location::new(PathBuf::from("src/domain/d.rs"), 9, 1),
            "Cycle between scopes: domain \u{2192} web \u{2192} domain",
        ));

        let doc = render_value(&result);
        let rules = doc["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .expect("array");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["id"], Value::String("AL002".to_string()));
        assert_eq!(rules[0]["name"], Value::String("no-sync-io".to_string()));
        assert_eq!(
            rules[0]["shortDescription"]["text"],
            Value::String("no-sync-io".to_string())
        );
        assert_eq!(rules[1]["id"], Value::String("ALD009".to_string()));
        assert_eq!(rules[1]["name"], Value::String("acyclic".to_string()));
    }

    #[test]
    fn result_fields_match_the_violation() {
        let result = two_violations();
        let doc = render_value(&result);
        let results = doc["runs"][0]["results"].as_array().expect("array");
        assert_eq!(results.len(), 2);

        let first = &results[0];
        assert_eq!(first["ruleId"], Value::String("ALD009".to_string()));
        // "AL002" < "ALD009" lexically, so AL002 is rule index 0 and ALD009 is 1.
        assert_eq!(first["ruleIndex"], Value::from(1));
        assert_eq!(first["level"], Value::String("error".to_string()));
        assert_eq!(
            first["message"]["text"],
            Value::String(
                "Cycle between scopes: domain \u{2192} infra \u{2192} domain".to_string()
            )
        );

        let location = &first["locations"][0]["physicalLocation"];
        assert_eq!(
            location["artifactLocation"]["uri"],
            Value::String("src/domain/a.rs".to_string())
        );
        assert_eq!(
            location["artifactLocation"]["uriBaseId"],
            Value::String("%SRCROOT%".to_string())
        );
        assert_eq!(location["region"]["startLine"], Value::from(1));
        assert_eq!(location["region"]["startColumn"], Value::from(23));

        assert_eq!(
            first["properties"]["severity"],
            Value::String("error".to_string())
        );
        assert_eq!(
            first["properties"]["suggestion"],
            Value::String("Break the cycle".to_string())
        );

        assert_eq!(
            first["partialFingerprints"]["arch-lint/fingerprint/v1"],
            Value::String(baseline::fingerprint(&result.violations[0]))
        );
    }

    #[test]
    fn start_column_omitted_when_zero() {
        let result = two_violations();
        let doc = render_value(&result);
        let related = &doc["runs"][0]["results"][0]["relatedLocations"][1];
        let region = &related["physicalLocation"]["region"];
        assert!(
            region.get("startColumn").is_none(),
            "startColumn should be omitted when 0, got {region}"
        );
    }

    #[test]
    fn two_labels_become_two_related_locations() {
        let doc = render_value(&two_violations());
        let related = doc["runs"][0]["results"][0]["relatedLocations"]
            .as_array()
            .expect("array");
        assert_eq!(related.len(), 2);
        assert_eq!(
            related[0]["message"]["text"],
            Value::String("infra \u{2192} domain".to_string())
        );
        assert_eq!(
            related[0]["physicalLocation"]["artifactLocation"]["uri"],
            Value::String("src/infra/b.rs".to_string())
        );
        assert_eq!(
            related[1]["message"]["text"],
            Value::String("domain \u{2192} infra".to_string())
        );
    }

    #[test]
    fn warning_maps_to_warning_level() {
        let doc = render_value(&two_violations());
        assert_eq!(
            doc["runs"][0]["results"][1]["level"],
            Value::String("warning".to_string())
        );
        assert!(doc["runs"][0]["results"][1]
            .get("relatedLocations")
            .is_none());
        assert!(doc["runs"][0]["results"][1]["properties"]
            .get("suggestion")
            .is_none());
    }

    #[test]
    fn info_maps_to_note_level() {
        let mut result = LintResult::new();
        result.violations.push(Violation::new(
            "AL010",
            "some-info-rule",
            Severity::Info,
            Location::new(PathBuf::from("src/a.rs"), 1, 1),
            "informational",
        ));
        let doc = render_value(&result);
        assert_eq!(
            doc["runs"][0]["results"][0]["level"],
            Value::String("note".to_string())
        );
    }

    #[test]
    fn line_zero_omits_region_but_keeps_artifact_location() {
        // Project-level rules report `Location::new(root, 0, 0)` — see the
        // documented `ProjectRule` example in `arch-lint-core/src/rule.rs`.
        // The SARIF schema requires `region.startLine >= 1`, so `line == 0`
        // must omit `region` entirely rather than emit `0`.
        let mut result = LintResult::new();
        result.violations.push(Violation::new(
            "AL100",
            "require-readme",
            Severity::Warning,
            Location::new(PathBuf::from("."), 0, 0),
            "Project should have a README.md",
        ));
        let doc = render_value(&result);
        let location = &doc["runs"][0]["results"][0]["locations"][0]["physicalLocation"];
        assert!(
            location.get("region").is_none(),
            "region should be omitted when line == 0, got {location}"
        );
        assert_eq!(
            location["artifactLocation"]["uri"],
            Value::String(".".to_string())
        );
    }

    #[test]
    fn empty_result_has_no_rules_or_results() {
        let doc = render_value(&LintResult::new());
        assert_eq!(doc["runs"][0]["results"].as_array().unwrap().len(), 0);
        assert_eq!(
            doc["runs"][0]["tool"]["driver"]["rules"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn rules_table_is_order_independent_of_violation_order() {
        // `rules[]` is built from a `BTreeMap` keyed by `code`, so it (and
        // every `ruleIndex`) must come out the same regardless of which
        // order the violations arrived in — not just because calling
        // `render` twice on the same input is pure.
        let forward = two_violations();
        let mut reversed = LintResult::new();
        reversed.files_checked = forward.files_checked;
        reversed.violations = forward.violations.iter().rev().cloned().collect();

        let doc_forward = render_value(&forward);
        let doc_reversed = render_value(&reversed);

        assert_eq!(
            doc_forward["runs"][0]["tool"]["driver"]["rules"],
            doc_reversed["runs"][0]["tool"]["driver"]["rules"]
        );

        // Each rule id maps to the same `ruleIndex` in both renders, found
        // by matching on `ruleId` rather than assuming position, since the
        // `results[]` order itself is intentionally violation order (and
        // so differs between the two).
        for forward_result in doc_forward["runs"][0]["results"].as_array().unwrap() {
            let rule_id = &forward_result["ruleId"];
            let matching = doc_reversed["runs"][0]["results"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| &r["ruleId"] == rule_id)
                .expect("same ruleId present in the reversed render");
            assert_eq!(forward_result["ruleIndex"], matching["ruleIndex"]);
        }
    }

    #[test]
    fn output_ends_with_a_trailing_newline() {
        let out = render(&two_violations()).expect("render");
        assert!(out.ends_with('\n'));
        assert!(!out.ends_with("\n\n"));
    }

    #[test]
    fn omits_original_uri_base_ids() {
        let doc = render_value(&two_violations());
        assert!(doc["runs"][0].get("originalUriBaseIds").is_none());
    }
}
