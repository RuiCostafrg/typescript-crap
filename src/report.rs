use crate::{
    EvidenceKind, Format, FunctionMutation, FunctionScore, MutationEvidence, MutatorScope,
    RenderOptions, Report, Sort,
};
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

/// Visible rows with their index in `Report::functions`.
fn presentation_rows<'a>(
    report: &'a Report,
    options: &RenderOptions,
) -> Vec<(usize, &'a FunctionScore)> {
    let mut rows: Vec<_> = report
        .functions
        .iter()
        .enumerate()
        .filter(|(_, f)| !options.only_failing || f.failed)
        .collect();
    rows.sort_by(|(_, a), (_, b)| {
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
    let mutation_only = report.failed_by_mutation_only();
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
            for &(index, f) in &rows {
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
                    status(f.failed, mutation_only[index])
                )?;
            }
            writeln!(
                s,
                "Missing coverage: {} files, {} functions",
                report.files_missing_coverage, report.functions_missing_coverage
            )?;
            if !report.syntax_errors.is_empty() {
                writeln!(
                    s,
                    "Syntax errors: {} files not scored (incomplete)",
                    report.syntax_errors.len()
                )?;
                for e in &report.syntax_errors {
                    writeln!(s, "  {}", terminal(&e.message))?;
                }
            }
            if let Some(mutation) = &report.mutation {
                mutation_text(mutation, &rows, &mut s)?;
            }
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
            if let Some(m) = &report.mutation {
                writeln!(
                    s,
                    "  <mutation-evidence tool=\"oximut\" run-complete=\"{}\" mutators=\"{}\" mutator-families=\"{}\" mutants-in-functions=\"{}\" mutants-outside-functions=\"{}\" mutants-outside-selection=\"{}\" files-without-mutants=\"{}\" functions-failed-by-mutation-only=\"{}\"/>",
                    m.run_complete,
                    match m.mutators {
                        MutatorScope::All => "all",
                        MutatorScope::Subset => "subset",
                        _ => "unknown",
                    },
                    xml_attribute(&m.mutator_families.join(",")),
                    m.mutants_in_functions,
                    m.mutants_outside_functions,
                    m.mutants_outside_selection,
                    m.files_without_mutants,
                    m.functions_failed_by_mutation_only
                )?;
            }
            for (index, f) in report.functions.iter().enumerate() {
                write!(
                    s,
                    "  <function path=\"{}\" name=\"{}\" line=\"{}\" column=\"{}\" end-line=\"{}\" complexity=\"{}\" coverage=\"{}\" crap=\"{}\" failed=\"{}\"",
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
                if let Some(m) = report.mutation.as_ref().map(|m| &m.functions[index]) {
                    write!(
                        s,
                        " mutants=\"{}\" killed=\"{}\" survived=\"{}\" no-coverage=\"{}\" inconclusive=\"{}\"",
                        m.mutants,
                        m.killed,
                        m.survived,
                        m.no_coverage,
                        inconclusive(m)
                    )?;
                    if let Some(crap) = m.effective_crap {
                        write!(s, " effective-crap=\"{crap}\"")?;
                    }
                }
                s.write_all(b"/>\n")?;
            }
            for e in &report.syntax_errors {
                writeln!(
                    s,
                    "  <syntax-error path=\"{}\" line=\"{}\" column=\"{}\" message=\"{}\"/>",
                    xml_attribute(&e.path),
                    e.line,
                    e.column,
                    xml_attribute(&e.message)
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
            for &(index, f) in &rows {
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
                    status(f.failed, mutation_only[index])
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
            if let Some(mutation) = &report.mutation {
                mutation_html(mutation, &rows, &mut s)?;
            }
            if !report.syntax_errors.is_empty() {
                write!(
                    s,
                    "<h2 class=\"fail\">Syntax errors: {} files not scored</h2><ul>",
                    report.syntax_errors.len()
                )?;
                for e in &report.syntax_errors {
                    write!(s, "<li><code>{}</code></li>", escape(&e.message))?;
                }
                s.write_all(b"</ul>")?;
            }
            s.write_all(include_bytes!("report-filter.html"))?;
            s.write_all(b"</body></html>\n")?;
        }
    }
    Ok(())
}

fn status(failed: bool, mutation_only: bool) -> &'static str {
    match (failed, mutation_only) {
        (true, true) => "FAIL (mutation)",
        (true, false) => "FAIL",
        (false, _) => "PASS",
    }
}

fn mutators(mutation: &MutationEvidence) -> String {
    match mutation.mutators {
        MutatorScope::All => "all families".to_owned(),
        MutatorScope::Subset => format!(
            "subset {} (partial evidence: other families were not run)",
            mutation.mutator_families.join(",")
        ),
        _ => "unknown (partial evidence: the report does not name its families)".to_owned(),
    }
}

fn mutation_heading(mutation: &MutationEvidence) -> String {
    format!(
        "Mutation evidence (oximut): {} mutants in functions, {} outside functions, {} in unselected files; {} selected files have no mutants. Mutators: {}{}",
        mutation.mutants_in_functions,
        mutation.mutants_outside_functions,
        mutation.mutants_outside_selection,
        mutation.files_without_mutants,
        mutators(mutation),
        if mutation.run_complete {
            ""
        } else {
            "; incomplete run: inconclusive mutants count neither way"
        }
    )
}

fn unkilled(f: &FunctionMutation) -> bool {
    f.survived + f.no_coverage > 0
}

fn inconclusive(f: &FunctionMutation) -> usize {
    f.mutants - f.killed - f.survived - f.no_coverage
}

fn percent(value: Option<f64>) -> String {
    value.map_or("n/a".to_owned(), |v| format!("{:.1}%", v * 100.0))
}

fn effective_crap(f: &FunctionMutation) -> String {
    f.effective_crap
        .map_or("n/a".to_owned(), |crap| format!("{crap:.3}"))
}

/// Visible functions with unkilled mutants; the JSON report lists every function.
fn mutation_text(
    mutation: &MutationEvidence,
    rows: &[(usize, &FunctionScore)],
    mut s: impl Write,
) -> Result<()> {
    writeln!(s, "{}", terminal(&mutation_heading(mutation)))?;
    let mut heading = true;
    for &(index, _) in rows {
        let f = &mutation.functions[index];
        if !unkilled(f) {
            continue;
        }
        if heading {
            s.write_all(
                b"PATH:LINE:COLUMN  FUNCTION  KILLED  SURVIVED  NO-COVERAGE  INCONCLUSIVE  EFFECTIVE-COVERAGE  EFFECTIVE-CRAP  STATUS\n",
            )?;
            heading = false;
        }
        writeln!(
            s,
            "{}:{}:{}  {}  {}  {}  {}  {}  {}  {}  {}",
            terminal(&f.path),
            f.line,
            f.column,
            terminal(&f.name),
            f.killed,
            f.survived,
            f.no_coverage,
            inconclusive(f),
            percent(f.effective_coverage),
            effective_crap(f),
            if f.failed { "FAIL" } else { "PASS" }
        )?;
    }
    let none = mutation
        .functions
        .iter()
        .filter(|f| f.evidence == EvidenceKind::None)
        .count();
    writeln!(
        s,
        "Mutation gate: {} functions pass on CRAP and fail the mutation gate; {} functions have no mutants",
        mutation.functions_failed_by_mutation_only, none
    )?;
    Ok(())
}

fn mutation_html(
    mutation: &MutationEvidence,
    rows: &[(usize, &FunctionScore)],
    mut s: impl Write,
) -> Result<()> {
    write!(
        s,
        "<h2>Mutation evidence</h2><p>{}</p><p>The mutation gate fails on any surviving or uncovered mutant. Effective coverage and effective CRAP are informational estimates. {} functions pass on CRAP and fail the mutation gate.</p><table><caption>Functions with surviving or uncovered mutants</caption><thead><tr><th>Relative path : line : column</th><th>Function</th><th>Killed</th><th>Survived</th><th>No coverage</th><th>Inconclusive</th><th>Effective coverage</th><th>Effective CRAP</th><th>Gate</th></tr></thead><tbody>",
        escape(&mutation_heading(mutation)),
        mutation.functions_failed_by_mutation_only
    )?;
    for &(index, _) in rows {
        let f = &mutation.functions[index];
        if !unkilled(f) {
            continue;
        }
        write!(
            s,
            "<tr><td><code>{}:{}:{}</code></td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"{}\">{}</td></tr>",
            escape(&f.path),
            f.line,
            f.column,
            escape(&f.name),
            f.killed,
            f.survived,
            f.no_coverage,
            inconclusive(f),
            percent(f.effective_coverage),
            effective_crap(f),
            if f.failed { "fail" } else { "pass" },
            if f.failed { "FAIL" } else { "PASS" }
        )?;
    }
    s.write_all(b"</tbody></table>")?;
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
