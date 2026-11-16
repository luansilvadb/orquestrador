//! Helpers de caminhos relativos com `/`.

/// Separa `pasta/nome` no último `/`: `("pasta", "nome")`, ou `("", caminho)` sem `/`.
pub fn dividir(caminho: &str) -> (&str, &str) {
    match caminho.rfind('/') {
        Some(i) => (&caminho[..i], &caminho[i + 1..]),
        None => ("", caminho),
    }
}

#[cfg(test)]
mod testes {
    use super::dividir;

    #[test]
    fn divide_no_ultimo_separador() {
        assert_eq!(dividir("src/lib/a.rs"), ("src/lib", "a.rs"));
        assert_eq!(dividir("a.rs"), ("", "a.rs"));
    }
}
