use num_bigint::BigUint;
use anyhow::{Result, bail};

#[derive(Debug)]
pub struct Field {
    pub n64: usize, // Number of 64-bit words
}

#[derive(Debug)]
pub struct Curve {
    pub f1: Field,
}

/// Given a field modulus `q`, returns curve-specific metadata such as the number
/// of 64-bit words needed to represent elements in the base field (F1).
///
/// This function currently supports BN128 and BLS12-381. It uses the modulus to
/// identify which curve is being referenced. If the modulus doesn't match a known
/// curve, it returns an error.
pub fn get_curve_from_q(q: &BigUint) -> Result<Curve> {
    let bn128q: BigUint = BigUint::parse_bytes(b"21888242871839275222246405745257275088696311157297823662689037894645226208583", 10).unwrap();
    let bls12381q: BigUint = BigUint::parse_bytes(b"1a0111ea397fe69a4b1ba7b6434bacd764774b84f38512bf6730d2a0f6b0f6241eabfffeb153ffffb9feffffffffaaab", 16).unwrap();
    if q == &bn128q {
        Ok(Curve {
            f1: Field { n64: 4 }, // 256 bits / 64
        })
    } else if q == &bls12381q {
        Ok(Curve {
            f1: Field { n64: 6 }, // 381 bits -> 6 * 64-bit words (384 bits)
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
        let q = BigUint::parse_bytes(b"21888242871839275222246405745257275088696311157297823662689037894645226208583", 10).unwrap();
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
