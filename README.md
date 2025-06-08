## Snark-rs

A simple library for PLONK circuit setup, proving and verification. Inspired in [snarkjs](https://github.com/iden3/snarkjs).

### Rationale

[snarkjs](https://github.com/iden3/snarkjs) is a very handy tool for setting up, proving and verifying PLONK circuits written in [Circom](https://github.com/iden3/circom).
 But in scenarios of large circuits, Javascript performance is not enough, PLONK key generation and proving for certain circuits is impractical due to memory allocation issues in the Javascript VM.

### Roadmap

- [ ] PLONK circuit setup
- [ ] PLONK circuit proving
- [ ] PLONK circuit verification

