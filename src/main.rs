use std::cmp::max;

use ::r1cs::{num::BigUint, Bls12_381, Bn128, Element, Field};

use crate::{curves::R1csField, file::BinFile};

mod curves;
mod file;
mod utils;
mod r1cs;

#[derive(Debug)]
pub enum K1K2 {
    Bn128(Element<Bn128>, Element<Bn128>),
    Bls12_381(Element<Bls12_381>, Element<Bls12_381>),
}

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
    let constraints = r1cs::process_constraints(&curve.fr, &mut r1cs);

    let mut fd_zkey = file::BinFile::create("output.zkey", "zkey", 1, 14).await?;

    // 1. Check if R1CS curve matches ptau curve prime
    if r1cs.header.prime != curve.r {
        eprintln!("❌ R1CS curve does not match PTAU curve");
        return Ok(());
    }

    // 2. Compute circuit power
    let plonk_constraints_len = match &constraints {
        r1cs::ConstraintOutput::Bn128(c, _) => c.len(),
        r1cs::ConstraintOutput::Bls12_381(c, _) => c.len(),
    };

    let mut cir_power = ((plonk_constraints_len - 1) as f64).log2().ceil() as u32;
    cir_power = max(cir_power, 3); // t polynomial requires at least power 3

    let domain_size = 1 << cir_power;

    println!("ℹ️  Plonk constraints: {}", plonk_constraints_len);

    if cir_power > power {
        eprintln!(
            "❌ Circuit too big for this PTAU. 2**{} > 2**{} ({} constraints)",
            cir_power, power, plonk_constraints_len
        );
        return Ok(());
    }

    // 4. Check if section 12 is present
    if !sections_ptau.contains_key(&12) {
        eprintln!("❌ PTAU file is not prepared (section 12 missing)");
        return Ok(());
    }


    let k1k2 = match &curve.fr {
        R1csField::Bn128(_) => {
            let (k1, k2) = get_k1_k2::<Bn128>(&curve.r, cir_power as usize, domain_size);
            K1K2::Bn128(k1, k2)
        }
        R1csField::Bls12_381(_) => {
            let (k1, k2) = get_k1_k2::<Bls12_381>(&curve.r, cir_power as usize, domain_size);
            K1K2::Bls12_381(k1, k2)
        }
    };

    match k1k2 {
        K1K2::Bn128(k1, k2) => {
            println!("ℹ️  k1: {}, k2: {}", k1, k2);
        }
        K1K2::Bls12_381(k1, k2) => {
            println!("ℹ️  k1: {}, k2: {}", k1, k2);
        }
    }
    Ok(())
}

pub fn get_k1_k2<F: Field>(
    r: &BigUint,
    pow: usize,
    domain_size: u64,
) -> (Element<F>, Element<F>) {
    let one = Element::<F>::one();
    let two = &one + &one;

    let exp = (r - 1u32) >> pow;
    let w = two.exponentiation(&Element::<F>::from(exp));

    fn is_included<F: Field>(
        k: &Element<F>,
        existing: &[Element<F>],
        w: &Element<F>,
        domain_size: u64,
    ) -> bool {
        let mut cur = Element::<F>::one();
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
    while is_included::<F>(&k1, &[], &w, domain_size) {
        k1 = &k1 + &one;
    }

    // Step 4: find k2
    let mut k2 = &k1 + &one;
    while is_included::<F>(&k2, &[k1.clone()], &w, domain_size) {
        k2 = &k2 + &one;
    }

    (k1, k2)
}
