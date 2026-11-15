//! Gancho de `cargo xtask medir`, ligado por `ORQ_MEDIR_DIR` (uma pasta). No primeiro quadro desenhado grava em
//! `abertura.txt` o instante (µs desde a época Unix). Com `roteiro.txt` na pasta, abre o painel Terminal com um shell por
//! linha e, com todos vivos, escreve cada linha no seu shell; o último fica ativo e focado para receber as teclas.
use crate::bancada::Bancada;
use gpui_kit::{Context, WeakEntity, Window};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn pasta() -> Option<PathBuf> {
    std::env::var_os("ORQ_MEDIR_DIR").filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Chamado em todo `render` da bancada; só age no primeiro.
pub fn quadro(window: &mut Window, cx: &mut Context<Bancada>) {
    static FEITO: AtomicBool = AtomicBool::new(false);
    let Some(dir) = pasta() else { return };
    if FEITO.swap(true, Ordering::AcqRel) {
        return;
    }
    let d = dir.clone();
    window.on_next_frame(move |_, _| {
        let us = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_micros()).unwrap_or(0);
        let _ = std::fs::write(d.join("abertura.txt"), us.to_string());
    });
    let Ok(texto) = std::fs::read_to_string(dir.join("roteiro.txt")) else { return };
    let comandos: Vec<String> = texto.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect();
    if comandos.is_empty() {
        return;
    }
    cx.spawn(async move |this: WeakEntity<Bancada>, cx| {
        let executor = cx.background_executor().clone();
        let espera = || executor.timer(Duration::from_millis(100));
        // Abre o painel e cria os shells um por vez: o shell só nasce quando a aba ativa é desenhada.
        for _ in 0..1200 {
            let feito = this.update(cx, |b, cx| {
                if b.ativa.is_none() {
                    return false;
                }
                if !b.terminal_aberto {
                    b.alternar_terminal(cx);
                }
                let (ativa, raiz) = (b.ativa.clone(), b.aba_ativa().map(|a| a.caminho.clone()));
                b.terminais.update(cx, |t, cx| {
                    let ultimo = t.projeto(&ativa).shells.last().map(|s| crate::terminal::motor::chave(ativa.as_deref(), &s.id));
                    let ultimo_vivo = ultimo.is_none_or(|k| t.vistas.get(&k).is_some_and(|v| v.read(cx).vivo));
                    if ultimo_vivo && t.projeto(&ativa).shells.len() < comandos.len() {
                        t.adicionar_shell(ativa.clone(), raiz, cx);
                    }
                    t.projeto(&ativa).shells.len() >= comandos.len()
                })
            });
            match feito {
                Ok(true) => break,
                Ok(false) => espera().await,
                Err(_) => return,
            }
        }
        // Escreve quando todos os shells estiverem vivos.
        for _ in 0..600 {
            let feito = this.update(cx, |b, cx| {
                let ativa = b.ativa.clone();
                b.terminais.update(cx, |t, cx| {
                    let ids: Vec<String> = t.projeto(&ativa).shells.iter().map(|s| s.id.clone()).collect();
                    let vistas: Vec<_> = ids
                        .iter()
                        .filter_map(|id| t.vistas.get(&crate::terminal::motor::chave(ativa.as_deref(), id)).cloned())
                        .collect();
                    if vistas.len() < comandos.len() || !vistas.iter().all(|v| v.read(cx).vivo) {
                        return false;
                    }
                    for (v, c) in vistas.iter().zip(&comandos) {
                        v.read(cx).escrever(&format!("{c}\r"));
                    }
                    true
                })
            });
            match feito {
                Ok(true) => return,
                Ok(false) => espera().await,
                Err(_) => return,
            }
        }
    })
    .detach();
}

