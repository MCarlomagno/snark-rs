use crate::file::R1cs;
use r1cs::Bn128;
use r1cs::Element;
use std::collections::HashMap;

pub fn process_constraints(
    r1cs: &mut R1cs,
) -> (
    Vec<(
        u32,
        u32,
        u32,
        Element<Bn128>,
        Element<Bn128>,
        Element<Bn128>,
        Element<Bn128>,
        Element<Bn128>,
    )>,
    Vec<(u32, u32, Element<Bn128>, Element<Bn128>)>,
) {
    type LinearCombination = HashMap<u32, Element<Bn128>>;

    let mut plonk_n_vars = r1cs.header.n_vars;
    let n_public = r1cs.header.n_outputs + r1cs.header.n_pub_inputs;

    let mut plonk_constraints: Vec<(
        u32,
        u32,
        u32,
        Element<Bn128>,
        Element<Bn128>,
        Element<Bn128>,
        Element<Bn128>,
        Element<Bn128>,
    )> = vec![];
    let mut plonk_additions: Vec<(u32, u32, Element<Bn128>, Element<Bn128>)> = vec![];

    fn normalize(lc: &mut LinearCombination) {
        lc.retain(|_, v| !v.is_zero());
    }

    fn join(
        lc1: &LinearCombination,
        k: &Element<Bn128>,
        lc2: &LinearCombination,
    ) -> LinearCombination {
        let mut res = HashMap::new();
        for (s, v) in lc1 {
            let val = k.clone() * v.clone();
            res.entry(*s)
                .and_modify(|e: &mut Element<Bn128>| *e = e.clone() + val.clone())
                .or_insert(val);
        }
        for (s, v) in lc2 {
            res.entry(*s)
                .and_modify(|e: &mut Element<Bn128>| *e = e.clone() + v.clone())
                .or_insert(v.clone());
        }
        normalize(&mut res);
        res
    }

    fn reduce_coefs(
        lc: &LinearCombination,
        max_c: usize,
        plonk_n_vars: &mut u32,
        plonk_constraints: &mut Vec<(
            u32,
            u32,
            u32,
            Element<Bn128>,
            Element<Bn128>,
            Element<Bn128>,
            Element<Bn128>,
            Element<Bn128>,
        )>,
        plonk_additions: &mut Vec<(u32, u32, Element<Bn128>, Element<Bn128>)>,
    ) -> (Element<Bn128>, Vec<u32>, Vec<Element<Bn128>>) {
        let mut k = Element::<Bn128>::zero();
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

            let qm = Element::<Bn128>::zero();
            let ql = -c1.1.clone();
            let qr = -c2.1.clone();
            let qo = Element::<Bn128>::one();
            let qc = Element::<Bn128>::zero();

            plonk_constraints.push((
                sl,
                sr,
                so,
                qm.clone(),
                ql.clone(),
                qr.clone(),
                qo.clone(),
                qc.clone(),
            ));
            plonk_additions.push((sl, sr, c1.1, c2.1));
            cs.push((so, Element::<Bn128>::one()));
        }

        let (mut s, mut coefs): (Vec<_>, Vec<_>) = cs.into_iter().unzip();
        while coefs.len() < max_c {
            s.push(0);
            coefs.push(Element::<Bn128>::zero());
        }

        (k, s, coefs)
    }

    fn add_constraint_sum(
        lc: &LinearCombination,
        plonk_constraints: &mut Vec<(
            u32,
            u32,
            u32,
            Element<Bn128>,
            Element<Bn128>,
            Element<Bn128>,
            Element<Bn128>,
            Element<Bn128>,
        )>,
        plonk_n_vars: &mut u32,
        plonk_additions: &mut Vec<(u32, u32, Element<Bn128>, Element<Bn128>)>,
    ) {
        let (k, s, coefs) = reduce_coefs(lc, 3, plonk_n_vars, plonk_constraints, plonk_additions);
        plonk_constraints.push((
            s[0],
            s[1],
            s[2],
            Element::<Bn128>::zero(),
            coefs[0].clone(),
            coefs[1].clone(),
            coefs[2].clone(),
            k,
        ));
    }

    fn add_constraint_mul(
        a: &LinearCombination,
        b: &LinearCombination,
        c: &LinearCombination,
        plonk_constraints: &mut Vec<(
            u32,
            u32,
            u32,
            Element<Bn128>,
            Element<Bn128>,
            Element<Bn128>,
            Element<Bn128>,
            Element<Bn128>,
        )>,
        plonk_n_vars: &mut u32,
        plonk_additions: &mut Vec<(u32, u32, Element<Bn128>, Element<Bn128>)>,
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

    fn get_lc_type(lc: &mut LinearCombination) -> String {
        let mut k = Element::<Bn128>::zero();
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

    fn process(
        mut a: LinearCombination,
        mut b: LinearCombination,
        mut c: LinearCombination,
        plonk_constraints: &mut Vec<(
            u32,
            u32,
            u32,
            Element<Bn128>,
            Element<Bn128>,
            Element<Bn128>,
            Element<Bn128>,
            Element<Bn128>,
        )>,
        plonk_n_vars: &mut u32,
        plonk_additions: &mut Vec<(u32, u32, Element<Bn128>, Element<Bn128>)>,
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
        plonk_constraints.push((
            s,
            0,
            0,
            Element::<Bn128>::zero(),
            Element::<Bn128>::one(),
            Element::<Bn128>::zero(),
            Element::<Bn128>::zero(),
            Element::<Bn128>::zero(),
        ));
    }

    let mut progress = 0;
    for constraint in &r1cs.constraints {
        let [a, b, c] = constraint;
        let a = a
            .iter()
            .map(|(&k, v)| (k, Element::<Bn128>::from(v.clone())))
            .collect();
        let b = b
            .iter()
            .map(|(&k, v)| (k, Element::<Bn128>::from(v.clone())))
            .collect();
        let c = c
            .iter()
            .map(|(&k, v)| (k, Element::<Bn128>::from(v.clone())))
            .collect();
        process(
            a,
            b,
            c,
            &mut plonk_constraints,
            &mut plonk_n_vars,
            &mut plonk_additions,
        );
        progress += 1;
        if progress % 1000 == 0 {
            println!(
                "ℹ️  Processed {}% of constraints",
                progress * 100 / r1cs.constraints.len()
            );
        }
    }

    (plonk_constraints, plonk_additions)
}
