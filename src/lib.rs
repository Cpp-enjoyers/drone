#![warn(clippy::pedantic)]

mod drone;

#[cfg(test)]
mod tests;

pub use drone::CppEnjoyersDrone;
