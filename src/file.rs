use anyhow::{Result, anyhow};
use num_bigint::BigUint;
use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::Path;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::curves::{Curve, get_curve_from_q};

#[derive(Debug, Clone)]
pub struct Section {
    pub offset: u64,
    pub size: u64,
}

pub struct BinFile {
    pub file: File,
    pub pos: u64,
}

impl BinFile {
    pub async fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path).await?;
        Ok(Self { file, pos: 0 })
    }

    pub async fn read_bytes(&mut self, len: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        self.file.read_exact(&mut buf).await?;
        self.pos += len as u64;
        Ok(buf)
    }

    pub async fn read_u32(&mut self) -> Result<u32> {
        let mut buf = [0u8; 4];
        self.file.read_exact(&mut buf).await?;
        self.pos += 4;
        Ok(u32::from_le_bytes(buf))
    }

    pub async fn read_u64(&mut self) -> Result<u64> {
        let mut buf = [0u8; 8];
        self.file.read_exact(&mut buf).await?;
        self.pos += 8;
        Ok(u64::from_le_bytes(buf))
    }

    pub async fn skip(&mut self, n: u64) -> Result<()> {
        self.pos += n;
        self.file.seek(SeekFrom::Start(self.pos)).await?;
        Ok(())
    }
}

pub async fn read_bin_file(
    file_name: &str,
    expected_type: &str,
    max_version: u32,
) -> Result<(BinFile, HashMap<u32, Vec<Section>>)> {
    let mut bin_file = BinFile::open(file_name).await?;

    let file_type_bytes = bin_file.read_bytes(4).await?;
    let read_type = String::from_utf8(file_type_bytes.clone())
        .map_err(|_| anyhow!("Invalid UTF-8 in file type"))?;

    if read_type != expected_type {
        return Err(anyhow!(
            "{}: Invalid file format (expected {}, got {})",
            file_name,
            expected_type,
            read_type
        ));
    }

    let version = bin_file.read_u32().await?;
    if version > max_version {
        return Err(anyhow!(
            "Version {} not supported (max {})",
            version,
            max_version
        ));
    }

    let n_sections = bin_file.read_u32().await?;

    let mut sections: HashMap<u32, Vec<Section>> = HashMap::new();

    for _ in 0..n_sections {
        let ht = bin_file.read_u32().await?;
        let hl = bin_file.read_u64().await?;
        let offset = bin_file.pos;

        sections
            .entry(ht)
            .or_default()
            .push(Section { offset, size: hl });

        bin_file.skip(hl).await?;
    }

    Ok((bin_file, sections))
}

pub async fn read_ptau_header(
    fd: &mut BinFile,
    sections: &HashMap<u32, Vec<Section>>,
) -> Result<(Curve, u32, u32)> {
    let section = sections
        .get(&1)
        .and_then(|v| v.first())
        .ok_or_else(|| anyhow!("{}: File has no header section (1)", "ptau"))?;

    if sections[&1].len() > 1 {
        return Err(anyhow!("ptau: File has more than one header section"));
    }

    fd.file.seek(SeekFrom::Start(section.offset)).await?;
    fd.pos = section.offset;
    let n8 = fd.read_u32().await?;
    let buff = fd.read_bytes(n8 as usize).await?;
    let q_biguint = BigUint::from_bytes_le(&buff);
    let curve = get_curve_from_q(&q_biguint).unwrap();

    if (curve.f1.n64 * 8) != n8.try_into().unwrap() {
        return Err(anyhow!(
            "Invalid size: expected {} bytes, got {}",
            curve.f1.n64 * 8,
            n8
        ));
    }

    let power = fd.read_u32().await?;
    let ceremony_power = fd.read_u32().await?;

    let read_bytes = fd.pos - section.offset;
    if read_bytes != section.size {
        return Err(anyhow!(
            "Invalid PTau header size: read {}, expected {}",
            read_bytes,
            section.size
        ));
    }

    Ok((curve, power, ceremony_power))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Seek;
    use std::io::SeekFrom;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_read_bin_file_basic() -> Result<()> {
        // Create temporary file
        let tmp = NamedTempFile::new()?;
        let mut file = OpenOptions::new().write(true).read(true).open(tmp.path())?;

        // Build binary structure
        let mut contents = vec![];

        // Header: "ptau"
        contents.extend(b"ptau");

        // Version: 1 (ULE32)
        contents.extend(&1u32.to_le_bytes());

        // Section count: 2 (ULE32)
        contents.extend(&2u32.to_le_bytes());

        // Section 1: ht = 3, hl = 8
        contents.extend(&3u32.to_le_bytes());
        contents.extend(&8u64.to_le_bytes());
        contents.extend(&[0xaa; 8]); // Dummy section data

        // Section 2: ht = 12, hl = 4
        contents.extend(&12u32.to_le_bytes());
        contents.extend(&4u64.to_le_bytes());
        contents.extend(&[0xbb; 4]); // Dummy section data

        // Write to file
        file.write_all(&contents)?;
        file.sync_all()?;
        file.seek(SeekFrom::Start(0))?;

        // Now test read_bin_file
        let (_bin_file, sections) = read_bin_file(tmp.path().to_str().unwrap(), "ptau", 2).await?;

        assert_eq!(sections.len(), 2);
        assert!(sections.contains_key(&3));
        assert!(sections.contains_key(&12));

        let sec3 = &sections[&3][0];
        assert_eq!(sec3.size, 8);

        let sec12 = &sections[&12][0];
        assert_eq!(sec12.size, 4);

        Ok(())
    }

    #[tokio::test]
    async fn test_invalid_magic_type() {
        let tmp = NamedTempFile::new().unwrap();
        let mut file = OpenOptions::new()
            .write(true)
            .read(true)
            .open(tmp.path())
            .unwrap();

        file.write_all(b"junk").unwrap(); // Invalid header
        file.write_all(&1u32.to_le_bytes()).unwrap(); // version
        file.write_all(&0u32.to_le_bytes()).unwrap(); // section count
        file.sync_all().unwrap();

        let result = read_bin_file(tmp.path().to_str().unwrap(), "ptau", 2).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unsupported_version() {
        let tmp = NamedTempFile::new().unwrap();
        let mut file = OpenOptions::new()
            .write(true)
            .read(true)
            .open(tmp.path())
            .unwrap();

        file.write_all(b"ptau").unwrap(); // valid header
        file.write_all(&999u32.to_le_bytes()).unwrap(); // version too high
        file.write_all(&0u32.to_le_bytes()).unwrap(); // section count
        file.sync_all().unwrap();

        let result = read_bin_file(tmp.path().to_str().unwrap(), "ptau", 1).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_real_ptau_file() -> Result<()> {
        let path = "src/fixtures/pot24.ptau";
        let (_bin_file, sections) = read_bin_file(path, "ptau", 1).await?;

        // Check basic expectations on real file
        assert!(!sections.is_empty(), "ptau file should have sections");

        // These section IDs are typical for ptau files, e.g., 1 = header, 12 = G1 powers
        let expected_ids = [1, 12, 13, 14, 15];
        let _present_ids: Vec<u32> = sections.keys().copied().collect();

        for id in &expected_ids {
            assert!(
                sections.contains_key(id),
                "Missing expected section id: {}",
                id
            );
        }

        // Optional: print some info to debug
        for (id, sects) in &sections {
            for s in sects {
                println!("Section {} -> offset: {}, size: {}", id, s.offset, s.size);
            }
        }

        Ok(())
    }
}
