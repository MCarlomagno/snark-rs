use r1cs::{Bn128, Element, Field};
use num_bigint::BigUint;
use std::ops::{Add, Sub, Mul};

pub struct FftEngine {
    pub w: Vec<Element<Bn128>>,      // roots of unity
    pub wi: Vec<Element<Bn128>>,     // inverse roots
    pub one: Element<Bn128>,
    pub twoinv: Element<Bn128>,
}

impl FftEngine {
    pub fn new(max_bits: usize) -> Self {
        let mut nqr = Element::<Bn128>::one();
        let half = (Bn128::order() - 1u32) >> 1;
        let half = Element::<Bn128>::from(half);

        while nqr.clone().exponentiation(&half) == Element::<Bn128>::one() {
            nqr = &nqr + &Element::<Bn128>::one();
        }

        let mut w = vec![Element::<Bn128>::zero(); max_bits + 1];
        let mut wi = vec![Element::<Bn128>::zero(); max_bits + 1];

        let pow = Element::<Bn128>::from(Bn128::order() - 1u32 >> max_bits);
        w[max_bits] = nqr.clone().exponentiation(&pow);
        wi[max_bits] = w[max_bits].multiplicative_inverse_or_zero();

        for i in (0..max_bits).rev() {
            w[i] = w[i + 1].clone() * &w[i + 1];
            wi[i] = wi[i + 1].clone() * &wi[i + 1];
        }

        let one = Element::<Bn128>::one();
        let twoinv = (&one + &one).multiplicative_inverse_or_zero();

        Self { w, wi, one, twoinv }
    }

    pub fn fft(&self, input: &[Element<Bn128>]) -> Vec<Element<Bn128>> {
        self.fft_internal(input, false)
    }

    pub fn ifft(&self, input: &[Element<Bn128>]) -> Vec<Element<Bn128>> {
        let mut out = self.fft_internal(input, true);
        let inv_n = Element::<Bn128>::from(input.len() as u64).multiplicative_inverse_or_zero();
        out.iter_mut().for_each(|x| *x = x.clone() * &inv_n);
        out
    }

    fn fft_internal(&self, input: &[Element<Bn128>], inverse: bool) -> Vec<Element<Bn128>> {
        let n = input.len();
        let bits = (n as f64).log2() as usize;
        assert_eq!(n, 1 << bits, "Input length must be power of 2");

        let mut output = vec![Element::<Bn128>::zero(); n];
        for i in 0..n {
            let rev = bit_reverse(i, bits);
            output[rev] = input[i].clone();
        }

        for s in 1..=bits {
            let m = 1 << s;
            let m_half = m / 2;
            let root = if inverse { &self.wi[s] } else { &self.w[s] };
            for k in (0..n).step_by(m) {
                let mut w = Element::<Bn128>::one();
                for j in 0..m_half {
                    let t = w.clone() * &output[k + j + m_half];
                    let u = output[k + j].clone();
                    output[k + j] = &u + &t;
                    output[k + j + m_half] = &u - &t;
                    w = w * root;
                }
            }
        }

        output
    }
}

fn bit_reverse(mut x: usize, bits: usize) -> usize {
    let mut result = 0;
    for _ in 0..bits {
        result = (result << 1) | (x & 1);
        x >>= 1;
    }
    result
}
