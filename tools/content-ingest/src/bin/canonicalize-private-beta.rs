use std::fs;
use std::io::Write;
use std::path::PathBuf;

use catalogue::{canonical_manifest_bytes, ContentPackManifest};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: canonicalize-private-beta <manifest.json>")?;
    let parent = path.parent().ok_or("manifest path has no parent")?;
    let manifest: ContentPackManifest = serde_json::from_slice(&fs::read(&path)?)?;
    manifest.validate_bundled_owner_waived()?;
    let canonical = canonical_manifest_bytes(&manifest)?;

    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(&canonical)?;
    staged.as_file().sync_all()?;
    staged.persist(&path)?;
    Ok(())
}
