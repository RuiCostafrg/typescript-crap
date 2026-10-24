use anyhow::{Context, Result, bail};
use ignore::WalkBuilder;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::Command,
};

mod committed;
pub(crate) use committed::BlobReader;

#[derive(Debug)]
pub(crate) struct Selection {
    pub paths: Vec<PathBuf>,
    pub committed: BTreeMap<PathBuf, String>,
}

fn supported(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    [".ts", ".tsx", ".mts", ".cts"]
        .iter()
        .any(|ext| name.ends_with(ext))
        && ![".d.ts", ".d.mts", ".d.cts"]
            .iter()
            .any(|ext| name.ends_with(ext))
}

fn excluded(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .any(|part| {
            matches!(
                part.as_os_str().to_str(),
                Some("node_modules" | ".git" | "target" | "dist" | "build" | "coverage")
            )
        })
}

fn checked(path: &Path, root: &Path) -> Result<PathBuf> {
    if path.symlink_metadata()?.file_type().is_symlink() {
        bail!("symbolic link source is not supported: {}", path.display());
    }
    let absolute = path
        .canonicalize()
        .with_context(|| format!("missing source {}", path.display()))?;
    if !absolute.starts_with(root) {
        bail!("source is outside project root: {}", path.display());
    }
    Ok(absolute)
}

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .context("cannot execute git")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out.stdout)
}

fn oid(root: &Path, reference: &str) -> Result<String> {
    let data = git(
        root,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )?;
    Ok(String::from_utf8(data)?.trim().to_string())
}

fn diff_files(root: &Path, reference: &str) -> Result<BTreeMap<PathBuf, String>> {
    let reference = oid(root, reference)?;
    let head = oid(root, "HEAD")?;
    let data = git(root, &["merge-base", &head, &reference])?;
    let base = String::from_utf8(data)?.trim().to_string();
    let bytes = git(
        root,
        &[
            "diff",
            "--raw",
            "--no-abbrev",
            "--no-renames",
            "--no-relative",
            "-z",
            "--diff-filter=ACMRT",
            &base,
            &head,
            "--",
        ],
    )?;
    let top = String::from_utf8(git(root, &["rev-parse", "--show-toplevel"])?)?;
    let top = PathBuf::from(top.trim_end()).canonicalize()?;
    let mut result = BTreeMap::new();
    // With rename detection disabled, raw -z records are metadata/path pairs.
    // Object IDs are safe batch requests even when filenames contain newlines.
    let mut records = bytes.split(|b| *b == 0).filter(|s| !s.is_empty());
    while let Some(metadata) = records.next() {
        let name = records.next().context("Git diff omitted source path")?;
        let blob = std::str::from_utf8(metadata)?
            .split_whitespace()
            .nth(3)
            .context("Git diff omitted HEAD object ID")?;
        let path = top.join(std::str::from_utf8(name).context("Git source path is not UTF-8")?);
        if path.starts_with(root) && supported(&path) && !excluded(&path, root) {
            // A committed file absent from the worktree is stale, not a passing empty scan.
            result.insert(
                checked(&path, root)
                    .with_context(|| format!("changed source unavailable: {}", path.display()))?,
                blob.to_owned(),
            );
        }
    }
    Ok(result)
}

fn pr_base(root: &Path, pr: &str) -> Result<String> {
    if pr.starts_with('-') {
        bail!("invalid PR number/URL");
    }
    let mut command = Command::new("gh");
    command.current_dir(root).args(["pr", "view"]);
    if !pr.is_empty() {
        command.arg(pr);
    }
    let output = command
        .args(["--json", "baseRefOid,headRefOid"])
        .output()
        .context("cannot execute gh; use --diff BASE for offline scans")?;
    if !output.status.success() {
        bail!(
            "gh PR lookup failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let base = value["baseRefOid"]
        .as_str()
        .context("PR has no base commit")?;
    let head = value["headRefOid"]
        .as_str()
        .context("PR has no head commit")?;
    if oid(root, "HEAD")? != head {
        bail!("checkout HEAD does not match PR head; checkout the PR before scanning");
    }
    // Explicitly validate object availability; no implicit network fetch or checkout.
    oid(root, base).context("PR base commit is not available locally; fetch the PR base first")
}

pub fn files(
    root: &Path,
    requested: &[PathBuf],
    diff: Option<&str>,
    pr: Option<&str>,
) -> Result<Selection> {
    // Git's tracked change set is authoritative in diff mode. A directory walk
    // would silently omit tracked ignored/hidden files and could hide gate failures.
    let changed = if let Some(pr) = pr {
        Some(diff_files(root, &pr_base(root, pr)?)?)
    } else {
        diff.map(|base| diff_files(root, base)).transpose()?
    };
    let mut all = BTreeSet::new();
    let default = vec![root.to_path_buf()];
    for input in if requested.is_empty() {
        &default
    } else {
        requested
    } {
        let path = if input.is_absolute() {
            input.clone()
        } else {
            root.join(input)
        };
        let path =
            checked(&path, root).with_context(|| format!("cannot select {}", input.display()))?;
        if path.is_file() {
            if !supported(&path) {
                bail!("not a supported TypeScript source: {}", path.display());
            }
            if excluded(&path, root) {
                bail!("source is in an excluded directory: {}", path.display());
            }
            all.insert(path);
        } else if path.is_dir() {
            if let Some(changed) = &changed {
                all.extend(
                    changed
                        .keys()
                        .filter(|file| file.starts_with(&path))
                        .cloned(),
                );
                continue;
            }
            let mut builder = WalkBuilder::new(&path);
            builder
                .require_git(false)
                .follow_links(false)
                .filter_entry({
                    let root = root.to_path_buf();
                    move |entry| !excluded(entry.path(), &root)
                });
            for entry in builder.build() {
                let entry = entry.context("cannot walk source directory")?;
                if entry.file_type().is_some_and(|t| t.is_file()) && supported(entry.path()) {
                    all.insert(checked(entry.path(), root)?);
                }
            }
        } else {
            bail!("not a regular source or directory: {}", path.display());
        }
    }
    let committed = if let Some(mut changed) = changed {
        all.retain(|path| changed.contains_key(path));
        changed.retain(|path, _| all.contains(path));
        changed
    } else {
        BTreeMap::new()
    };
    Ok(Selection {
        paths: all.into_iter().collect(),
        committed,
    })
}
