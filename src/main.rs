mod curves;
mod file;
mod utils;
mod r1cs;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ptau_path = "src/fixtures/pot24.ptau";
    let r1cs_path = "src/fixtures/email_auth.r1cs";

    let (mut fd_ptau, sections_ptau) = file::read_bin_file(ptau_path, "ptau", 1).await?;

    let (curve, power, ceremony_power) =
        file::read_ptau_header(&mut fd_ptau, &sections_ptau).await?;
    println!(
        "Curve: {}, Power: {}, Ceremony Power: {}",
        curve.f1.n64, power, ceremony_power
    );

    let (mut fd_r1cs, sections_r1cs) = file::read_bin_file(r1cs_path, "r1cs", 1).await?;

    let mut r1cs = file::read_r1cs_fd(&mut fd_r1cs, &sections_r1cs).await?;
    println!("R1CS constraints: {}", r1cs.header.n_constraints);

    let s_g1 = curve.n8q * 2;
    let s_g2 = curve.n8q * 4;
    let n8r = curve.n8r;

    let s_r1cs = file::read_section(&mut fd_r1cs, &sections_r1cs, 2, None, None).await?;

    let plonk_n_vars = r1cs.header.n_vars;
    let n_public = r1cs.header.n_outputs + r1cs.header.n_pub_inputs;

    println!("Plonk n_vars: {}, n_public: {}", plonk_n_vars, n_public);

    let constraints = r1cs::process_constraints(curve.fr, &mut r1cs);

    match constraints {
        r1cs::ConstraintOutput::Bn128(constraints, additions) => {
            println!("bn128 constraints: {}", constraints.len());
            println!("bn128 additions: {}", additions.len());
        }
        r1cs::ConstraintOutput::Bls12_381(constraints, additions) => {
            println!("bls12_381 constraints: {}", constraints.len());
            println!("bls12_381 additions: {}", additions.len());
        }
    }
    println!("Plonk constraints processed");
    

    Ok(())
}
