use anyhow::{anyhow, bail, Result};
use r1cs::num::BigUint;
use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::Path;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::curves::{Curve, get_curve_from_q};

const R1CS_FILE_HEADER_SECTION: u32 = 1;
const R1CS_FILE_CUSTOM_GATES_LIST_SECTION: u32 = 4;
const R1CS_FILE_CUSTOM_GATES_USES_SECTION: u32 = 5;

pub struct R1cs {
    pub header: R1csHeader,
    pub constraints: Vec<[HashMap<u32, BigUint>; 3]>,
}

pub type LinearCombination = HashMap<u32, BigUint>; // idx → coefficient
pub type Constraint = [LinearCombination; 3]; // A, B, C

#[derive(Debug)]
pub struct R1csHeader {
    pub n8: u32,
    pub prime: BigUint,
    pub n_vars: u32,
    pub n_outputs: u32,
    pub n_pub_inputs: u32,
    pub n_prv_inputs: u32,
    pub n_labels: u64,
    pub n_constraints: u32,
    pub use_custom_gates: bool,
}

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

    pub async fn create<P: AsRef<Path>>(
        path: P,
        magic_type: &str,
        version: u32,
        n_sections: u32,
    ) -> Result<Self> {
        if magic_type.len() != 4 {
            bail!("Magic type must be exactly 4 characters");
        }

        let mut file = File::create(path).await?;
        let mut pos = 0;

        // Write magic type
        file.write_all(magic_type.as_bytes()).await?;
        pos += 4;

        // Write version
        file.write_all(&version.to_le_bytes()).await?;
        pos += 4;

        // Write number of sections
        file.write_all(&n_sections.to_le_bytes()).await?;
        pos += 4;

        Ok(Self { file, pos })
    }

    pub async fn write_bytes(&mut self, data: &[u8]) -> Result<()> {
        self.file.write_all(data).await?;
        self.pos += data.len() as u64;
        Ok(())
    }

    pub async fn write_u32(&mut self, val: u32) -> Result<()> {
        self.file.write_all(&val.to_le_bytes()).await?;
        self.pos += 4;
        Ok(())
    }

    pub async fn write_u64(&mut self, val: u64) -> Result<()> {
        self.file.write_all(&val.to_le_bytes()).await?;
        self.pos += 8;
        Ok(())
    }

    pub async fn flush(&mut self) -> Result<()> {
        self.file.flush().await?;
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

pub async fn read_r1cs_header(
    fd: &mut BinFile,
    sections: &HashMap<u32, Vec<Section>>,
) -> Result<R1csHeader> {
    // Locate header section
    let section = sections
        .get(&R1CS_FILE_HEADER_SECTION)
        .and_then(|v| v.first())
        .ok_or_else(|| anyhow!("R1CS header section missing"))?;

    if sections[&R1CS_FILE_HEADER_SECTION].len() > 1 {
        bail!("R1CS header section duplicated");
    }

    // Seek to header section start
    fd.file.seek(SeekFrom::Start(section.offset)).await?;
    fd.pos = section.offset;

    // Read header values
    let n8 = fd.read_u32().await?;
    let prime_bytes = fd.read_bytes(n8 as usize).await?;
    let prime = BigUint::from_bytes_le(&prime_bytes);

    let n_vars = fd.read_u32().await?;
    let n_outputs = fd.read_u32().await?;
    let n_pub_inputs = fd.read_u32().await?;
    let n_prv_inputs = fd.read_u32().await?;
    let n_labels = fd.read_u64().await?;
    let n_constraints = fd.read_u32().await?;

    // Check for custom gates sections
    let use_custom_gates = sections.contains_key(&R1CS_FILE_CUSTOM_GATES_LIST_SECTION)
        && sections.contains_key(&R1CS_FILE_CUSTOM_GATES_USES_SECTION);

    // Validate we consumed the section fully
    let read_len = fd.pos - section.offset;
    if read_len != section.size {
        bail!(
            "Invalid R1CS header section size: read {}, expected {}",
            read_len,
            section.size
        );
    }

    Ok(R1csHeader {
        n8,
        prime,
        n_vars,
        n_outputs,
        n_pub_inputs,
        n_prv_inputs,
        n_labels,
        n_constraints,
        use_custom_gates,
    })
}

pub async fn read_section(
    fd: &mut BinFile,
    sections: &HashMap<u32, Vec<Section>>,
    section_id: u32,
    offset: Option<u64>,
    length: Option<u64>,
) -> Result<Vec<u8>> {
    let section = sections
        .get(&section_id)
        .and_then(|v| v.first())
        .ok_or_else(|| anyhow!("Section {} not found", section_id))?;

    let off = offset.unwrap_or(0);
    let len = length.unwrap_or(section.size - off);

    if off + len > section.size {
        return Err(anyhow!(
            "Out-of-bounds section read: offset {} + length {} > size {}",
            off,
            len,
            section.size
        ));
    }

    fd.file.seek(SeekFrom::Start(section.offset + off)).await?;
    fd.pos = section.offset + off;

    let mut buf = vec![0u8; len as usize];
    fd.file.read_exact(&mut buf).await?;
    fd.pos += len;

    Ok(buf)
}

pub async fn read_constraints(
    fd: &mut BinFile,
    sections: &HashMap<u32, Vec<Section>>,
    r1cs: &R1csHeader,
) -> Result<Vec<[HashMap<u32, BigUint>; 3]>> {
    const CONSTRAINTS_SECTION: u32 = 2;

    let section = sections
        .get(&CONSTRAINTS_SECTION)
        .and_then(|v| v.first())
        .ok_or_else(|| anyhow::anyhow!("Missing constraints section"))?;

    fd.file.seek(SeekFrom::Start(section.offset)).await?;
    fd.pos = section.offset;

    let mut buf = vec![0u8; section.size as usize];
    fd.file.read_exact(&mut buf).await?;
    fd.pos += section.size;

    let mut constraints: Vec<[HashMap<u32, BigUint>; 3]> = Vec::with_capacity(r1cs.n_constraints as usize);
    let mut cursor = 0;

    for _ in 0..r1cs.n_constraints {
        let mut triple: [HashMap<u32, BigUint>; 3] = Default::default();
        for lc in &mut triple {
            let n_idx = u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;

            for _ in 0..n_idx {
                let idx = u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap());
                cursor += 4;

                let coeff_bytes = &buf[cursor..cursor + r1cs.n8 as usize];
                cursor += r1cs.n8 as usize;

                let coeff = BigUint::from_bytes_le(coeff_bytes);
                lc.insert(idx, coeff);
            }
        }
        constraints.push(triple);
    }

    // Optional: sanity check we consumed entire section
    if (cursor as u64) != section.size {
        bail!("Unexpected constraint section size: read {}, expected {}", cursor, section.size);
    }

    Ok(constraints)
} 

pub async fn read_r1cs_fd(fd: &mut BinFile, sections: &HashMap<u32, Vec<Section>>) -> Result<R1cs> {
    let header = read_r1cs_header(fd, sections).await?;
    let constraints = read_constraints(fd, sections, &header).await?;
    Ok(R1cs { header, constraints })
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
        let path = "src/artifacts/pot24.ptau";
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

    #[tokio::test]
    #[ignore] // Heavy test, run only on demand.
    async fn test_read_constraints_basic() -> Result<()> {
        let path = "src/artifacts/email_auth.r1cs";

        let (mut fd, sections) = read_bin_file(path, "r1cs", 1).await?;
        let header = read_r1cs_header(&mut fd, &sections).await?;
        let constraints = read_constraints(&mut fd, &sections, &header).await?;

        assert_eq!(constraints.len(), header.n_constraints as usize);

        // Optional: inspect first constraint
        let c0 = &constraints[0];
        for (i, lc) in c0.iter().enumerate() {
            println!("Constraint part {}: {} terms", i, lc.len());
            for (idx, val) in lc {
                println!("  [{}] = {}", idx, val.to_str_radix(10));
            }
        }

        Ok(())
    }
}
