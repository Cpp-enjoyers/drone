#![warn(clippy::pedantic)]

mod drone;
mod ring_buffer;

#[cfg(test)]
mod tests;

pub use drone::CppEnjoyersDrone;
