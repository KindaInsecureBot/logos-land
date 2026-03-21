// This crate embeds the RISC Zero guest methods at build time.
include!(concat!(env!("OUT_DIR"), "/methods.rs"));
