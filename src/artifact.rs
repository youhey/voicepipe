use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactMetadata {
    pub sha256: String,
    pub size_bytes: u64,
}

pub fn calculate_metadata(path: &Path) -> Result<ArtifactMetadata> {
    let file =
        File::open(path).with_context(|| format!("artifact を開けません: {}", path.display()))?;
    let size_bytes = file
        .metadata()
        .with_context(|| format!("artifact metadata を取得できません: {}", path.display()))?
        .len();
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];

    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .with_context(|| format!("artifact を読み込めません: {}", path.display()))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(ArtifactMetadata {
        sha256: format!("{:x}", hasher.finalize()),
        size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn calculates_sha256_and_size_from_file_bytes() {
        let temp_dir =
            std::env::temp_dir().join(format!("voicepipe-artifact-test-{}", std::process::id()));
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let path = temp_dir.join("artifact.bin");
        fs::write(&path, b"abc").expect("artifact should be written");

        let metadata = calculate_metadata(&path).expect("metadata should be calculated");

        assert_eq!(
            metadata.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(metadata.size_bytes, 3);

        fs::remove_dir_all(temp_dir).expect("temp dir should be removed");
    }
}
