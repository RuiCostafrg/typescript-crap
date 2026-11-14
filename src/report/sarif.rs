//! Borrow report identities and serialize findings one at a time.
use crate::{FunctionScore, Report};
use serde::{Serialize, Serializer, ser::SerializeSeq};
use std::io::Write;

#[derive(Serialize)]
struct Document<'a> {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: &'static str,
    runs: [Run<'a>; 1],
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Run<'a> {
    column_kind: &'static str,
    tool: Tool,
    results: Results<'a>,
}
#[derive(Serialize)]
struct Tool {
    driver: Driver,
}
#[derive(Serialize)]
struct Driver {
    name: &'static str,
    rules: Vec<Rule>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Rule {
    id: &'static str,
    short_description: Message<'static>,
}
#[derive(Serialize)]
struct Message<'a> {
    text: std::borrow::Cow<'a, str>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Finding<'a> {
    rule_id: &'static str,
    level: &'static str,
    message: Message<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    partial_fingerprints: Option<Fingerprints<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    locations: Option<[Location; 1]>,
}
#[derive(Serialize)]
struct Fingerprints<'a> {
    #[serde(rename = "function/v1")]
    function: &'a str,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Location {
    physical_location: PhysicalLocation,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PhysicalLocation {
    artifact_location: ArtifactLocation,
    region: Region,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactLocation {
    uri: String,
    uri_base_id: &'static str,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Region {
    start_line: usize,
    start_column: usize,
    end_line: usize,
}

struct Results<'a>(&'a Report);
impl Serialize for Results<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let report = self.0;
        let mut sequence = serializer.serialize_seq(None)?;
        let mutation = report.mutation.as_ref().map(|m| &m.functions);
        for (index, f) in report.functions.iter().enumerate() {
            if f.failed {
                let evidence = mutation.map(|m| &m[index]).filter(|m| m.failed);
                sequence.serialize_element(&finding(f, report.threshold, evidence))?;
            }
        }
        // Each unparsable file is located missing evidence under the completeness rule.
        for e in &report.syntax_errors {
            sequence.serialize_element(&Finding {
                rule_id: "TSCRAP002",
                level: "error",
                message: Message {
                    text: format!("Not scored: {}", e.message).into(),
                },
                partial_fingerprints: None,
                locations: Some([Location {
                    physical_location: PhysicalLocation {
                        artifact_location: ArtifactLocation {
                            uri: uri(&e.path),
                            uri_base_id: "%SRCROOT%",
                        },
                        region: Region {
                            start_line: e.line,
                            start_column: e.column_utf16,
                            end_line: e.line,
                        },
                    },
                }]),
            })?;
        }
        if report.completeness_failed {
            sequence.serialize_element(&Finding {
                rule_id: "TSCRAP002", level: "error",
                message: Message { text: format!("Incomplete selection: {} files and {} functions missing coverage; {} functions selected", report.files_missing_coverage, report.functions_missing_coverage, report.functions.len()).into() },
                partial_fingerprints: None, locations: None,
            })?;
        }
        sequence.end()
    }
}
/// Relative percent-encoded URI for a report path.
fn uri(path: &str) -> String {
    use std::fmt::Write;
    let mut uri = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    uri
}
fn finding<'a>(
    f: &'a FunctionScore,
    threshold: f64,
    mutation: Option<&crate::FunctionMutation>,
) -> Finding<'a> {
    let uri = uri(&f.path);
    let text = match mutation {
        Some(m) => format!(
            "{} has {} surviving and {} uncovered mutants; mutation gate failed (CRAP {:.3}, threshold {})",
            f.name, m.survived, m.no_coverage, f.crap, threshold
        ),
        None => format!(
            "{} has CRAP {:.3} (threshold {})",
            f.name, f.crap, threshold
        ),
    };
    Finding {
        rule_id: if mutation.is_some() {
            "TSCRAP003"
        } else {
            "TSCRAP001"
        },
        level: "error",
        message: Message { text: text.into() },
        partial_fingerprints: Some(Fingerprints {
            function: &f.identity,
        }),
        locations: Some([Location {
            physical_location: PhysicalLocation {
                artifact_location: ArtifactLocation {
                    uri,
                    uri_base_id: "%SRCROOT%",
                },
                region: Region {
                    start_line: f.line,
                    start_column: f.column_utf16,
                    end_line: f.end_line,
                },
            },
        }]),
    }
}
pub(super) fn write(report: &Report, mut writer: impl Write) -> anyhow::Result<()> {
    let mut rules: Vec<_> = [
        ("TSCRAP001", "CRAP threshold exceeded"),
        ("TSCRAP002", "Missing quality evidence"),
    ]
    .map(|(id, text)| Rule {
        id,
        short_description: Message { text: text.into() },
    })
    .into_iter()
    .collect();
    if report.mutation.is_some() {
        rules.push(Rule {
            id: "TSCRAP003",
            short_description: Message {
                text: "Surviving or uncovered mutants".into(),
            },
        });
    }
    let document = Document {
        schema: "https://json.schemastore.org/sarif-2.1.0.json",
        version: "2.1.0",
        runs: [Run {
            column_kind: "utf16CodeUnits",
            tool: Tool {
                driver: Driver {
                    name: "ts-crap",
                    rules,
                },
            },
            results: Results(report),
        }],
    };
    serde_json::to_writer_pretty(&mut writer, &document)?;
    writer.write_all(b"\n")?;
    Ok(())
}
