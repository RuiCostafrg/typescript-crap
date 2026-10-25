//! Serialized source identities share these rules; selection/glob policies do not.
pub(crate) fn normalized_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':'])
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

pub(crate) fn sha256_digest(digest: &str) -> bool {
    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests;

/// Encode actual platform separators without rewriting filename characters.
pub(crate) fn source_path(path: &std::path::Path) -> anyhow::Result<String> {
    use anyhow::{Context, ensure};
    let parts = path
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .context("source path is not UTF-8")
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let relative = parts.join("/");
    ensure!(
        normalized_relative_path(&relative),
        "source path cannot be represented in schema v1: {relative:?}"
    );
    Ok(relative)
}
