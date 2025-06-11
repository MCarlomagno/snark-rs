use std::cmp::max;

use crate::file::BinFile;
use ::r1cs::{Bn128, Element, num::BigUint};

mod curves;
mod file;
mod r1cs;
mod utils;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ptau_path = "src/artifacts/pot24.ptau";
    let r1cs_path = "src/artifacts/email_auth.r1cs";

    println!("Processing PTAU..");
    let (mut fd_ptau, sections_ptau) = file::read_bin_file(ptau_path, "ptau", 1).await?;

    let (curve, power, ceremony_power) =
        file::read_ptau_header(&mut fd_ptau, &sections_ptau).await?;
    println!(
        "Curve: {}, Power: {}, Ceremony Power: {}",
        curve.f1.n64, power, ceremony_power
    );

    let (mut fd_r1cs, sections_r1cs) = file::read_bin_file(r1cs_path, "r1cs", 1).await?;

    println!("Processing R1CS...");
    let mut r1cs = file::read_r1cs_fd(&mut fd_r1cs, &sections_r1cs).await?;
    println!("R1CS constraints: {}", r1cs.header.n_constraints);

    let s_g1 = curve.n8q * 2;
    let s_g2 = curve.n8q * 4;
    let n8r = curve.n8r;

    let s_r1cs = file::read_section(&mut fd_r1cs, &sections_r1cs, 2, None, None).await?;

    let plonk_n_vars = r1cs.header.n_vars;
    let n_public = r1cs.header.n_outputs + r1cs.header.n_pub_inputs;

    println!("Plonk n_vars: {}, n_public: {}", plonk_n_vars, n_public);
    println!("Processing constraints...");
    let (plonk_constraints, plonk_additions) = r1cs::process_constraints(&mut r1cs);

    // 1. Check if R1CS curve matches ptau curve prime
    if r1cs.header.prime != curve.r {
        eprintln!("❌ R1CS curve does not match PTAU curve");
        return Ok(());
    }

    let mut cir_power = ((plonk_constraints.len() - 1) as f64).log2().ceil() as u32;
    cir_power = max(cir_power, 3); // t polynomial requires at least power 3

    let domain_size = 1 << cir_power;

    println!("ℹ️  Plonk constraints: {}", plonk_constraints.len());

    if cir_power > power {
        eprintln!(
            "❌ Circuit too big for this PTAU. 2**{} > 2**{} ({} constraints)",
            cir_power,
            power,
            plonk_constraints.len()
        );
        return Ok(());
    }

    // 4. Check if section 12 is present
    if !sections_ptau.contains_key(&12) {
        eprintln!("❌ PTAU file is not prepared (section 12 missing)");
        return Ok(());
    }

    let (k1, k2) = get_k1_k2(&curve.r, cir_power as usize, domain_size);
    println!("ℹ️  k1: {}, k2: {}", k1, k2);

    let mut fd_zkey = file::BinFile::create("output.zkey", "zkey", 1, 14).await?;
    write_additions(&mut fd_zkey, 3, "Additions", n8r, &plonk_additions).await?;

    Ok(())
}

pub fn get_k1_k2(r: &BigUint, pow: usize, domain_size: u64) -> (Element<Bn128>, Element<Bn128>) {
    let one = Element::<Bn128>::one();
    let two = &one + &one;

    let exp = (r - 1u32) >> pow;
    let w = two.exponentiation(&Element::<Bn128>::from(exp));

    fn is_included(
        k: &Element<Bn128>,
        existing: &[Element<Bn128>],
        w: &Element<Bn128>,
        domain_size: u64,
    ) -> bool {
        let mut cur = Element::<Bn128>::one();
        for _ in 0..domain_size {
            if k == &cur {
                return true;
            }
            for e in existing {
                if k == &(e.clone() * &cur) {
                    return true;
                }
            }
            cur = &cur * w;
        }
        false
    }

    // Step 3: find k1
    let mut k1 = two.clone();
    while is_included(&k1, &[], &w, domain_size) {
        k1 = &k1 + &one;
    }

    // Step 4: find k2
    let mut k2 = &k1 + &one;
    while is_included(&k2, &[k1.clone()], &w, domain_size) {
        k2 = &k2 + &one;
    }

    (k1, k2)
}

pub trait ToMontgomeryBytes {
    fn as_montgomery_bytes(&self) -> Vec<u8>;
}

impl ToMontgomeryBytes for Element<Bn128> {
    fn as_montgomery_bytes(&self) -> Vec<u8> {
        self.to_biguint().to_bytes_le()
    }
}

pub async fn write_additions(
    fd: &mut BinFile,
    section_num: u32,
    name: &str,
    n8r: usize,
    plonk_additions: &[(u32, u32, Element<Bn128>, Element<Bn128>)],
) -> Result<(), anyhow::Error> {
    fd.start_write_section(section_num).await?;

    let mut buffer = vec![0u8; 2 * 4 + 2 * n8r];

    for (i, (a, b, v1, v2)) in plonk_additions.iter().enumerate() {
        let mut offset = 0;

        buffer[offset..offset + 4].copy_from_slice(&a.to_le_bytes());
        offset += 4;

        buffer[offset..offset + 4].copy_from_slice(&b.to_le_bytes());
        offset += 4;

        let v1_bytes = v1.as_montgomery_bytes();
        let v2_bytes = v2.as_montgomery_bytes();

        buffer[offset..offset + n8r].copy_from_slice(&v1_bytes[..n8r]);
        offset += n8r;

        buffer[offset..offset + n8r].copy_from_slice(&v2_bytes[..n8r]);

        fd.write_bytes(&buffer).await?;

        if i % 1_000 == 0 {
            println!("🔧 Writing {name}: {}/{}", i, plonk_additions.len());
        }
    }

    fd.end_write_section().await?;

    Ok(())
}
