//! Documentacao do modulo.
use std::collections::HashMap;
use std::fmt;

/// Uma estrutura.
#[derive(Debug, Clone, PartialEq)]
pub struct Ponto<T> {
    pub x: T,
    y: f64,
}

pub enum Forma { Circulo(f32), Quadrado { lado: u32 } }

impl<T: fmt::Display> Ponto<T> {
    pub const ORIGEM: i32 = 0;
    pub fn novo(x: T) -> Self {
        Self { x, y: 1.5e3 }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut mapa: HashMap<&str, Vec<u8>> = HashMap::new();
    mapa.insert("chave", vec![1, 2, 3]);
    let s = format!("{} e {:?}", 'c', r"bruto");
    if let Some(v) = mapa.get("chave") { println!("{s} {}", v.len()); }
    for i in 0..10 { if i % 2 == 0 { continue; } }
    let f = |a: &mut i64| -> bool { *a += 1; true };
    Ok(())
}
