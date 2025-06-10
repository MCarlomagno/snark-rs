use std::collections::HashMap;
use r1cs::{Bls12_381, Bn128, Element, Field};
use crate::curves::R1csField;
use crate::file::R1cs;

pub enum ConstraintOutput {
    Bn128(
        Vec<(u32, u32, u32, Element<Bn128>, Element<Bn128>, Element<Bn128>, Element<Bn128>, Element<Bn128>)>,
        Vec<(u32, u32, Element<Bn128>, Element<Bn128>)>,
    ),
    Bls12_381(
        Vec<(u32, u32, u32, Element<Bls12_381>, Element<Bls12_381>, Element<Bls12_381>, Element<Bls12_381>, Element<Bls12_381>)>,
        Vec<(u32, u32, Element<Bls12_381>, Element<Bls12_381>)>,
    ),
}

pub fn process_constraints(fr: &R1csField, r1cs: &mut R1cs) -> ConstraintOutput {
    match fr {
        R1csField::Bn128(_) => {
            let (constraints, additions) = process_constraints_impl::<Bn128>(r1cs);
            ConstraintOutput::Bn128(constraints, additions)
        }
        R1csField::Bls12_381(_) => {
            let (constraints, additions) = process_constraints_impl::<Bls12_381>(r1cs);
            ConstraintOutput::Bls12_381(constraints, additions)
        }
    }
}

fn process_constraints_impl<F: Field>(r1cs: &mut R1cs) -> (Vec<(u32, u32, u32, Element<F>, Element<F>, Element<F>, Element<F>, Element<F>)>, Vec<(u32, u32, Element<F>, Element<F>)>) {
    type Coeff<F> = Element<F>;
    type LinearCombination<F> = HashMap<u32, Coeff<F>>;

    let mut plonk_n_vars = r1cs.header.n_vars;
    let n_public = r1cs.header.n_outputs + r1cs.header.n_pub_inputs;

    let mut plonk_constraints: Vec<(u32, u32, u32, Coeff<F>, Coeff<F>, Coeff<F>, Coeff<F>, Coeff<F>)> = vec![];
    let mut plonk_additions: Vec<(u32, u32, Coeff<F>, Coeff<F>)> = vec![];

    fn normalize<F: Field>(lc: &mut LinearCombination<F>) {
        lc.retain(|_, v| !v.is_zero());
    }

    fn join<F: Field>(lc1: &LinearCombination<F>, k: &Coeff<F>, lc2: &LinearCombination<F>) -> LinearCombination<F> {
        let mut res = HashMap::new();
        for (s, v) in lc1 {
            let val = k.clone() * v.clone();
            res.entry(*s).and_modify(|e: &mut Coeff<F>| *e = e.clone() + val.clone()).or_insert(val);
        }
        for (s, v) in lc2 {
            res.entry(*s).and_modify(|e: &mut Coeff<F>| *e = e.clone() + v.clone()).or_insert(v.clone());
        }
        normalize(&mut res);
        res
    }

    fn reduce_coefs<F: Field>(
        lc: &LinearCombination<F>,
        max_c: usize,
        plonk_n_vars: &mut u32,
        plonk_constraints: &mut Vec<(u32, u32, u32, Coeff<F>, Coeff<F>, Coeff<F>, Coeff<F>, Coeff<F>)>,
        plonk_additions: &mut Vec<(u32, u32, Coeff<F>, Coeff<F>)>,
    ) -> (Coeff<F>, Vec<u32>, Vec<Coeff<F>>) {
        let mut k = Coeff::<F>::zero();
        let mut cs = vec![];

        for (&s, v) in lc {
            if s == 0 {
                k = k + v.clone();
            } else {
                cs.push((s, v.clone()));
            }
        }

        while cs.len() > max_c {
            let c1 = cs.remove(0);
            let c2 = cs.remove(0);

            let sl = c1.0;
            let sr = c2.0;
            let so = *plonk_n_vars;
            *plonk_n_vars += 1;

            let qm = Coeff::zero();
            let ql = -c1.1.clone();
            let qr = -c2.1.clone();
            let qo = Coeff::one();
            let qc = Coeff::zero();

            plonk_constraints.push((sl, sr, so, qm.clone(), ql.clone(), qr.clone(), qo.clone(), qc.clone()));
            plonk_additions.push((sl, sr, c1.1, c2.1));
            cs.push((so, Coeff::one()));
        }

        let (mut s, mut coefs): (Vec<_>, Vec<_>) = cs.into_iter().unzip();
        while coefs.len() < max_c {
            s.push(0);
            coefs.push(Coeff::zero());
        }

        (k, s, coefs)
    }

    fn add_constraint_sum<F: Field>(
        lc: &LinearCombination<F>,
        plonk_constraints: &mut Vec<(u32, u32, u32, Coeff<F>, Coeff<F>, Coeff<F>, Coeff<F>, Coeff<F>)>,
        plonk_n_vars: &mut u32,
        plonk_additions: &mut Vec<(u32, u32, Coeff<F>, Coeff<F>)>,
    ) {
        let (k, s, coefs) = reduce_coefs(lc, 3, plonk_n_vars, plonk_constraints, plonk_additions);
        plonk_constraints.push((s[0], s[1], s[2], Coeff::zero(), coefs[0].clone(), coefs[1].clone(), coefs[2].clone(), k));
    }

    fn add_constraint_mul<F: Field>(
        a: &LinearCombination<F>,
        b: &LinearCombination<F>,
        c: &LinearCombination<F>,
        plonk_constraints: &mut Vec<(u32, u32, u32, Coeff<F>, Coeff<F>, Coeff<F>, Coeff<F>, Coeff<F>)>,
        plonk_n_vars: &mut u32,
        plonk_additions: &mut Vec<(u32, u32, Coeff<F>, Coeff<F>)>,
    ) {
        let (ka, sa, ca) = reduce_coefs(a, 1, plonk_n_vars, plonk_constraints, plonk_additions);
        let (kb, sb, cb) = reduce_coefs(b, 1, plonk_n_vars, plonk_constraints, plonk_additions);
        let (kc, sc, cc) = reduce_coefs(c, 1, plonk_n_vars, plonk_constraints, plonk_additions);

        let qm = ca[0].clone() * cb[0].clone();
        let ql = ca[0].clone() * kb.clone();
        let qr = ka.clone() * cb[0].clone();
        let qo = -cc[0].clone();
        let qc = ka * kb - kc;

        plonk_constraints.push((sa[0], sb[0], sc[0], qm, ql, qr, qo, qc));
    }

    fn get_lc_type<F: Field>(lc: &mut LinearCombination<F>) -> String {
        let mut k = Coeff::zero();
        let mut n = 0;
        let keys: Vec<_> = lc.keys().cloned().collect();
        for s in keys {
            if lc[&s].is_zero() {
                lc.remove(&s);
            } else if s == 0 {
                k = k + lc[&s].clone();
            } else {
                n += 1;
            }
        }
        if n > 0 {
            n.to_string()
        } else if !k.is_zero() {
            "k".to_string()
        } else {
            "0".to_string()
        }
    }

    fn process<F: Field>(
        mut a: LinearCombination<F>,
        mut b: LinearCombination<F>,
        mut c: LinearCombination<F>,
        plonk_constraints: &mut Vec<(u32, u32, u32, Coeff<F>, Coeff<F>, Coeff<F>, Coeff<F>, Coeff<F>)>,
        plonk_n_vars: &mut u32,
        plonk_additions: &mut Vec<(u32, u32, Coeff<F>, Coeff<F>)>,
    ) {
        let ta = get_lc_type(&mut a);
        let tb = get_lc_type(&mut b);
        if ta == "0" || tb == "0" {
            normalize(&mut c);
            add_constraint_sum(&c, plonk_constraints, plonk_n_vars, plonk_additions);
        } else if ta == "k" {
            let k = a.get(&0).unwrap();
            let cc = join(&b, k, &c);
            add_constraint_sum(&cc, plonk_constraints, plonk_n_vars, plonk_additions);
        } else if tb == "k" {
            let k = b.get(&0).unwrap();
            let cc = join(&a, k, &c);
            add_constraint_sum(&cc, plonk_constraints, plonk_n_vars, plonk_additions);
        } else {
            add_constraint_mul(&a, &b, &c, plonk_constraints, plonk_n_vars, plonk_additions);
        }
    }

    for s in 1..=n_public {
        plonk_constraints.push((s, 0, 0, Coeff::zero(), Coeff::one(), Coeff::zero(), Coeff::zero(), Coeff::zero()));
    }

    for constraint in &r1cs.constraints {
        let [a, b, c] = constraint;
        let a = a.iter().map(|(&k, v)| (k, Element::<F>::from(v.clone()))).collect();
        let b = b.iter().map(|(&k, v)| (k, Element::<F>::from(v.clone()))).collect();
        let c = c.iter().map(|(&k, v)| (k, Element::<F>::from(v.clone()))).collect();
        process(a, b, c, &mut plonk_constraints, &mut plonk_n_vars, &mut plonk_additions);
    }

    (plonk_constraints, plonk_additions)
}
