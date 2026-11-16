use anyhow::{Context, Result, bail, ensure};
use same_file::Handle;
use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
};

// Include declarations and case variants too: output is never a TS destination.
fn typescript(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        let name = name.as_encoded_bytes();
        [".ts", ".tsx", ".mts", ".cts"].iter().any(|extension| {
            name.len()
                .checked_sub(extension.len())
                .and_then(|start| name.get(start..))
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(extension.as_bytes()))
        })
    })
}

/// Existing destinations, identified for the unfiltered source walk.
///
/// The handles stay open for the whole check, so no destination inode can be
/// reused while it runs. On Unix each destination also keeps the device and
/// inode from the same `lstat` that proved it a regular file. Walked regular
/// files are compared by `lstat` as well: one method on both sides, because a
/// path `lstat` and a handle `fstat` can disagree on overlayfs.
#[derive(Default)]
struct Destinations {
    handles: Vec<Handle>,
    #[cfg(unix)]
    inodes: Vec<(u64, u64)>,
}

impl Destinations {
    fn push(&mut self, handle: Handle, metadata: &std::fs::Metadata) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            self.inodes.push((metadata.dev(), metadata.ino()));
        }
        #[cfg(not(unix))]
        let _ = metadata;
        self.handles.push(handle);
    }

    /// Whether a TypeScript-named project entry is one of the destinations.
    fn aliased_by(&self, path: &Path) -> Result<bool> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let Ok(metadata) = path.symlink_metadata() {
                // A regular file is identified without opening it, so an
                // unreadable source can no longer abort publication.
                if metadata.is_file() {
                    return Ok(self.inodes.contains(&(metadata.dev(), metadata.ino())));
                }
                if !metadata.file_type().is_symlink() {
                    return Ok(false);
                }
            }
        }
        // Links, and entries `lstat` could not describe, keep the full check.
        Ok(path.is_file() && self.handles.contains(&Handle::from_path(path)?))
    }
}

// Resolve existing ancestors too, so two absent destinations through different
// directory aliases still conflict. No filesystem identity is cached between checks.
fn location(path: &Path) -> Result<PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let absolute = std::path::absolute(path)?;
            let parent = absolute.parent().context("output has no parent")?;
            let name = absolute.file_name().context("output has no filename")?;
            Ok(location(parent)?.join(name))
        }
        Err(error) => Err(error.into()),
    }
}

/// CLI-owned publication policy for every destination and protected input.
/// Each validation phase checks all existing handles in one unfiltered walk.
#[derive(Debug)]
pub(crate) struct DestinationPlan<'a> {
    root: &'a Path,
    destinations: Vec<&'a Path>,
    protected: Vec<&'a Path>,
}

impl<'a> DestinationPlan<'a> {
    pub(crate) fn new(
        root: &'a Path,
        destinations: Vec<&'a Path>,
        protected: Vec<&'a Path>,
    ) -> Result<Self> {
        let plan = Self {
            root,
            destinations,
            protected,
        };
        plan.validate()?;
        Ok(plan)
    }

    fn validate(&self) -> Result<()> {
        if self.destinations.is_empty() {
            return Ok(());
        }
        let protected = self
            .protected
            .iter()
            .map(|path| {
                let handle = match Handle::from_path(path) {
                    Ok(handle) => Some(handle),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(error.into()),
                };
                Ok((location(path)?, handle))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut locations = BTreeSet::new();
        let mut destinations = Destinations::default();
        for &path in &self.destinations {
            if typescript(path) {
                bail!("report output cannot overwrite a TypeScript source destination");
            }
            let regular = |metadata: &std::fs::Metadata| -> Result<()> {
                // Never follow output symlinks, including dangling ones.
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    bail!("report output must be a regular file, not a symbolic link or directory");
                }
                Ok(())
            };
            let existing = match path.symlink_metadata() {
                Ok(metadata) => {
                    regular(&metadata)?;
                    let handle = Handle::from_path(path)?;
                    // Identify the destination by an `lstat` taken while its handle
                    // is open, so the recorded inode is never older than the handle.
                    let metadata = path.symlink_metadata()?;
                    regular(&metadata)?;
                    Some((handle, metadata))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            let location = location(path)?;
            let handle = existing.as_ref().map(|(handle, _)| handle);
            if !locations.insert(location.clone())
                || handle.is_some_and(|handle| destinations.handles.contains(handle))
            {
                bail!("baseline and report destinations must differ");
            }
            if protected.iter().any(|(input_path, input_handle)| {
                *input_path == location
                    || handle
                        .zip(input_handle.as_ref())
                        .is_some_and(|(a, b)| a == b)
            }) {
                bail!("report output cannot overwrite coverage input, config, or baseline");
            }
            if let Some((handle, metadata)) = existing {
                destinations.push(handle, &metadata);
            }
        }
        if destinations.handles.is_empty() {
            return Ok(());
        }
        // Selection and ignore rules must not hide unselected source aliases.
        // Directory symlinks are not followed, matching source-selection policy.
        let mut builder = ignore::WalkBuilder::new(self.root);
        builder
            .hidden(false)
            .ignore(false)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .follow_links(false);
        for entry in builder.build() {
            let entry = entry.context("cannot check report output against project sources")?;
            if typescript(entry.path()) && destinations.aliased_by(entry.path())? {
                bail!("report output cannot overwrite a source file");
            }
        }
        Ok(())
    }

    pub(crate) fn publish_with(
        &self,
        count: usize,
        mut render: impl FnMut(usize, &mut dyn Write) -> Result<()>,
    ) -> Result<()> {
        ensure!(
            count == self.destinations.len(),
            "missing rendered destination"
        );
        let mut staged = Vec::new();
        for (index, &path) in self.destinations.iter().enumerate() {
            let mut stage = || -> Result<_> {
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
                {
                    let mut writer =
                        std::io::BufWriter::with_capacity(1 << 20, temporary.as_file_mut());
                    render(index, &mut writer)?;
                    writer.flush()?;
                }
                temporary.as_file().sync_all()?;
                Ok(temporary)
            };
            staged
                .push(stage().with_context(|| format!("cannot write report {}", path.display()))?);
        }
        for (temporary, &path) in staged.into_iter().zip(&self.destinations) {
            // Refresh every input/destination handle after staging and immediately
            // before each rename. Individual replacements are atomic; the group is not.
            self.validate()?;
            temporary
                .persist(path)
                .with_context(|| format!("cannot write report {}", path.display()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "output/tests.rs"]
mod tests;
