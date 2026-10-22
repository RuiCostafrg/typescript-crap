use crate::{Format, Report};
use anyhow::Result;
use std::fmt::Write;

pub fn terminal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_control()
            || matches!(c, '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
            out.extend(c.escape_unicode());
        } else {
            out.push(c);
        }
    }
    out
}

fn escape(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            c if c.is_control() && !matches!(c, '\n' | '\r' | '\t') => escaped.push('\u{fffd}'),
            c => escaped.push(c),
        }
    }
    escaped
}

pub fn render(report: &Report, format: Format) -> Result<String> {
    Ok(match format {
        Format::Sarif => {
            let results: Vec<_> = report.functions.iter().filter(|f| f.failed).map(|f| {
                // URI encode UTF-8 bytes, including percent, fragments and path controls.
                let mut uri = String::new();
                for byte in f.path.bytes() {
                    if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) { uri.push(char::from(byte)); }
                    else { write!(uri, "%{byte:02X}")?; }
                }
                Ok(serde_json::json!({"ruleId":"TSCRAP001", "level":"error",
                    "message":{"text":format!("{} has CRAP {:.3} (threshold {})", f.name, f.crap, report.threshold)},
                    "partialFingerprints":{"function/v1":f.identity},
                    "locations":[{"physicalLocation":{"artifactLocation":{"uri":uri,"uriBaseId":"%SRCROOT%"},
                        "region":{"startLine":f.line,"startColumn":f.column_utf16,"endLine":f.end_line}}}]}))
            }).collect::<Result<Vec<_>>>()?;
            let mut results = results;
            if report.completeness_failed {
                results.push(serde_json::json!({"ruleId":"TSCRAP002","level":"error",
                "message":{"text":format!("Incomplete selection: {} files and {} functions missing coverage; {} functions selected", report.files_missing_coverage, report.functions_missing_coverage, report.functions.len())}}));
            }
            format!(
                "{}\n",
                serde_json::to_string_pretty(&serde_json::json!({
                    "$schema":"https://json.schemastore.org/sarif-2.1.0.json", "version":"2.1.0",
                    "runs":[{"columnKind":"utf16CodeUnits","tool":{"driver":{"name":"ts-crap","rules":[
                        {"id":"TSCRAP001","shortDescription":{"text":"CRAP threshold exceeded"}},
                        {"id":"TSCRAP002","shortDescription":{"text":"Missing quality evidence"}}]}}, "results":results}]
                }))?
            )
        }
        Format::Json => format!("{}\n", serde_json::to_string_pretty(report)?),
        Format::Text => {
            let mut s = format!(
                "CRAP gate: {} (threshold {}, {} files, {} functions)\nPATH:LINE:COLUMN  FUNCTION  CC  COVERAGE  CRAP  STATUS\n",
                if report.failed { "FAIL" } else { "PASS" },
                report.threshold,
                report.files_scanned,
                report.functions.len()
            );
            for f in &report.functions {
                writeln!(
                    s,
                    "{}:{}:{}  {}  {}  {:.1}%  {:.3}  {}",
                    terminal(&f.path),
                    f.line,
                    f.column,
                    terminal(&f.name),
                    f.complexity,
                    f.coverage * 100.0,
                    f.crap,
                    if f.failed { "FAIL" } else { "PASS" }
                )?;
            }
            writeln!(
                s,
                "Missing coverage: {} files, {} functions",
                report.files_missing_coverage, report.functions_missing_coverage
            )?;
            if report.functions.is_empty() {
                s.push_str("No implemented TypeScript functions found in the selection.\n");
            }
            s
        }
        Format::Xml => {
            let mut s = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<crap-report version=\"1\" threshold=\"{}\" files-scanned=\"{}\" failed=\"{}\" files-missing-coverage=\"{}\" functions-missing-coverage=\"{}\" completeness-failed=\"{}\">\n",
                report.threshold,
                report.files_scanned,
                report.failed,
                report.files_missing_coverage,
                report.functions_missing_coverage,
                report.completeness_failed
            );
            for f in &report.functions {
                writeln!(
                    s,
                    "  <function path=\"{}\" name=\"{}\" line=\"{}\" column=\"{}\" end-line=\"{}\" complexity=\"{}\" coverage=\"{}\" crap=\"{}\" failed=\"{}\"/>",
                    escape(&f.path),
                    escape(&f.name),
                    f.line,
                    f.column,
                    f.end_line,
                    f.complexity,
                    f.coverage,
                    f.crap,
                    f.failed
                )?;
            }
            s.push_str("</crap-report>\n");
            s
        }
        Format::Html => {
            let mut s = format!(
                "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>TypeScript CRAP report</title><style>body{{font:16px system-ui;margin:2rem;color:#17243a;background:#f4f7fb}}table{{border-collapse:collapse;background:white;width:100%}}th,td{{padding:.7rem;text-align:left;border-bottom:1px solid #ccd3dd}}.fail{{color:#b42318}}.pass{{color:#067647}}code{{overflow-wrap:anywhere}}caption{{text-align:left;margin-bottom:1rem}}@media(max-width:700px){{body{{margin:.5rem}}td,th{{padding:.3rem}}}}</style></head><body><h1>TypeScript CRAP report</h1><p class=\"{}\"><strong>{}</strong> · Threshold {} · {} files · {} functions</p><p>CRAP = CC² × (1 − coverage)³ + CC. Missing coverage is 0%. A score strictly above the threshold fails.</p><table><caption>Function quality gate results</caption><thead><tr><th>Relative path : line : column</th><th>Function</th><th>CC</th><th>Coverage</th><th>CRAP</th><th>Gate</th></tr></thead><tbody>",
                if report.failed { "fail" } else { "pass" },
                if report.failed { "FAIL" } else { "PASS" },
                report.threshold,
                report.files_scanned,
                report.functions.len()
            );
            for f in &report.functions {
                write!(
                    s,
                    "<tr><td><code>{}:{}:{}</code></td><td><code>{}</code></td><td>{}</td><td>{:.1}%</td><td>{:.3}</td><td class=\"{}\">{}</td></tr>",
                    escape(&f.path),
                    f.line,
                    f.column,
                    escape(&f.name),
                    f.complexity,
                    f.coverage * 100.0,
                    f.crap,
                    if f.failed { "fail" } else { "pass" },
                    if f.failed { "FAIL" } else { "PASS" }
                )?;
            }
            s.push_str("</tbody></table></body></html>\n");
            s
        }
    })
}
