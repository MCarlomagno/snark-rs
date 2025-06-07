mod curves;
mod file;
mod utils;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ptau_path = "src/fixtures/pot24.ptau";
    let r1cs_path = "src/fixtures/email_auth.r1cs";

    // Step 1: Read PTAU file
    let (mut fd_ptau, sections_ptau) = file::read_bin_file(ptau_path, "ptau", 1).await?;

    // Step 2: Read header to extract curve and power
    let (curve, power, ceremony_power) =
        file::read_ptau_header(&mut fd_ptau, &sections_ptau).await?;
    println!(
        "Curve: {}, Power: {}, Ceremony Power: {}",
        curve.f1.n64, power, ceremony_power
    );

    // Step 3: Read R1CS file
    let (_fd_r1cs, _sections_r1cs) = file::read_bin_file(r1cs_path, "r1cs", 1).await?;

    // continue setup...
    Ok(())
}
