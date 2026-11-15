//! As fileiras que o comparador desenha, como o `@pierre/diffs` as monta para `diffStyle: 'split'` e `'unified'`: no
//! lado a lado, cada trecho de mudança pareia a k-ésima remoção com a k-ésima inclusão e o lado que sobra fica vazio
//! (hachurado); o aviso "No newline at end of file" vira uma fileira própria; as linhas inalteradas fora dos blocos do
//! patch ficam recolhidas, e o separador delas não aparece (o `unsafeCSS` do app esconde `[data-separator]`).
use super::palavras;
use crate::comparar::cabecalho;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    Contexto,
    Removida,
    Incluida,
}

/// Uma linha do patch, com o texto (sem o sinal) e se o arquivo acaba nela sem quebra de linha.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Linha {
    pub tipo: Tipo,
    pub antiga: Option<usize>,
    pub nova: Option<usize>,
    pub texto: String,
    pub sem_quebra: bool,
    pub bloco: usize,
}

/// O conteúdo de uma célula de texto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Celula {
    /// A linha `i` do patch; no contexto, `antiga` diz de que lado o texto é lido.
    Linha(usize),
    /// O aviso de fim sem quebra de linha, com o tipo da linha a que se refere.
    SemQuebra(Tipo),
    /// Espaço de alinhamento (hachurado no lado a lado).
    Vazia,
    /// Coluna que não existe (o lado novo de um arquivo removido).
    Nada,
}

/// Uma fileira: no unificado só `a` vale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fileira {
    pub a: Celula,
    pub b: Celula,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Modelo {
    pub linhas: Vec<Linha>,
    pub lado: Vec<Fileira>,
    pub unificado: Vec<Fileira>,
    /// Fileira de cada linha do patch em cada modo.
    pub fileira_lado: Vec<usize>,
    pub fileira_unificado: Vec<usize>,
    /// Realce por palavra de cada linha (faixas em bytes do texto).
    pub enfase: Vec<Vec<Range<usize>>>,
    pub arquivo_novo: bool,
    pub arquivo_removido: bool,
}

/// Lê as linhas do patch (as do cabeçalho do arquivo ficam de fora).
pub fn linhas(patch: &str) -> Vec<Linha> {
    let mut v: Vec<Linha> = Vec::new();
    let (mut o, mut n) = (0, 0);
    let mut bloco = None::<usize>;
    for bruta in patch.split('\n') {
        if let Some(h) = cabecalho(bruta) {
            o = h.antigo;
            n = h.novo;
            bloco = Some(bloco.map_or(0, |b| b + 1));
            continue;
        }
        let Some(b) = bloco else { continue };
        let Some(sinal) = bruta.chars().next() else { continue };
        let texto = bruta[1..].strip_suffix('\r').unwrap_or(&bruta[1..]).to_string();
        let linha = |tipo, antiga, nova| Linha { tipo, antiga, nova, texto, sem_quebra: false, bloco: b };
        match sinal {
            ' ' => {
                v.push(linha(Tipo::Contexto, Some(o), Some(n)));
                o += 1;
                n += 1;
            }
            '-' => {
                v.push(linha(Tipo::Removida, Some(o), None));
                o += 1;
            }
            '+' => {
                v.push(linha(Tipo::Incluida, None, Some(n)));
                n += 1;
            }
            '\\' => {
                if let Some(u) = v.last_mut() {
                    u.sem_quebra = true;
                }
            }
            _ => {}
        }
    }
    v
}

pub fn montar(patch: &str) -> Modelo {
    let cab = crate::comparar::cabecalho_arquivo(patch);
    let arquivo_novo = cab.lines().any(|l| l.starts_with("new file mode "));
    let arquivo_removido = cab.lines().any(|l| l.starts_with("deleted file mode "));
    let linhas = linhas(patch);
    let n = linhas.len();
    let mut m = Modelo {
        fileira_lado: vec![0; n],
        fileira_unificado: vec![0; n],
        enfase: vec![Vec::new(); n],
        arquivo_novo,
        arquivo_removido,
        ..Default::default()
    };
    let lado_b = |c: Celula| if arquivo_removido { Celula::Nada } else { c };
    // Unificado: uma fileira por linha; o aviso de fim sem quebra vem logo depois (dois no contexto, um por lado).
    for (i, l) in linhas.iter().enumerate() {
        m.fileira_unificado[i] = m.unificado.len();
        m.unificado.push(Fileira { a: Celula::Linha(i), b: Celula::Nada });
        if l.sem_quebra {
            let k = if l.tipo == Tipo::Contexto { 2 } else { 1 };
            for _ in 0..k {
                m.unificado.push(Fileira { a: Celula::SemQuebra(l.tipo), b: Celula::Nada });
            }
        }
    }
    // Lado a lado, bloco por bloco: o aviso de fim sem quebra entra depois da última fileira do bloco.
    let mut i = 0;
    while i < n {
        let bloco = linhas[i].bloco;
        let mut fim = i;
        while fim < n && linhas[fim].bloco == bloco {
            fim += 1;
        }
        let (mut sem_d, mut sem_a) = (None, None);
        let mut j = i;
        while j < fim {
            let l = &linhas[j];
            if l.tipo == Tipo::Contexto {
                m.fileira_lado[j] = m.lado.len();
                m.lado.push(Fileira { a: Celula::Linha(j), b: lado_b(Celula::Linha(j)) });
                if l.sem_quebra {
                    sem_d = Some(Tipo::Contexto);
                    sem_a = Some(Tipo::Contexto);
                }
                j += 1;
                continue;
            }
            let mut k = j;
            while k < fim && linhas[k].tipo != Tipo::Contexto {
                k += 1;
            }
            let dels: Vec<usize> = (j..k).filter(|&x| linhas[x].tipo == Tipo::Removida).collect();
            let adds: Vec<usize> = (j..k).filter(|&x| linhas[x].tipo == Tipo::Incluida).collect();
            for p in 0..dels.len().max(adds.len()) {
                let (d, a) = (dels.get(p).copied(), adds.get(p).copied());
                for x in d.iter().chain(a.iter()) {
                    m.fileira_lado[*x] = m.lado.len();
                    if linhas[*x].sem_quebra {
                        if linhas[*x].tipo == Tipo::Removida {
                            sem_d = Some(Tipo::Removida);
                        } else {
                            sem_a = Some(Tipo::Incluida);
                        }
                    }
                }
                m.lado.push(Fileira {
                    a: d.map_or(Celula::Vazia, Celula::Linha),
                    b: lado_b(a.map_or(Celula::Vazia, Celula::Linha)),
                });
                // O par removida × incluída ganha o realce por palavra nos dois modos.
                if let (Some(d), Some(a)) = (d, a) {
                    let (rd, ra) = palavras::realces(&linhas[d].texto, &linhas[a].texto);
                    m.enfase[d] = rd;
                    m.enfase[a] = ra;
                }
            }
            j = k;
        }
        if sem_d.is_some() || sem_a.is_some() {
            m.lado.push(Fileira {
                a: sem_d.map_or(Celula::Vazia, Celula::SemQuebra),
                b: lado_b(sem_a.map_or(Celula::Vazia, Celula::SemQuebra)),
            });
        }
        i = fim;
    }
    m.linhas = linhas;
    m
}

impl Modelo {
    pub fn fileiras(&self, lado: bool) -> &[Fileira] {
        if lado { &self.lado } else { &self.unificado }
    }

    /// Fileira de cada linha do patch no modo.
    pub fn fileira(&self, lado: bool, linha: usize) -> usize {
        if lado { self.fileira_lado[linha] } else { self.fileira_unificado[linha] }
    }

    /// Número mostrado na calha de uma célula: a linha antiga na coluna original, a nova na modificada; no unificado,
    /// a antiga só nas remoções.
    pub fn numero(&self, c: Celula, coluna_b: bool, lado: bool) -> Option<usize> {
        let Celula::Linha(i) = c else { return None };
        let l = &self.linhas[i];
        match (lado, coluna_b, l.tipo) {
            (true, false, _) => l.antiga,
            (true, true, _) => l.nova,
            (false, _, Tipo::Removida) => l.antiga,
            (false, _, _) => l.nova,
        }
    }

    /// Tipo de uma célula (o `data-line-type`): `None` para vazio.
    pub fn tipo(&self, c: Celula) -> Option<Tipo> {
        match c {
            Celula::Linha(i) => Some(self.linhas[i].tipo),
            Celula::SemQuebra(t) => Some(t),
            _ => None,
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    const PATCH: &str = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,6 +1,7 @@\n a\n-b\n+B\n+B2\n c\n d\n-e\n f\n";

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn pareia_remocoes_e_inclusoes() {
        let m = montar(PATCH);
        assert_eq!(m.linhas.len(), 8);
        assert_eq!(m.lado.len(), 7);
        assert_eq!(m.lado[1], Fileira { a: Celula::Linha(1), b: Celula::Linha(2) });
        assert_eq!(m.lado[2], Fileira { a: Celula::Vazia, b: Celula::Linha(3) });
        assert_eq!(m.lado[5], Fileira { a: Celula::Linha(6), b: Celula::Vazia });
        assert_eq!(m.unificado.len(), 8);
        assert_eq!(m.fileira_lado[3], 2);
        // b → B: o par ganha realce por palavra.
        assert_eq!(m.enfase[1], [0..1]);
        assert_eq!(m.enfase[2], [0..1]);
        assert_eq!(m.numero(m.unificado[1].a, false, false), Some(2));
        assert_eq!(m.numero(m.lado[1].b, true, true), Some(2));
    }

    #[test]
    fn fim_sem_quebra_vira_fileira() {
        let p = "--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n";
        let m = montar(p);
        assert_eq!(m.unificado.len(), 5);
        assert_eq!(m.unificado[2].a, Celula::SemQuebra(Tipo::Removida));
        assert_eq!(m.unificado[4].a, Celula::SemQuebra(Tipo::Incluida));
        assert_eq!(m.lado.len(), 3);
        assert_eq!(m.lado[2], Fileira { a: Celula::SemQuebra(Tipo::Removida), b: Celula::SemQuebra(Tipo::Incluida) });
        // Só um lado sem quebra: o outro fica vazio.
        let m = montar("@@ -1 +1,2 @@\n a\n+b\n\\ No newline at end of file\n");
        assert_eq!(m.lado.last(), Some(&Fileira { a: Celula::Vazia, b: Celula::SemQuebra(Tipo::Incluida) }));
        // Contexto sem quebra: um aviso por lado no unificado.
        let m = montar("@@ -1,2 +1,2 @@\n-a\n+b\n c\n\\ No newline at end of file\n");
        assert_eq!(m.unificado.len(), 5);
    }

    #[test]
    fn blocos_recolhidos_sem_separador() {
        // Dois blocos com linhas inalteradas entre eles fora do patch: as fileiras ficam seguidas, sem separador.
        let p = "--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n a\n-b\n+B\n@@ -40,2 +40,2 @@\n y\n-z\n+Z\n";
        let m = montar(p);
        assert_eq!(m.lado.len(), 4);
        assert_eq!(m.unificado.len(), 6);
        assert_eq!(m.linhas[3].antiga, Some(40));
        assert_eq!(m.linhas[3].bloco, 1);
        assert_eq!(m.numero(m.lado[2].a, false, true), Some(40));
    }

    #[test]
    fn arquivo_novo_e_removido() {
        let m = montar("diff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,2 @@\n+x\n+y\n");
        assert!(m.arquivo_novo);
        assert_eq!(m.unificado.len(), 2);
        let m = montar("diff --git a/n b/n\ndeleted file mode 100644\n--- a/n\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n");
        assert!(m.arquivo_removido);
        assert_eq!(m.lado[0], Fileira { a: Celula::Linha(0), b: Celula::Nada });
    }
}
