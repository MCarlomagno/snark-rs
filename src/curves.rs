use anyhow::{Result, bail};
use r1cs::num::BigUint;
use r1cs::{Bls12_381, Bn128};

#[derive(Debug)]
pub struct Field {
    pub n64: usize, // Number of 64-bit words
}

#[derive(Debug)]
pub enum R1csField {
    Bn128(Bn128),
    Bls12_381(Bls12_381),
}

impl Clone for R1csField {
    fn clone(&self) -> Self {
        match self {
            R1csField::Bn128(_) => R1csField::Bn128(Bn128 {}),
            R1csField::Bls12_381(_) => R1csField::Bls12_381(Bls12_381 {}),
        }
    }
}

#[derive(Debug)]
pub struct Curve {
    pub f1: Field,
    pub q: BigUint,
    pub r: BigUint,
    pub n8q: usize, // bytes for q field (Fq, G1/G2 coords)
    pub n8r: usize, // bytes for r field (Fr, scalar field)
    pub fr: R1csField,
}

/// Given a field modulus `q`, returns curve-specific metadata such as the number
/// of 64-bit words needed to represent elements in the base field (F1).
///
/// This function currently supports BN128 and BLS12-381. It uses the modulus to
/// identify which curve is being referenced. If the modulus doesn't match a known
/// curve, it returns an error.
pub fn get_curve_from_q(q: &BigUint) -> Result<Curve> {
    let bn_128_q: BigUint = BigUint::parse_bytes(
        b"21888242871839275222246405745257275088696311157297823662689037894645226208583",
        10,
    )
    .unwrap();

    let bls12_381_q: BigUint = BigUint::parse_bytes(b"1a0111ea397fe69a4b1ba7b6434bacd764774b84f38512bf6730d2a0f6b0f6241eabfffeb153ffffb9feffffffffaaab", 16).unwrap();
    
    let bn_128_r: BigUint = BigUint::parse_bytes(b"21888242871839275222246405745257275088548364400416034343698204186575808495617", 10).unwrap();

    let bls12_381_r = BigUint::parse_bytes(
        b"73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001",
        16,
    ).unwrap();
    
    if q == &bn_128_q {
        let test = Bn128 {};
        Ok(Curve {
            f1: Field { n64: 4 }, // 256 bits / 64
            q: bn_128_q,
            r: bn_128_r,
            n8q: 32,
            n8r: 32,
            fr: R1csField::Bn128(Bn128 {}),
        })
    } else if q == &bls12_381_q {
        Ok(Curve {
            f1: Field { n64: 6 }, // 381 bits -> 6 * 64-bit words (384 bits)
            q: bls12_381_q,
            r: bls12_381_r,
            n8q: 48,
            n8r: 32,
            fr: R1csField::Bls12_381(Bls12_381 {}),
        })  
    } else {
        bail!("Curve not supported: {}", q);
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_get_curve_from_q_bn128() {
        let q = BigUint::parse_bytes(
            b"21888242871839275222246405745257275088696311157297823662689037894645226208583",
            10,
        )
        .unwrap();
        let curve = get_curve_from_q(&q).unwrap();
        assert_eq!(curve.f1.n64, 4);
    }
    #[test]
    fn test_get_curve_from_q_bls12381() {
        let q = BigUint::parse_bytes(b"1a0111ea397fe69a4b1ba7b6434bacd764774b84f38512bf6730d2a0f6b0f6241eabfffeb153ffffb9feffffffffaaab", 16).unwrap();
        let curve = get_curve_from_q(&q).unwrap();
        assert_eq!(curve.f1.n64, 6);
    }
    #[test]
    fn test_get_curve_from_q_not_supported() {
        let q = BigUint::parse_bytes(b"1234567890123456789012345678901234567890", 16).unwrap();
        let curve = get_curve_from_q(&q);
        assert!(curve.is_err());
    }
}
