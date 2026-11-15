//! Atualização periódica (porte de `src/shared/polling.ts`): uma rodada por vez, a cada `ms` com a janela em foco e a cada
//! 10 s sem foco. Ganhar foco adianta a rodada; perder estica a espera em curso. `cutucar` (evento de executor) pede uma
//! rodada imediata mesmo sem foco; rajadas viram no máximo uma rodada pendente.
use futures_lite::future;
use gpui_kit::{AsyncWindowContext, Context, Subscription, Task, WeakEntity, Window};
use std::time::{Duration, Instant};

pub const RITMO: Duration = Duration::from_millis(1500);
pub const SEM_FOCO: Duration = Duration::from_millis(10_000);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sinal {
    Foco,
    Desfoco,
    Cutucar,
}

/// Laço vivo enquanto este valor existir.
pub struct Sondagem {
    tx: async_channel::Sender<Sinal>,
    _laco: Task<()>,
    _foco: Subscription,
}

impl Sondagem {
    /// `rodada` devolve a tarefa de uma consulta; a próxima só sai depois que ela termina.
    pub fn iniciar<T: 'static>(
        ms: Duration,
        rodada: impl Fn(&mut T, &mut Window, &mut Context<T>) -> Task<()> + 'static,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> Sondagem {
        let (tx, rx) = async_channel::unbounded::<Sinal>();
        let tx_foco = tx.clone();
        let foco = cx.observe_window_activation(window, move |_, window, _| {
            let _ = tx_foco.try_send(if window.is_window_active() { Sinal::Foco } else { Sinal::Desfoco });
        });
        let laco = cx.spawn_in(window, async move |this: WeakEntity<T>, cx: &mut AsyncWindowContext| loop {
            while rx.try_recv().is_ok() {}
            let Ok(tarefa) = this.update_in(cx, |this, window, cx| rodada(this, window, cx)) else { break };
            tarefa.await;
            // Cutucada durante a rodada: a próxima sai logo depois.
            let mut pendente = false;
            while let Ok(s) = rx.try_recv() {
                pendente |= s == Sinal::Cutucar;
            }
            if pendente {
                continue;
            }
            let ativa = cx.update(|window, _| window.is_window_active()).unwrap_or(true);
            let inicio = Instant::now();
            let mut espera = if ativa { ms } else { SEM_FOCO };
            loop {
                let resto = espera.saturating_sub(inicio.elapsed());
                let timer = cx.background_executor().timer(resto);
                let sinal = future::or(async { timer.await; None }, async { rx.recv().await.ok() }).await;
                match sinal {
                    None => break,
                    Some(Sinal::Foco | Sinal::Cutucar) => break,
                    Some(Sinal::Desfoco) => espera = SEM_FOCO,
                }
            }
        });
        Sondagem { tx, _laco: laco, _foco: foco }
    }

    /// Rodada imediata (evento de executor), mesmo sem foco.
    pub fn cutucar(&self) {
        let _ = self.tx.try_send(Sinal::Cutucar);
    }
}
