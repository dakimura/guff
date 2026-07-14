//! SARIF 2.1.0 output — golangci-lint `pkg/printers/sarif.go`.

use std::io::{self, Write};

use serde::Serialize;

use crate::exclude::Issue;

use super::severity::SARIF;
use super::Formatter;

const SARIF_VERSION: &str = "2.1.0";
const SARIF_SCHEMA: &str =
    "https://schemastore.azurewebsites.net/schemas/json/sarif-2.1.0-rtm.6.json";

/// SARIF formatter. Tool driver name is `guff` (golangci uses `golangci-lint`).
#[derive(Debug, Default, Clone)]
pub struct SarifFormatter;

impl SarifFormatter {
    pub fn new() -> Self {
        Self
    }
}

impl Formatter for SarifFormatter {
    fn name(&self) -> &'static str {
        "sarif"
    }

    fn print(&self, issues: &[Issue], w: &mut dyn Write) -> io::Result<()> {
        let results: Vec<SarifResult> = issues
            .iter()
            .map(|issue| {
                let start_column = if issue.column > 0 { issue.column } else { 1 };
                SarifResult {
                    rule_id: issue.from_linter.clone(),
                    level: SARIF.sanitize(&issue.severity).to_string(),
                    message: SarifMessage {
                        text: issue.text.clone(),
                    },
                    locations: vec![SarifLocation {
                        physical_location: SarifPhysicalLocation {
                            artifact_location: SarifArtifactLocation {
                                uri: issue.filename.clone(),
                                index: 0,
                            },
                            region: SarifRegion {
                                start_line: issue.line,
                                start_column,
                            },
                        },
                    }],
                }
            })
            .collect();

        let mut run = SarifRun {
            tool: SarifTool {
                driver: SarifDriver {
                    name: "guff".into(),
                },
            },
            results,
        };
        // golangci always emits `"results":[]` (never omit) for empty.
        if run.results.is_empty() {
            run.results = Vec::new();
        }

        let output = SarifOutput {
            version: SARIF_VERSION.into(),
            schema: SARIF_SCHEMA.into(),
            runs: vec![run],
        };

        serde_json::to_writer(&mut *w, &output).map_err(io::Error::other)?;
        writeln!(w)?;
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct SarifOutput {
    version: String,
    #[serde(rename = "$schema")]
    schema: String,
    runs: Vec<SarifRun>,
}

#[derive(Debug, Serialize)]
struct SarifRun {
    tool: SarifTool,
    results: Vec<SarifResult>,
}

#[derive(Debug, Serialize)]
struct SarifTool {
    driver: SarifDriver,
}

#[derive(Debug, Serialize)]
struct SarifDriver {
    name: String,
}

#[derive(Debug, Serialize)]
struct SarifResult {
    #[serde(rename = "ruleId")]
    rule_id: String,
    level: String,
    message: SarifMessage,
    locations: Vec<SarifLocation>,
}

#[derive(Debug, Serialize)]
struct SarifMessage {
    text: String,
}

#[derive(Debug, Serialize)]
struct SarifLocation {
    #[serde(rename = "physicalLocation")]
    physical_location: SarifPhysicalLocation,
}

#[derive(Debug, Serialize)]
struct SarifPhysicalLocation {
    #[serde(rename = "artifactLocation")]
    artifact_location: SarifArtifactLocation,
    region: SarifRegion,
}

#[derive(Debug, Serialize)]
struct SarifArtifactLocation {
    uri: String,
    index: i64,
}

#[derive(Debug, Serialize)]
struct SarifRegion {
    #[serde(rename = "startLine")]
    start_line: i64,
    #[serde(rename = "startColumn")]
    start_column: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::Diagnostic;
    use serde_json::Value;

    fn sample(
        from: &str,
        text: &str,
        file: &str,
        line: i64,
        col: i64,
        severity: &str,
    ) -> Issue {
        Issue {
            from_linter: from.into(),
            analyzer: from.into(),
            text: text.into(),
            severity: severity.into(),
            filename: file.into(),
            line,
            column: col,
            source_line: None,
            diagnostic: Diagnostic {
                message: text.into(),
                ..Diagnostic::default()
            },
        }
    }

    #[test]
    fn sarif_matches_golangci_key_structure() {
        let issues = vec![
            sample("linter-a", "some issue", "path/to/filea.go", 10, 4, "warning"),
            sample("linter-b", "another issue", "path/to/fileb.go", 300, 9, "error"),
            sample(
                "linter-c",
                "some issue without column",
                "path/to/filed.go",
                11,
                0,
                "error",
            ),
            sample("linter-c", "without severity", "path/to/filec.go", 300, 10, ""),
            sample("linter-d", "unknown severity", "path/to/filed.go", 300, 11, "foo"),
        ];

        let mut buf = Vec::new();
        SarifFormatter::new().print(&issues, &mut buf).unwrap();
        let v: Value = serde_json::from_slice(&buf).unwrap();

        assert_eq!(v["version"], "2.1.0");
        assert_eq!(
            v["$schema"],
            "https://schemastore.azurewebsites.net/schemas/json/sarif-2.1.0-rtm.6.json"
        );
        assert_eq!(v["runs"][0]["tool"]["driver"]["name"], "guff");

        let results = v["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 5);
        assert_eq!(results[0]["ruleId"], "linter-a");
        assert_eq!(results[0]["level"], "warning");
        assert_eq!(results[0]["message"]["text"], "some issue");
        assert_eq!(
            results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "path/to/filea.go"
        );
        assert_eq!(
            results[0]["locations"][0]["physicalLocation"]["region"]["startLine"],
            10
        );
        assert_eq!(
            results[0]["locations"][0]["physicalLocation"]["region"]["startColumn"],
            4
        );
        // Column 0 → default 1.
        assert_eq!(
            results[2]["locations"][0]["physicalLocation"]["region"]["startColumn"],
            1
        );
        assert_eq!(results[3]["level"], "error");
        assert_eq!(results[4]["level"], "error");
    }

    #[test]
    fn empty_emits_empty_results_array() {
        let mut buf = Vec::new();
        SarifFormatter::new().print(&[], &mut buf).unwrap();
        let v: Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(v["runs"][0]["results"], serde_json::json!([]));
    }
}
