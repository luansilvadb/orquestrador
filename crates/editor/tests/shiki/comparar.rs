//! Compara o realce com o do Shiki dark-plus nas amostras desta pasta (`amostra.<ext>.cores`: por linha, `RRGGBB:n`
//! para cada token do `codeToTokensBase`). Mede a fração de caracteres visíveis com a mesma cor, sem os colchetes.
use super::*;

/// Fração mínima por extensão (a alcançada; não pode regredir).
const MINIMO: &[(&str, f32)] = &[
    ("c", 0.9385),
    ("cpp", 0.7195),
    ("cs", 0.7695),
    ("css", 0.9995),
    ("dart", 0.9995),
    ("go", 0.8685),
    ("html", 0.9345),
    ("java", 0.9485),
    ("js", 0.9995),
    ("json", 0.9995),
    ("jsx", 0.9995),
    ("kt", 0.8415),
    ("lua", 0.9825),
    ("md", 0.9335),
    ("php", 0.8995),
    ("ps1", 0.9995),
    ("py", 0.9995),
    ("rb", 0.9445),
    ("rs", 0.9995),
    ("scss", 0.9865),
    ("sh", 0.7645),
    ("sql", 0.9635),
    ("swift", 0.7475),
    ("toml", 0.9075),
    ("ts", 0.9995),
    ("tsx", 0.9995),
    ("vue", 0.8885),
    ("xml", 0.9995),
    ("yaml", 0.9925),
];

fn hex(c: Hsla) -> String {
    let r = Rgba::from(c);
    format!("{:02X}{:02X}{:02X}", (r.r * 255.).round() as u8, (r.g * 255.).round() as u8, (r.b * 255.).round() as u8)
}

/// Fração de acerto e as diferenças (linha, trecho, esperado, obtido).
fn comparar(ext: &str) -> (f32, Vec<(usize, String, String, String)>) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/shiki/");
    let texto = std::fs::read_to_string(format!("{dir}amostra.{ext}")).unwrap();
    let cores = std::fs::read_to_string(format!("{dir}amostra.{ext}.cores")).unwrap();
    let caminho = format!("amostra.{ext}");
    let lang = crate::linguagem::linguagem(&caminho);
    let s = lang.as_deref().and_then(|l| sintaxe(l, &caminho));
    let tr = colorir(s, &texto);
    let cor_em = |i: usize| {
        let k = tr.partition_point(|(r, _)| r.end <= i);
        tr.get(k).filter(|(r, _)| r.contains(&i)).and_then(|(_, s)| s.color).map(hex).unwrap_or_else(|| "D4D4D4".into())
    };
    let (mut total, mut certos) = (0usize, 0usize);
    let mut erros = Vec::new();
    let mut base = 0;
    for (n, (linha, ref_linha)) in texto.split_inclusive('\n').zip(cores.lines()).enumerate() {
        let mut i = base;
        for tok in ref_linha.split_whitespace() {
            let (cor, len) = tok.split_once(':').unwrap();
            let len: usize = len.parse().unwrap();
            let pedaco = &texto[i..i + len];
            let mut errado = None;
            for (j, ch) in pedaco.char_indices() {
                if ch.is_whitespace() || "()[]{}".contains(ch) {
                    continue;
                }
                total += 1;
                let obtida = cor_em(i + j);
                if obtida == cor {
                    certos += 1;
                } else {
                    errado.get_or_insert(obtida);
                }
            }
            if let Some(o) = errado {
                erros.push((n + 1, pedaco.to_string(), cor.to_string(), o));
            }
            i += len;
        }
        base += linha.len();
    }
    (if total == 0 { 1. } else { certos as f32 / total as f32 }, erros)
}

#[test]
fn cores_iguais_as_do_shiki() {
    let mut falhas = Vec::new();
    for (ext, minimo) in MINIMO {
        let (fracao, erros) = comparar(ext);
        eprintln!("{ext}: {:.1}%", fracao * 100.);
        if std::env::var_os("SHIKI_DETALHE").is_some_and(|v| v == *ext) {
            for e in &erros {
                eprintln!("  {e:?}");
            }
        }
        if fracao + 1e-4 < *minimo {
            falhas.push(format!("{ext}: {:.1}% < {:.1}%", fracao * 100., minimo * 100.));
        }
    }
    assert!(falhas.is_empty(), "{falhas:?}");
}

/// Tempo de montar o conjunto de gramáticas e de colorir as amostras (`--ignored --nocapture`).
#[test]
#[ignore]
fn medir_tempo() {
    let ini = std::time::Instant::now();
    let _ = &*SINTAXES;
    eprintln!("conjunto: {:?}", ini.elapsed());
    for (ext, _) in MINIMO {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/shiki/");
        let texto = std::fs::read_to_string(format!("{dir}amostra.{ext}")).unwrap().repeat(20);
        let caminho = format!("amostra.{ext}");
        let s = crate::linguagem::linguagem(&caminho).as_deref().and_then(|l| sintaxe(l, &caminho));
        let _ = colorir(s, "x\n");
        let ini = std::time::Instant::now();
        let _ = colorir(s, &texto);
        eprintln!("{ext}: {:.3} ms/linha", ini.elapsed().as_secs_f64() * 1000. / texto.lines().count() as f64);
    }
}

/// Monta o conjunto: o do `two-face` mais as gramáticas de `assets/gramaticas`.
fn montar_sintaxes() -> SyntaxSet {
    let mut b = two_face::syntax::extra_newlines().into_builder();
    for (nome, texto) in include!("../../assets/gramaticas/lista.rs") {
        let d = syntect::parsing::SyntaxDefinition::load_from_str(texto, true, None).unwrap_or_else(|e| panic!("{nome}: {e}"));
        b.add(d);
    }
    b.build()
}

/// Refaz `assets/sintaxes.packdump` depois de mudar `assets/gramaticas` (`--ignored`).
#[test]
#[ignore]
fn gerar_sintaxes() {
    let caminho = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sintaxes.packdump");
    std::fs::write(caminho, syntect::dumps::dump_binary(&montar_sintaxes())).unwrap();
}

#[test]
fn packdump_em_dia_com_as_gramaticas() {
    let nomes = |s: &SyntaxSet| s.syntaxes().iter().map(|x| (x.name.clone(), x.scope)).collect::<Vec<_>>();
    assert_eq!(nomes(&SINTAXES), nomes(&montar_sintaxes()), "rode o teste ignorado gerar_sintaxes");
}
