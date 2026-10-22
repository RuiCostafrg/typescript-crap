use anyhow::{Context, Result, bail};
use same_file::Handle;
use std::{io::Write, path::Path};

// Include declarations and case variants too: output is never a TS destination.
fn typescript(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        let name = name.to_string_lossy().to_ascii_lowercase();
        [".ts", ".tsx", ".mts", ".cts"]
            .iter()
            .any(|extension| name.ends_with(extension))
    })
}

pub(crate) fn validate(root: &Path, path: &Path, coverage: Option<&Path>) -> Result<()> {
    if typescript(path) {
        bail!("report output cannot overwrite a TypeScript source destination");
    }
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    // Never follow an output symlink, even if dangling or its name hides TS.
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("report output must be a regular file, not a symbolic link or directory");
    }
    let destination = Handle::from_path(path)?;
    if let Some(coverage) = coverage
        && destination == Handle::from_path(coverage)?
    {
        bail!("report output cannot overwrite coverage input");
    }
    // Selection and ignore rules must not hide an unselected source alias.
    // Directory symlinks are not followed, matching source-selection policy.
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false);
    for entry in builder.build() {
        let entry = entry.context("cannot check report output against project sources")?;
        if typescript(entry.path())
            && entry.path().is_file()
            && destination == Handle::from_path(entry.path())?
        {
            bail!("report output cannot overwrite a source file");
        }
    }
    Ok(())
}

pub fn write_report(
    root: &Path,
    path: &Path,
    coverage: Option<&Path>,
    rendered: &str,
) -> Result<()> {
    let write = || -> Result<()> {
        validate(root, path, coverage)?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(rendered.as_bytes())?;
        temporary.as_file().sync_all()?;
        // Recheck after writing; persist replaces the directory entry atomically
        // rather than opening/truncating a destination (or following its links).
        validate(root, path, coverage)?;
        temporary.persist(path)?;
        Ok(())
    };
    write().with_context(|| format!("cannot write report {}", path.display()))
}

#[cfg(test)]
mod tests;
