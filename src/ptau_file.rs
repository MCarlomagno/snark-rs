use std::collections::HashMap;
use std::io::SeekFrom;

use crate::file::{BinFile, Section};
use crate::curves::Curve;
use anyhow::{Result, anyhow};
use r1cs::num::BigUint;
use tokio::io::AsyncSeekExt;

pub struct PTauFile {
    bin_file: BinFile,
}

impl PTauFile {

    pub fn from(bin_file: BinFile) -> Self {
        Self { bin_file }
    }

    pub async fn read_header(
        &mut self,
        sections: &HashMap<u32, Vec<Section>>,
    ) -> Result<(Curve, u32, u32)> {
        let section = sections
            .get(&1)
            .and_then(|v| v.first())
            .ok_or_else(|| anyhow!("{}: File has no header section (1)", "ptau"))?;
    
        if sections[&1].len() > 1 {
            return Err(anyhow!("ptau: File has more than one header section"));
        }
    
        self.bin_file.file.seek(SeekFrom::Start(section.offset)).await?;
        self.bin_file.pos = section.offset;
        let n8 = self.bin_file.read_u32().await?;
        let buff = self.bin_file.read_bytes(n8 as usize).await?;
        let q_biguint = BigUint::from_bytes_le(&buff);
        let curve = Curve::from_q(&q_biguint).unwrap();
    
        if (curve.f1.n64 * 8) != n8.try_into().unwrap() {
            return Err(anyhow!(
                "Invalid size: expected {} bytes, got {}",
                curve.f1.n64 * 8,
                n8
            ));
        }
    
        let power = self.bin_file.read_u32().await?;
        let ceremony_power = self.bin_file.read_u32().await?;
    
        let read_bytes = self.bin_file.pos - section.offset;
        if read_bytes != section.size {
            return Err(anyhow!(
                "Invalid PTau header size: read {}, expected {}",
                read_bytes,
                section.size
            ));
        }
    
        Ok((curve, power, ceremony_power))
    }
}
