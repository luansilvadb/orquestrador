//! Regras da tela sem interface: ocorrências por linha, corte do trecho, alinhamento da prévia, linhas da lista (lista e
//! árvore), resumo e histórico.
use orquestrador_core::ide::search::{FileMatches, Match, SkipReason};
use regex::Regex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

/// Mesmo limite do backend: com mais linhas casadas, só as primeiras aparecem e a lista avisa.
pub const LIMITE: usize = 2000;
pub const HISTORICO_MAX: usize = 20;
/// Como no VS Code: o trecho antes da ocorrência fica com no máximo 26 caracteres (cortado em limite de palavra, com "…").
pub const ANTES_MAX: usize = 26;
pub const DEPOIS_MAX: usize = 250;
/// Quantos arquivos o diálogo de confirmação lista antes de resumir.
pub const LISTA_CONFIRMAR: usize = 12;

pub fn motivo(r: SkipReason) -> &'static str {
    match r {
        SkipReason::Binary => "binário",
        SkipReason::NotUtf8 => "não é UTF-8",
        SkipReason::TooLarge => "maior que 1 MiB",
        SkipReason::NotFile => "não é arquivo comum",
        SkipReason::Unreadable => "não pôde ser lido",
        SkipReason::LooseCr => "tem CR solto (fora de CRLF)",
    }
}

/// Colunas em UTF-16, como o editor e o backend contam.
pub fn utf16(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

// `\w` do JavaScript: só ASCII.
fn palavra(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// O `lcut` do VS Code: tira os espaços do começo e, se passar de `max`, corta no começo em limite de palavra e põe "…".
pub fn lcut(texto: &str, max: usize) -> String {
    let t = texto.trim_start();
    let chars: Vec<char> = t.chars().collect();
    let n = chars.len();
    if n < max {
        return t.to_string();
    }
    let mut at = 0;
    for b in 0..=n {
        let antes = b > 0 && palavra(chars[b - 1]);
        let depois = b < n && palavra(chars[b]);
        if antes == depois {
            continue;
        }
        if n - b < max {
            break;
        }
        at = b;
    }
    if at == 0 {
        t.to_string()
    } else {
        let resto: String = chars[at..].iter().collect();
        format!("…{}", resto.trim_start())
    }
}

/// Uma ocorrência na lista. `trecho` é o intervalo (em bytes de `texto`) do que casou; `None` quando o padrão daqui não
/// achou nada na janela (a linha vira uma só). `col` é UTF-16, a partir de 1.
#[derive(Clone, Debug, PartialEq)]
pub struct Ocorrencia {
    pub caminho: Rc<str>,
    pub linha: u32,
    pub col: u32,
    pub texto: Rc<str>,
    pub trecho: Option<(usize, usize)>,
}

impl Ocorrencia {
    pub fn chave(&self) -> String {
        chave_ocorrencia(&self.caminho, self.linha, self.col)
    }
}

pub fn chave_ocorrencia(caminho: &str, linha: u32, col: u32) -> String {
    format!("h:{caminho}\u{0}{linha}\u{0}{col}")
}

pub fn chave_arquivo(caminho: &str) -> String {
    format!("f:{caminho}")
}

pub fn chave_pasta(caminho: &str) -> String {
    format!("d:{caminho}")
}

/// As ocorrências de uma linha do backend. A coluna de cada uma sai da coluna da primeira.
pub fn ocorrencias(caminho: &Rc<str>, m: &Match, padrao: Option<&Regex>) -> Vec<Ocorrencia> {
    let texto: Rc<str> = m.text.as_str().into();
    let achados: Vec<(usize, usize)> = padrao
        .map(|re| re.find_iter(&m.text).filter(|x| !x.is_empty()).map(|x| (x.start(), x.end())).collect())
        .unwrap_or_default();
    if achados.is_empty() {
        return vec![Ocorrencia { caminho: caminho.clone(), linha: m.line, col: m.col, texto, trecho: None }];
    }
    let primeira = achados[0].0;
    achados
        .into_iter()
        .map(|(a, b)| Ocorrencia {
            caminho: caminho.clone(),
            linha: m.line,
            col: m.col + utf16(&m.text[primeira..a]),
            texto: texto.clone(),
            trecho: Some((a, b)),
        })
        .collect()
}

/// O que entra no lugar de cada ocorrência de `antes`, lido de `depois` (a linha que o backend calculou). O texto entre as
/// ocorrências não muda e serve de âncora. Devolve coluna UTF-16 (a partir de 1) → substituto, ou `None` quando o
/// alinhamento é ambíguo.
pub fn substitutos(antes: &str, depois: &str, re: &Regex) -> Option<HashMap<u32, String>> {
    let achados: Vec<(usize, usize)> =
        re.find_iter(antes).filter(|x| !x.is_empty()).map(|x| (x.start(), x.end())).collect();
    if achados.is_empty() {
        return None;
    }
    let mut vaos = Vec::new();
    let mut ultimo = 0;
    for &(a, b) in &achados {
        vaos.push(&antes[ultimo..a]);
        ultimo = b;
    }
    let cauda = &antes[ultimo..];
    if !depois.starts_with(vaos[0]) || !depois.ends_with(cauda) || depois.len() < vaos[0].len() + cauda.len() {
        return None;
    }
    let fim = depois.len() - cauda.len();
    let mut out = HashMap::new();
    let mut pos = vaos[0].len();
    for (i, &(a, _)) in achados.iter().enumerate() {
        let vao = vaos.get(i + 1).copied();
        // Ocorrências coladas: não dá para saber onde uma termina.
        if vao == Some("") {
            return None;
        }
        let prox = match vao {
            None => fim,
            Some(v) => pos + depois.get(pos..)?.find(v)?,
        };
        if prox < pos || prox > fim {
            return None;
        }
        out.insert(utf16(&antes[..a]) + 1, depois[pos..prox].to_string());
        pos = prox + vao.map_or(0, str::len);
    }
    Some(out)
}

/// Linhas de um texto da prévia, sem o CR do fim. A substituição não cria nem junta linhas.
pub fn linhas(texto: &str) -> Vec<&str> {
    texto.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect()
}

/// Substitutos de um arquivo da prévia: linha → (coluna → texto), `None` na linha ambígua.
pub type SubstitutosArquivo = HashMap<u32, Option<HashMap<u32, String>>>;

pub fn substitutos_arquivo(original: &str, resultado: &str, re: &Regex) -> SubstitutosArquivo {
    let antes = linhas(original);
    let depois = linhas(resultado);
    let mut out = HashMap::new();
    for (i, b) in antes.iter().enumerate() {
        let a = depois.get(i).copied().unwrap_or("");
        if *b != a {
            out.insert(i as u32 + 1, substitutos(b, a, re));
        }
    }
    out
}

/// Trechos separados por vírgula, sem espaço nas pontas e sem os vazios.
pub fn termos(texto: &str) -> Vec<String> {
    texto.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect()
}

pub fn plural(n: usize, um: &str, muitos: &str) -> String {
    format!("{n} {}", if n == 1 { um } else { muitos })
}

/// Resumo: contagem de resultados e de arquivos, com o aviso do limite quando passou dele.
pub fn resumo(ocorrencias: usize, arquivos: usize, truncado: bool) -> String {
    if ocorrencias == 0 {
        return "Nenhum resultado.".into();
    }
    let t = format!("{} em {}", plural(ocorrencias, "resultado", "resultados"), plural(arquivos, "arquivo", "arquivos"));
    if truncado {
        format!("{t}. Só as {LIMITE} primeiras linhas aparecem.")
    } else {
        t
    }
}

/// A consulta entra no topo, sem repetição, até o máximo.
pub fn lembrar(historico: &mut Vec<String>, consulta: &str) {
    historico.retain(|q| q != consulta);
    historico.insert(0, consulta.to_string());
    historico.truncate(HISTORICO_MAX);
}

/// Pasta e nome de um caminho com `/`.
pub use orquestrador_ui::caminhos::dividir;

/// Texto de um arquivo para a área de transferência: caminho e, depois, "  linha: texto" por linha com resultado.
pub fn texto_bloco(caminho: &str, ocorrencias: &[Ocorrencia]) -> String {
    let mut linhas: Vec<(u32, &str)> = Vec::new();
    for o in ocorrencias {
        match linhas.iter_mut().find(|(l, _)| *l == o.linha) {
            Some(x) => x.1 = &o.texto,
            None => linhas.push((o.linha, &o.texto)),
        }
    }
    let mut out = caminho.to_string();
    for (l, t) in linhas {
        out.push_str(&format!("\n  {l}: {}", t.trim_start()));
    }
    out
}

/// As chaves das pastas da árvore acima de um caminho: abrir todas mostra o arquivo.
pub fn ancestrais(caminho: &str) -> Vec<String> {
    let (pasta, _) = dividir(caminho);
    let partes: Vec<&str> = pasta.split('/').filter(|p| !p.is_empty()).collect();
    (0..partes.len()).map(|i| chave_pasta(&partes[..=i].join("/"))).collect()
}

/// Um arquivo na tela, com as ocorrências que não foram dispensadas.
#[derive(Clone, Debug)]
pub struct ArquivoVisivel {
    pub caminho: Rc<str>,
    pub ocorrencias: Vec<Ocorrencia>,
}

/// Arquivos na tela (sem os dispensados) e os que têm alguma ocorrência dispensada (`parcial`).
pub fn visiveis(
    todos: &[ArquivoVisivel],
    arquivos_fora: &HashSet<String>,
    ocorrencias_fora: &HashSet<String>,
) -> (Vec<ArquivoVisivel>, HashSet<String>) {
    let mut parcial = HashSet::new();
    let mut out = Vec::new();
    for f in todos.iter().filter(|f| !arquivos_fora.contains(&*f.caminho)) {
        let ficam: Vec<Ocorrencia> =
            f.ocorrencias.iter().filter(|o| !ocorrencias_fora.contains(&o.chave())).cloned().collect();
        if ficam.len() != f.ocorrencias.len() {
            parcial.insert(f.caminho.to_string());
        }
        if !ficam.is_empty() {
            out.push(ArquivoVisivel { caminho: f.caminho.clone(), ocorrencias: ficam });
        }
    }
    (out, parcial)
}

/// Todas as ocorrências da resposta, já divididas pelo padrão.
pub fn todas(arquivos: &[FileMatches], padrao: Option<&Regex>) -> Vec<ArquivoVisivel> {
    arquivos
        .iter()
        .map(|f| {
            let caminho: Rc<str> = f.path.as_str().into();
            let ocorrencias = f.matches.iter().flat_map(|m| ocorrencias(&caminho, m, padrao)).collect();
            ArquivoVisivel { caminho, ocorrencias }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visao {
    #[default]
    Lista,
    Arvore,
}

/// Uma linha da lista na tela: pasta (só na árvore), arquivo ou ocorrência. Índices apontam para os arquivos visíveis.
#[derive(Clone, Debug, PartialEq)]
pub enum Linha {
    Pasta { chave: String, caminho: String, rotulo: String, nivel: usize, total: usize },
    Arquivo { chave: String, arquivo: usize, nivel: usize, arvore: bool },
    Ocorrencia { chave: String, arquivo: usize, indice: usize, nivel: usize },
}

impl Linha {
    pub fn chave(&self) -> &str {
        match self {
            Linha::Pasta { chave, .. } | Linha::Arquivo { chave, .. } | Linha::Ocorrencia { chave, .. } => chave,
        }
    }
    pub fn nivel(&self) -> usize {
        match self {
            Linha::Pasta { nivel, .. } | Linha::Arquivo { nivel, .. } | Linha::Ocorrencia { nivel, .. } => *nivel,
        }
    }
}

/// Põe o arquivo `i` (e as ocorrências, se aberto) no nível dado.
type Empurrar<'a> = dyn Fn(&mut Vec<Linha>, usize, usize, bool) + 'a;

#[derive(Default)]
struct Pasta {
    nome: String,
    caminho: String,
    pastas: BTreeMap<String, Pasta>,
    arquivos: Vec<usize>,
}

/// As linhas da tela, na ordem. Na árvore, pastas com uma só subpasta e sem arquivo ficam numa linha só ("src/components").
pub fn linhas_da_tela(arquivos: &[ArquivoVisivel], visao: Visao, recolhidos: &HashSet<String>) -> Vec<Linha> {
    let mut out = Vec::new();
    let empurrar = |out: &mut Vec<Linha>, i: usize, nivel: usize, arvore: bool| {
        let chave = chave_arquivo(&arquivos[i].caminho);
        let aberto = !recolhidos.contains(&chave);
        out.push(Linha::Arquivo { chave, arquivo: i, nivel, arvore });
        if aberto {
            for (j, o) in arquivos[i].ocorrencias.iter().enumerate() {
                out.push(Linha::Ocorrencia { chave: o.chave(), arquivo: i, indice: j, nivel: nivel + 1 });
            }
        }
    };
    if visao == Visao::Lista {
        for i in 0..arquivos.len() {
            empurrar(&mut out, i, 0, false);
        }
        return out;
    }
    let mut raiz = Pasta::default();
    for (i, f) in arquivos.iter().enumerate() {
        let mut no = &mut raiz;
        for parte in dividir(&f.caminho).0.split('/').filter(|p| !p.is_empty()) {
            let caminho = if no.caminho.is_empty() { parte.to_string() } else { format!("{}/{parte}", no.caminho) };
            no = no.pastas.entry(parte.to_string()).or_insert_with(|| Pasta {
                nome: parte.into(),
                caminho,
                ..Pasta::default()
            });
        }
        no.arquivos.push(i);
    }
    fn total(p: &Pasta, arquivos: &[ArquivoVisivel]) -> usize {
        p.arquivos.iter().map(|&i| arquivos[i].ocorrencias.len()).sum::<usize>()
            + p.pastas.values().map(|c| total(c, arquivos)).sum::<usize>()
    }
    fn andar(
        p: &Pasta,
        nivel: usize,
        arquivos: &[ArquivoVisivel],
        recolhidos: &HashSet<String>,
        out: &mut Vec<Linha>,
        empurrar: &Empurrar<'_>,
    ) {
        // `localeCompare` sem localidade: a do navegador, pt-BR no WebView2.
        let mut filhos: Vec<&Pasta> = p.pastas.values().collect();
        filhos.sort_by(|a, b| orquestrador_ui::colagem::comparar(&a.nome, &b.nome));
        for filho in filhos {
            let mut no = filho;
            let mut rotulo = filho.nome.clone();
            while no.arquivos.is_empty() && no.pastas.len() == 1 {
                no = no.pastas.values().next().unwrap();
                rotulo.push('/');
                rotulo.push_str(&no.nome);
            }
            let chave = chave_pasta(&no.caminho);
            let aberta = !recolhidos.contains(&chave);
            out.push(Linha::Pasta { chave, caminho: no.caminho.clone(), rotulo, nivel, total: total(no, arquivos) });
            if aberta {
                andar(no, nivel + 1, arquivos, recolhidos, out, empurrar);
            }
        }
        for &i in &p.arquivos {
            empurrar(out, i, nivel, true);
        }
    }
    andar(&raiz, 0, arquivos, recolhidos, &mut out, &empurrar);
    out
}

/// A linha seguinte à que sai (pulando as de dentro, se for arquivo ou pasta) ou, no fim, a anterior.
pub fn vizinha(linhas: &[Linha], indice: usize) -> Option<String> {
    let l = &linhas[indice];
    let mut fim = indice + 1;
    if !matches!(l, Linha::Ocorrencia { .. }) {
        while fim < linhas.len() && linhas[fim].nivel() > l.nivel() {
            fim += 1;
        }
    }
    linhas
        .get(fim)
        .or_else(|| indice.checked_sub(1).and_then(|i| linhas.get(i)))
        .map(|l| l.chave().to_string())
}

/// Próxima ocorrência do F4 (`dir` 1) ou Shift+F4 (`dir` -1), com volta, na ordem da tela.
pub fn passo(arquivos: &[ArquivoVisivel], atual: Option<(&str, u32, u32)>, dir: i32) -> Option<Ocorrencia> {
    let todas: Vec<&Ocorrencia> = arquivos.iter().flat_map(|f| f.ocorrencias.iter()).collect();
    if todas.is_empty() {
        return None;
    }
    let n = todas.len() as i64;
    let idx = atual.and_then(|(c, l, col)| todas.iter().position(|o| &*o.caminho == c && o.linha == l && o.col == col));
    let i = match idx {
        None => if dir > 0 { 0 } else { n - 1 },
        Some(i) => (i as i64 + dir as i64 + n) % n,
    };
    Some(todas[i as usize].clone())
}

#[cfg(test)]
mod testes {
    use super::*;
    use orquestrador_core::ide::search::{compile, Query};

    fn re(texto: &str, palavra: bool, regex: bool) -> Regex {
        compile(&Query { text: texto.into(), whole_word: palavra, regex, ..Query::default() }).unwrap()
    }

    fn m(line: u32, col: u32, text: &str) -> Match {
        Match { line, col, text: text.into() }
    }

    #[test]
    fn uma_linha_por_ocorrencia_com_a_coluna_de_cada() {
        // termo-b.ts do teste de interface: duas ocorrências na mesma linha, colunas 9 e 17.
        let c: Rc<str> = "src/termo-b.ts".into();
        let os = ocorrencias(&c, &m(1, 9, "let x = termo + termo;"), Some(&re("termo", false, false)));
        assert_eq!(os.iter().map(|o| o.col).collect::<Vec<_>>(), vec![9, 17]);
        assert_eq!(os[0].trecho, Some((8, 13)));
        // Sem casamento aqui (regex que só o backend entende), a linha vira uma só.
        let os = ocorrencias(&c, &m(3, 2, "abc"), Some(&re("zzz", false, false)));
        assert_eq!(os.len(), 1);
        assert_eq!(os[0].trecho, None);
        // Coluna em UTF-16, depois de caractere fora do BMP.
        let os = ocorrencias(&c, &m(1, 1, "a 😀 a"), Some(&re("a", false, false)));
        assert_eq!(os.iter().map(|o| o.col).collect::<Vec<_>>(), vec![1, 6]);
    }

    #[test]
    fn lcut_como_o_vscode() {
        assert_eq!(lcut("   curto", 26), "curto");
        // Corta no último limite de palavra que ainda deixa pelo menos `max` caracteres.
        assert_eq!(lcut("uma frase bastante comprida antes do termo", 26), "…bastante comprida antes do termo");
        assert_eq!(lcut("palavraunicamuitolongasemlimitenenhum", 26), "palavraunicamuitolongasemlimitenenhum");
    }

    #[test]
    fn substitutos_lidos_da_previa() {
        let r = re("termo", false, false);
        let s = substitutos("let x = termo + termo;", "let x = novo + novo;", &r).unwrap();
        assert_eq!(s.get(&9).map(String::as_str), Some("novo"));
        assert_eq!(s.get(&17).map(String::as_str), Some("novo"));
        // Regex com captura: "v(\d+)" → "$1.0".
        let r = re(r"v(\d+)", false, true);
        let s = substitutos("v1 v22", "1.0 22.0", &r).unwrap();
        let mut v: Vec<_> = s.into_iter().collect();
        v.sort();
        assert_eq!(v, vec![(1, "1.0".to_string()), (4, "22.0".to_string())]);
        // Vazio apaga só as ocorrências inteiras.
        let r = re("termo", true, false);
        let s = substitutos("a termo b termo_x termo c", "a  b termo_x  c", &r).unwrap();
        assert_eq!(s.len(), 2);
        assert!(s.values().all(String::is_empty));
        // Ocorrências coladas são ambíguas.
        assert_eq!(substitutos("aa", "bb", &re("a", false, false)), None);
    }

    #[test]
    fn preservar_caixa_aparece_na_previa() {
        let r = re("termo", false, false);
        let s = substitutos("Termo termo TERMO", "Novo novo NOVO", &r).unwrap();
        let mut v: Vec<_> = s.into_iter().collect();
        v.sort();
        assert_eq!(v.into_iter().map(|x| x.1).collect::<Vec<_>>(), vec!["Novo", "novo", "NOVO"]);
    }

    #[test]
    fn resumo_e_plural() {
        assert_eq!(resumo(0, 0, false), "Nenhum resultado.");
        assert_eq!(resumo(3, 2, false), "3 resultados em 2 arquivos");
        assert_eq!(resumo(1, 1, false), "1 resultado em 1 arquivo");
        assert_eq!(resumo(2000, 1, true), "2000 resultados em 1 arquivo. Só as 2000 primeiras linhas aparecem.");
        assert_eq!(termos(" *.ts, ,./src "), vec!["*.ts", "./src"]);
    }

    #[test]
    fn historico_sem_repeticao_e_limitado() {
        let mut h = Vec::new();
        for i in 0..25 {
            lembrar(&mut h, &format!("q{i}"));
        }
        lembrar(&mut h, "q20");
        assert_eq!(h.len(), HISTORICO_MAX);
        assert_eq!(h[0], "q20");
        assert_eq!(h.iter().filter(|q| *q == "q20").count(), 1);
    }

    fn arq(caminho: &str, n: usize) -> ArquivoVisivel {
        let c: Rc<str> = caminho.into();
        let ocorrencias = (0..n)
            .map(|i| Ocorrencia { caminho: c.clone(), linha: i as u32 + 1, col: 1, texto: "x".into(), trecho: None })
            .collect();
        ArquivoVisivel { caminho: c, ocorrencias }
    }

    #[test]
    fn pastas_na_ordem_do_locale_compare() {
        let fs: Vec<_> = ["b/x", "Á/x", "a10/x", "a2/x", "a/x", "_/x"].iter().map(|c| arq(c, 1)).collect();
        let ls = linhas_da_tela(&fs, Visao::Arvore, &HashSet::new());
        let pastas: Vec<_> = ls
            .iter()
            .filter_map(|l| match l {
                Linha::Pasta { rotulo, .. } => Some(rotulo.as_str()),
                _ => None,
            })
            .collect();
        // `[...].sort((a, b) => a.localeCompare(b))` no Chromium pt-BR.
        assert_eq!(pastas, ["_", "a", "Á", "a10", "a2", "b"]);
    }

    #[test]
    fn arvore_comprime_pasta_e_conta_o_de_dentro() {
        // src/arv/um.ts, src/arv/dois.ts e docs/tres.md: "src/arv" vira uma linha só com 2.
        let fs = vec![arq("src/arv/um.ts", 1), arq("src/arv/dois.ts", 1), arq("docs/tres.md", 1)];
        let ls = linhas_da_tela(&fs, Visao::Arvore, &HashSet::new());
        let pastas: Vec<_> = ls
            .iter()
            .filter_map(|l| match l {
                Linha::Pasta { rotulo, total, caminho, .. } => Some((rotulo.as_str(), *total, caminho.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(pastas, vec![("docs", 1, "docs"), ("src/arv", 2, "src/arv")]);
        // Recolher a pasta tira os arquivos dela.
        let rec: HashSet<String> = [chave_pasta("src/arv")].into();
        let ls = linhas_da_tela(&fs, Visao::Arvore, &rec);
        assert!(!ls.iter().any(|l| matches!(l, Linha::Arquivo { arquivo: 0, .. })));
        assert!(ls.iter().any(|l| matches!(l, Linha::Arquivo { arquivo: 2, .. })));
        // Lista: arquivo e ocorrências, sem pastas.
        let ls = linhas_da_tela(&fs, Visao::Lista, &HashSet::new());
        assert_eq!(ls.len(), 6);
        assert_eq!(ancestrais("src/arv/um.ts"), vec!["d:src", "d:src/arv"]);
    }

    #[test]
    fn dispensar_tira_da_tela_e_marca_parcial() {
        let fs = vec![arq("src/disp-a.ts", 2), arq("src/disp-b.ts", 1)];
        let fora: HashSet<String> = [chave_ocorrencia("src/disp-a.ts", 2, 1)].into();
        let (v, parcial) = visiveis(&fs, &HashSet::new(), &fora);
        assert_eq!(v.iter().map(|f| f.ocorrencias.len()).sum::<usize>(), 2);
        assert!(parcial.contains("src/disp-a.ts"));
        let arquivos: HashSet<String> = ["src/disp-b.ts".to_string()].into();
        let (v, _) = visiveis(&fs, &arquivos, &fora);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn vizinha_pula_o_de_dentro_e_no_fim_volta() {
        let fs = vec![arq("a.ts", 2), arq("b.ts", 1)];
        let ls = linhas_da_tela(&fs, Visao::Lista, &HashSet::new());
        assert_eq!(vizinha(&ls, 0).as_deref(), Some("f:b.ts"));
        assert_eq!(vizinha(&ls, 4), Some(ls[3].chave().to_string()));
    }

    #[test]
    fn f4_anda_com_volta() {
        let fs = vec![arq("f4.ts", 2)];
        assert_eq!(passo(&fs, None, 1).unwrap().linha, 1);
        assert_eq!(passo(&fs, None, -1).unwrap().linha, 2);
        assert_eq!(passo(&fs, Some(("f4.ts", 2, 1)), 1).unwrap().linha, 1);
        assert_eq!(passo(&fs, Some(("f4.ts", 1, 1)), -1).unwrap().linha, 2);
    }

    #[test]
    fn bloco_para_copiar() {
        let fs = arq("a.ts", 2);
        assert_eq!(texto_bloco("a.ts", &fs.ocorrencias), "a.ts\n  1: x\n  2: x");
    }
}
