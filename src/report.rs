use crate::{Format, FunctionScore, RenderOptions, Report, Sort};
use anyhow::Result;
use std::io::Write;
mod sarif;

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

fn presentation_rows<'a>(report: &'a Report, options: &RenderOptions) -> Vec<&'a FunctionScore> {
    let mut rows: Vec<_> = report
        .functions
        .iter()
        .filter(|f| !options.only_failing || f.failed)
        .collect();
    rows.sort_by(|a, b| {
        let location = (&a.path, a.line, a.column).cmp(&(&b.path, b.line, b.column));
        match options.sort {
            Sort::Path => location,
            Sort::Crap => b.crap.total_cmp(&a.crap).then(location),
        }
    });
    if let Some(top) = options.top {
        rows.truncate(top);
    }
    rows
}

pub fn render_to<W: Write>(
    report: &Report,
    format: Format,
    options: &RenderOptions,
    mut s: W,
) -> Result<()> {
    match format {
        Format::Sarif => {
            sarif::write(report, &mut s)?;
        }
        Format::Json => {
            serde_json::to_writer_pretty(&mut s, report)?;
            s.write_all(b"\n")?;
        }
        Format::Text => {
            let rows = presentation_rows(report, options);
            write!(
                s,
                "CRAP gate: {} (threshold {}, {} files, {} functions)\nPATH:LINE:COLUMN  FUNCTION  CC  COVERAGE  CRAP  STATUS\n",
                if report.failed { "FAIL" } else { "PASS" },
                report.threshold,
                report.files_scanned,
                report.functions.len()
            )?;
            for f in &rows {
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
                s.write_all(b"No implemented TypeScript functions found in the selection.\n")?;
            }
            if rows.len() != report.functions.len() {
                writeln!(
                    s,
                    "Showing {} of {} functions; gate uses all functions.",
                    rows.len(),
                    report.functions.len()
                )?;
            }
        }
        Format::Xml => {
            write!(
                s,
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<crap-report version=\"1\" threshold=\"{}\" files-scanned=\"{}\" failed=\"{}\" files-missing-coverage=\"{}\" functions-missing-coverage=\"{}\" completeness-failed=\"{}\">\n",
                report.threshold,
                report.files_scanned,
                report.failed,
                report.files_missing_coverage,
                report.functions_missing_coverage,
                report.completeness_failed
            )?;
            for f in &report.functions {
                writeln!(
                    s,
                    "  <function path=\"{}\" name=\"{}\" line=\"{}\" column=\"{}\" end-line=\"{}\" complexity=\"{}\" coverage=\"{}\" crap=\"{}\" failed=\"{}\"/>",
                    xml_attribute(&f.path),
                    xml_attribute(&f.name),
                    f.line,
                    f.column,
                    f.end_line,
                    f.complexity,
                    f.coverage,
                    f.crap,
                    f.failed
                )?;
            }
            s.write_all(b"</crap-report>\n")?;
        }
        Format::Html => {
            let rows = presentation_rows(report, options);
            write!(
                s,
                "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>TypeScript CRAP report</title><style>body{{font:16px system-ui;margin:2rem;color:#17243a;background:#f4f7fb}}table{{border-collapse:collapse;background:white;width:100%}}th,td{{padding:.7rem;text-align:left;border-bottom:1px solid #ccd3dd}}.fail{{color:#b42318}}.pass{{color:#067647}}code{{overflow-wrap:anywhere}}caption{{text-align:left;margin-bottom:1rem}}@media(max-width:700px){{body{{margin:.5rem}}td,th{{padding:.3rem}}}}</style></head><body><h1>TypeScript CRAP report</h1><p class=\"{}\"><strong>{}</strong> · Threshold {} · {} files · {} functions</p><p>CRAP = CC² × (1 − coverage)³ + CC. Missing coverage is 0%. A score strictly above the threshold fails.</p><label>Filter path or function <input id=\"filter\" type=\"search\"></label> <label><input id=\"only-failing\" type=\"checkbox\"> Only failing</label><p id=\"visible-count\" aria-live=\"polite\"></p><table><caption>Function quality gate results</caption><thead><tr><th>Relative path : line : column</th><th>Function</th><th>CC</th><th>Coverage</th><th>CRAP</th><th>Gate</th></tr></thead><tbody>",
                if report.failed { "fail" } else { "pass" },
                if report.failed { "FAIL" } else { "PASS" },
                report.threshold,
                report.files_scanned,
                report.functions.len()
            )?;
            for f in &rows {
                write!(
                    s,
                    "<tr data-failed=\"{}\"><td><code>{}:{}:{}</code></td><td><code>{}</code></td><td>{}</td><td>{:.1}%</td><td>{:.3}</td><td class=\"{}\">{}</td></tr>",
                    f.failed,
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
            write!(
                s,
                "</tbody></table><p>Showing {} of {} functions; gate uses all functions.</p><p>Missing coverage: {} files, {} functions.</p>",
                rows.len(),
                report.functions.len(),
                report.files_missing_coverage,
                report.functions_missing_coverage
            )?;
            s.write_all(include_bytes!("report-filter.html"))?;
            s.write_all(b"</body></html>\n")?;
        }
    }
    Ok(())
}

fn xml_attribute(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' => out.push_str("&#x9;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}' => {
                out.push(c)
            }
            _ => out.push('\u{fffd}'),
        }
    }
    out
}
