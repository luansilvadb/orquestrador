//! Modelo do comparador contínuo (porte de `src/editor/compare-rows.ts` e dos blocos de patch de `workspace-store.ts`): o
//! arquivo inteiro (patch com todo o contexto) vira linhas, em lado a lado e unificado. Cada linha de mudança aponta para a
//! sua unidade: o bloco do Git (-U3), que é onde as ações agem, ou, sem blocos (commit), cada trecho de mudança.

/// Uma linha do lado a lado: o número da linha de cada lado. `unidade` é `None` no contexto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinhaLado {
    pub esquerda: Option<usize>,
    pub direita: Option<usize>,
    pub unidade: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    Contexto,
    Removida,
    Incluida,
}

/// Uma linha do unificado: os números de cada lado, `None` onde a linha não existe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinhaUnificada {
    pub tipo: Tipo,
    pub antiga: Option<usize>,
    pub nova: Option<usize>,
    pub unidade: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipoUnidade {
    Inclusao,
    Remocao,
    Troca,
}

/// Uma unidade de navegação: a primeira linha do arquivo novo, as linhas (inclusive) em cada modo e o que ela muda.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unidade {
    pub linha: usize,
    pub lado: Option<(usize, usize)>,
    pub unificado: Option<(usize, usize)>,
    pub tipo: TipoUnidade,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Comparacao {
    pub lado: Vec<LinhaLado>,
    pub unificado: Vec<LinhaUnificada>,
    pub unidades: Vec<Unidade>,
}

/// O intervalo de um bloco do patch (-U3): início e tamanho de cada lado, como no cabeçalho `@@`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Intervalo {
    pub antigo: usize,
    pub antigo_n: usize,
    pub novo: usize,
    pub novo_n: usize,
}

/// Lê `@@ -a[,b] +c[,d] @@`.
pub fn cabecalho(linha: &str) -> Option<Intervalo> {
    let resto = linha.strip_prefix("@@ -")?;
    let fim = resto.find(" @@")?;
    let (velho, novo) = resto[..fim].split_once(" +")?;
    let par = |s: &str| -> Option<(usize, usize)> {
        match s.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let (antigo, antigo_n) = par(velho)?;
    let (novo, novo_n) = par(novo)?;
    Some(Intervalo { antigo, antigo_n, novo, novo_n })
}

/// Os intervalos dos blocos de um patch, na ordem em que aparecem (a mesma numeração da revisão).
pub fn intervalos(patch: &str) -> Vec<Intervalo> {
    patch.split('\n').filter_map(cabecalho).collect()
}

/// As chaves das linhas de mudança, `d` + número antigo e `a` + número novo; contexto não tem chave.
pub fn chaves(linha: &LinhaLado) -> Vec<String> {
    if linha.unidade.is_none() {
        return Vec::new();
    }
    linha.esquerda.map(|n| format!("d{n}")).into_iter().chain(linha.direita.map(|n| format!("a{n}"))).collect()
}

pub fn chave_unificada(linha: &LinhaUnificada) -> Option<String> {
    linha.unidade?;
    match linha.tipo {
        Tipo::Incluida => linha.nova.map(|n| format!("a{n}")),
        _ => linha.antiga.map(|n| format!("d{n}")),
    }
}

/// O comparador de um patch com o arquivo inteiro. `blocos` são os blocos (-U3) das unidades; sem eles, cada trecho de
/// mudança (linhas seguidas que não são contexto) é uma unidade.
pub fn comparar(completo: &str, blocos: Option<&[Intervalo]>) -> Comparacao {
    let mut unificado = Vec::new();
    let (mut o, mut n, mut dentro) = (0usize, 0usize, false);
    for bruta in completo.split('\n') {
        if let Some(h) = cabecalho(bruta) {
            o = h.antigo;
            n = h.novo;
            dentro = true;
            continue;
        }
        if !dentro || bruta.is_empty() {
            continue;
        }
        match bruta.as_bytes()[0] {
            b' ' => {
                unificado.push(LinhaUnificada { tipo: Tipo::Contexto, antiga: Some(o), nova: Some(n), unidade: None });
                o += 1;
                n += 1;
            }
            b'-' => {
                unificado.push(LinhaUnificada { tipo: Tipo::Removida, antiga: Some(o), nova: None, unidade: None });
                o += 1;
            }
            b'+' => {
                unificado.push(LinhaUnificada { tipo: Tipo::Incluida, antiga: None, nova: Some(n), unidade: None });
                n += 1;
            }
            _ => {}
        }
    }
    let mut trecho: isize = -1;
    let mut anterior = Tipo::Contexto;
    for l in unificado.iter_mut() {
        if l.tipo == Tipo::Contexto {
            anterior = Tipo::Contexto;
            continue;
        }
        if anterior == Tipo::Contexto {
            trecho += 1;
        }
        anterior = l.tipo;
        l.unidade = match blocos {
            None => Some(trecho as usize),
            Some(rs) => {
                if l.tipo == Tipo::Removida {
                    let v = l.antiga.unwrap_or(0);
                    rs.iter().position(|r| r.antigo_n > 0 && v >= r.antigo && v < r.antigo + r.antigo_n)
                } else {
                    let v = l.nova.unwrap_or(0);
                    rs.iter().position(|r| r.novo_n > 0 && v >= r.novo && v < r.novo + r.novo_n)
                }
            }
        };
    }
    // Lado a lado: contexto nos dois lados; cada trecho alinha remoções com inclusões e o que sobra fica vazio.
    let mut lado = Vec::new();
    let mut i = 0;
    while i < unificado.len() {
        let l = unificado[i];
        if l.tipo == Tipo::Contexto {
            lado.push(LinhaLado { esquerda: l.antiga, direita: l.nova, unidade: None });
            i += 1;
            continue;
        }
        let mut j = i;
        while j < unificado.len() && unificado[j].tipo != Tipo::Contexto {
            j += 1;
        }
        let dels: Vec<_> = unificado[i..j].iter().filter(|x| x.tipo == Tipo::Removida).collect();
        let adds: Vec<_> = unificado[i..j].iter().filter(|x| x.tipo == Tipo::Incluida).collect();
        for k in 0..dels.len().max(adds.len()) {
            let (d, a) = (dels.get(k), adds.get(k));
            lado.push(LinhaLado {
                esquerda: d.and_then(|d| d.antiga),
                direita: a.and_then(|a| a.nova),
                unidade: d.or(a).and_then(|x| x.unidade),
            });
        }
        i = j;
    }
    let total = match blocos {
        Some(rs) => rs.len(),
        None => (trecho + 1) as usize,
    };
    let faixa = |unidades: &mut dyn Iterator<Item = Option<usize>>, u: usize| -> Option<(usize, usize)> {
        let marcadas: Vec<usize> = unidades.enumerate().filter(|(_, x)| *x == Some(u)).map(|(k, _)| k).collect();
        Some((*marcadas.first()?, *marcadas.last()?))
    };
    let mut unidades = Vec::new();
    for u in 0..total {
        let mut linha = None;
        let mut ultima_nova = 0;
        for r in &unificado {
            if let Some(n) = r.nova {
                ultima_nova = n;
            }
            if r.unidade == Some(u) && linha.is_none() {
                linha = Some(r.nova.unwrap_or(ultima_nova + 1));
            }
        }
        let tem = |t: Tipo| unificado.iter().any(|r| r.unidade == Some(u) && r.tipo == t);
        let tipo = match (tem(Tipo::Incluida), tem(Tipo::Removida)) {
            (true, true) => TipoUnidade::Troca,
            (true, false) => TipoUnidade::Inclusao,
            _ => TipoUnidade::Remocao,
        };
        unidades.push(Unidade {
            linha: linha.unwrap_or(1),
            lado: faixa(&mut lado.iter().map(|r| r.unidade), u),
            unificado: faixa(&mut unificado.iter().map(|r| r.unidade), u),
            tipo,
        });
    }
    Comparacao { lado, unificado, unidades }
}

/// Um bloco `@@` do patch: o cabeçalho e o texto (cabeçalho e corpo), que a tela mostra ao confirmar um descarte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlocoPatch {
    pub cabecalho: String,
    pub texto: String,
}

/// Separa o patch nos blocos `@@`, na ordem do backend.
pub fn blocos_patch(patch: &str) -> Vec<BlocoPatch> {
    let mut blocos = Vec::new();
    let mut atual: Option<Vec<&str>> = None;
    for linha in patch.split('\n') {
        if linha.starts_with("@@") {
            if let Some(a) = atual.take() {
                blocos.push(BlocoPatch { cabecalho: a[0].to_string(), texto: a.join("\n") });
            }
            atual = Some(vec![linha]);
        } else if let Some(a) = atual.as_mut() {
            a.push(linha);
        }
    }
    if let Some(a) = atual {
        blocos.push(BlocoPatch { cabecalho: a[0].to_string(), texto: a.join("\n") });
    }
    blocos
}

/// Cabeçalho do arquivo no patch (tudo antes do primeiro `@@`).
pub fn cabecalho_arquivo(patch: &str) -> &str {
    match patch.find("\n@@") {
        Some(i) => &patch[..i],
        None => patch,
    }
}

/// Arquivo criado: o cabeçalho traz `new file mode`; o comparador mostra uma coluna só.
pub fn arquivo_novo(patch: &str) -> bool {
    cabecalho_arquivo(patch).lines().any(|l| l.starts_with("new file mode "))
}

pub fn normalizar_quebras(texto: &str) -> String {
    if texto.contains('\r') {
        texto.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        texto.to_string()
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    const PATCH: &str = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,6 +1,7 @@\n a\n-b\n+B\n+B2\n c\n d\n-e\n f\n";

    #[test]
    fn trechos_sem_blocos() {
        let c = comparar(PATCH, None);
        assert_eq!(c.unidades.len(), 2);
        assert_eq!(c.unidades[0].tipo, TipoUnidade::Troca);
        assert_eq!(c.unidades[0].linha, 2);
        assert_eq!(c.unidades[1].tipo, TipoUnidade::Remocao);
        // A remoção da linha 5 antiga: a linha nova de referência é a seguinte à última nova vista.
        assert_eq!(c.unidades[1].linha, 6);
        // Lado a lado: b/B na mesma linha, B2 sozinho à direita.
        assert_eq!(c.lado[1], LinhaLado { esquerda: Some(2), direita: Some(2), unidade: Some(0) });
        assert_eq!(c.lado[2], LinhaLado { esquerda: None, direita: Some(3), unidade: Some(0) });
        assert_eq!(c.unidades[0].lado, Some((1, 2)));
        assert_eq!(c.unidades[0].unificado, Some((1, 3)));
        assert_eq!(chaves(&c.lado[1]), ["d2", "a2"]);
    }

    #[test]
    fn unidades_pelos_blocos() {
        let blocos = intervalos("@@ -1,3 +1,4 @@\n@@ -4,3 +5,2 @@\n");
        assert_eq!(blocos.len(), 2);
        let c = comparar(PATCH, Some(&blocos));
        assert_eq!(c.unidades.len(), 2);
        assert_eq!(c.unificado.iter().filter(|l| l.unidade == Some(1)).count(), 1);
    }

    #[test]
    fn cabecalho_sem_contagem() {
        assert_eq!(cabecalho("@@ -1 +1 @@"), Some(Intervalo { antigo: 1, antigo_n: 1, novo: 1, novo_n: 1 }));
        assert_eq!(cabecalho("@@ -0,0 +1,3 @@ fn"), Some(Intervalo { antigo: 0, antigo_n: 0, novo: 1, novo_n: 3 }));
    }

    #[test]
    fn blocos_e_arquivo_novo() {
        let b = blocos_patch(PATCH);
        assert_eq!(b.len(), 1);
        assert!(b[0].texto.starts_with("@@ -1,6 +1,7 @@\n a"));
        assert!(!arquivo_novo(PATCH));
        assert!(arquivo_novo("diff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1 @@\n+x\n"));
        assert_eq!(cabecalho_arquivo(PATCH), "diff --git a/x b/x\n--- a/x\n+++ b/x");
    }
}
