//! `a.localeCompare(b, 'pt-BR')` do Chromium (ICU, força terciária): acentos, caixa, dígitos e pontuação.
use std::cmp::Ordering;
use std::sync::LazyLock;

use icu_collator::{Collator, CollatorBorrowed, options::CollatorOptions};

static COLADOR: LazyLock<CollatorBorrowed<'static>> = LazyLock::new(|| {
    Collator::try_new(icu_locale_core::locale!("pt-BR").into(), CollatorOptions::default())
        .expect("dados de colagem embutidos")
});

pub fn comparar(a: &str, b: &str) -> Ordering {
    COLADOR.compare(a, b)
}

#[cfg(test)]
mod testes {
    use super::*;

    fn ordenado(v: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = v.iter().map(|s| s.to_string()).collect();
        v.sort_by(|a, b| comparar(a, b));
        v
    }

    #[test]
    fn como_o_chromium() {
        // Saída de `[...].sort((a, b) => a.localeCompare(b, 'pt-BR'))` no Chromium (conferido no Node, mesmo ICU).
        assert_eq!(ordenado(&["assets/logo.png", "README.md", "src/a.ts"]), ["assets/logo.png", "README.md", "src/a.ts"]);
        assert_eq!(ordenado(&["b", "B", "a", "á", "A", "à"]), ["a", "A", "á", "à", "b", "B"]);
        assert_eq!(ordenado(&["é", "f", "e", "ê"]), ["e", "é", "ê", "f"]);
        assert_eq!(ordenado(&["a10", "a2", "a1"]), ["a1", "a10", "a2"]);
        assert_eq!(ordenado(&["a-b", "a_b", "a.b", "ab", "a b"]), ["a b", "a_b", "a-b", "a.b", "ab"]);
        assert_eq!(ordenado(&["z", "1", "_", "ç", "c"]), ["_", "1", "c", "ç", "z"]);
    }
}
