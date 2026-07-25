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
    // The same canonical serializer is used for both the owner-waived
    // listening-test pack and the approved library. Keep this utility usable
    // for the release pack as well; validation still gates the input before
    // any bytes are rewritten.
    if manifest.validate_bundled_owner_waived().is_err() {
        manifest.validate_published()?;
    }
    let canonical = canonical_manifest_bytes(&manifest)?;

    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(&canonical)?;
    staged.as_file().sync_all()?;
    staged.persist(&path)?;
    Ok(())
}
