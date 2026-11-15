//! Porte de `tests/check-media-ui.mjs`: a View numa janela de teste, com arquivos reais em pasta temporária. O que lá era
//! da árvore e das abas (duplo clique, renomear pelo menu, recarregar o app) aqui é a API que a bancada usa:
//! `VisaoMidia::new`, `trocar_caminho`, `definir_visivel` e soltar a entidade.
mod comum;

use comum::{copiar, gerar, pcm, Tmp};
use gpui_kit::{px, Entity, TestAppContext, VisualTestContext};
use orquestrador_midia::{player, TipoMidia, VisaoMidia};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn abrir(cx: &mut TestAppContext, caminho: PathBuf) -> (Entity<VisaoMidia>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        orquestrador_ui::tema::aplicar(cx);
        orquestrador_midia::registrar_atalhos(cx);
    });
    cx.add_window_view(move |window, cx| VisaoMidia::new(caminho, window, cx))
}

/// Espera em tempo real (o decodificador é uma thread de verdade) avançando o relógio simulado do GPUI.
fn esperar(cx: &mut VisualTestContext, v: &Entity<VisaoMidia>, cond: impl Fn(&VisaoMidia) -> bool) {
    let inicio = Instant::now();
    loop {
        cx.executor().advance_clock(Duration::from_millis(10));
        cx.run_until_parked();
        if v.read_with(cx, |v, _| cond(v)) {
            cx.run_until_parked();
            return;
        }
        if inicio.elapsed() > Duration::from_secs(10) {
            let (aviso, status) = v.read_with(cx, |v, _| (v.aviso(), v.status()));
            panic!("tempo esgotado: aviso {aviso:?}, status {status:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn posicao(cx: &mut VisualTestContext, v: &Entity<VisaoMidia>) -> f64 {
    v.read_with(cx, |v, _| v.estado_reproducao().expect("player").posicao)
}

#[gpui_kit::test]
fn imagem_abre_decodifica_e_alterna_ajustar_e_tamanho_real(cx: &mut TestAppContext) {
    let _s = comum::serial();
    let tmp = Tmp::new("imagem");
    let c = copiar(&tmp, "logo.png", "logo.png");
    let (v, cx) = abrir(cx, c);
    esperar(cx, &v, |v| v.tamanho().is_some());
    v.read_with(cx, |v, _| {
        assert_eq!(v.tipo(), Some(TipoMidia::Imagem));
        assert_eq!(v.tamanho(), Some((32, 32)), "PNG decodificado com o tamanho do arquivo");
        assert_eq!(v.status().as_deref(), Some("32 × 32 px"));
        assert!(!v.tamanho_real(), "nasce em Ajustar");
    });
    cx.run_until_parked();
    let area = cx.debug_bounds("midia-area").expect("área da imagem");
    let imagem = cx.debug_bounds("midia-imagem").expect("imagem");
    assert!(area.contains(&imagem.origin) && imagem.size.width <= area.size.width, "imagem maior que o painel");

    let botao = cx.debug_bounds("alternar-tamanho").expect("botão").center();
    cx.simulate_click(botao, Default::default());
    cx.run_until_parked();
    assert!(v.read_with(cx, |v, _| v.tamanho_real()));
    let real = cx.debug_bounds("midia-imagem").expect("imagem");
    assert_eq!((real.size.width, real.size.height), (px(32.), px(32.)), "tamanho real: 32 px na tela");
    let botao = cx.debug_bounds("alternar-tamanho").expect("botão").center();
    cx.simulate_click(botao, Default::default());
    cx.run_until_parked();
    assert!(!v.read_with(cx, |v, _| v.tamanho_real()));
}

#[gpui_kit::test]
fn audio_nao_toca_sozinho_e_toca_e_pausa_pelo_gesto(cx: &mut TestAppContext) {
    let _s = comum::serial();
    let tmp = Tmp::new("audio");
    let c = tmp.caminho("tom.wav");
    gerar(&c, None, Some(pcm()), None, 2.).unwrap();
    let (v, cx) = abrir(cx, c);
    esperar(cx, &v, |v| v.estado_reproducao().is_some_and(|e| e.info.pronto));
    let e = v.read_with(cx, |v, _| v.estado_reproducao().unwrap());
    assert!(!e.tocando, "áudio não toca sozinho");
    assert!((e.info.duracao.unwrap() - 2.).abs() < 0.05, "duração do WAV: {:?}", e.info.duracao);
    assert_eq!(v.read_with(cx, |v, _| v.status()), None, "áudio sem faixa de status");
    std::thread::sleep(Duration::from_millis(300));
    cx.run_until_parked();
    assert!(!v.read_with(cx, |v, _| v.tocando()), "continua parado depois de um instante");

    // Gesto no botão de tocar.
    let tocar = cx.debug_bounds("tocar").expect("botão de tocar").center();
    cx.simulate_click(tocar, Default::default());
    esperar(cx, &v, |v| v.tocando() && v.estado_reproducao().unwrap().posicao > 0.05);
    cx.simulate_click(tocar, Default::default());
    esperar(cx, &v, |v| !v.tocando());
    let em = posicao(cx, &v);
    std::thread::sleep(Duration::from_millis(250));
    cx.run_until_parked();
    assert_eq!(posicao(cx, &v), em, "pausado não avança");
}

#[gpui_kit::test]
fn audio_busca_volume_e_pausa_ao_sair_da_vista_e_ao_fechar(cx: &mut TestAppContext) {
    let _s = comum::serial();
    let tmp = Tmp::new("audio2");
    let c = tmp.caminho("tom.wav");
    gerar(&c, None, Some(pcm()), None, 2.).unwrap();
    let antes = player::decodificadores_ativos();
    let (v, cx) = abrir(cx, c);
    esperar(cx, &v, |v| v.estado_reproducao().is_some_and(|e| e.info.pronto));

    v.update(cx, |v, cx| v.buscar(1., cx));
    assert!((posicao(cx, &v) - 1.).abs() < 0.1);
    v.update(cx, |v, cx| v.definir_volume(0.25, cx));
    assert!((v.read_with(cx, |v, _| v.estado_reproducao().unwrap().volume) - 0.25).abs() < 0.001, "volume 0,25");

    v.update(cx, |v, cx| v.tocar(cx));
    esperar(cx, &v, |v| v.tocando());
    // Troca de aba: a bancada avisa que saiu da vista.
    v.update(cx, |v, cx| v.definir_visivel(false, cx));
    assert!(!v.read_with(cx, |v, _| v.tocando()));
    v.update(cx, |v, cx| v.definir_visivel(true, cx));
    std::thread::sleep(Duration::from_millis(200));
    cx.run_until_parked();
    assert!(!v.read_with(cx, |v, _| v.tocando()), "voltar à aba não retoma sozinho");

    // Fechar a aba tocando: a thread do decodificador e a saída de áudio vão embora.
    v.update(cx, |v, cx| v.tocar(cx));
    esperar(cx, &v, |v| v.tocando());
    assert_eq!(player::decodificadores_ativos(), antes + 1);
    let fraca = v.downgrade();
    cx.update(|window, _| window.remove_window());
    drop(v);
    cx.run_until_parked();
    cx.cx.update(|_| {});

    assert!(fraca.upgrade().is_none(), "a entidade sobreviveu ao fechamento da janela");
    let inicio = Instant::now();
    while player::decodificadores_ativos() != antes {
        assert!(inicio.elapsed() < Duration::from_secs(5), "decodificador continuou vivo depois de fechar");
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui_kit::test]
fn falha_de_leitura_da_imagem_aparece_como_erro_na_aba(cx: &mut TestAppContext) {
    let _s = comum::serial();
    let tmp = Tmp::new("falha");
    let (v, cx) = abrir(cx, tmp.caminho("quebrado.png"));
    esperar(cx, &v, |v| v.aviso().is_some_and(|a| a != "Lendo o arquivo…"));
    let aviso = v.read_with(cx, |v, _| v.aviso()).unwrap();
    assert!(aviso.starts_with("Não foi possível ler o arquivo:"), "{aviso}");
    assert!(cx.debug_bounds("midia-imagem").is_none(), "sem imagem quando a leitura falha");
    assert_eq!(v.read_with(cx, |v, _| v.status()), None, "erro de leitura sem faixa");
}

#[gpui_kit::test]
fn audio_com_falha_de_leitura_mostra_aviso(cx: &mut TestAppContext) {
    let _s = comum::serial();
    let tmp = Tmp::new("falha-audio");
    let (v, cx) = abrir(cx, tmp.caminho("quebrado.wav"));
    esperar(cx, &v, |v| v.aviso().is_some());
    assert!(v.read_with(cx, |v, _| v.aviso()).unwrap().starts_with("Não foi possível tocar este áudio"));
    assert!(cx.debug_bounds("tocar").is_some(), "o controle fica, com o erro abaixo");
}

#[gpui_kit::test]
fn arquivo_invalido_imagem_e_audio_mostram_erro(cx: &mut TestAppContext) {
    let _s = comum::serial();
    let tmp = Tmp::new("invalido");
    let png = tmp.caminho("ruim.png");
    let wav = tmp.caminho("ruim.wav");
    std::fs::write(&png, [0u8, 1, 2, 3]).unwrap();
    std::fs::write(&wav, [0u8, 1, 2, 3]).unwrap();
    let (v, cx) = abrir(cx, png);
    esperar(cx, &v, |v| v.aviso().is_some_and(|a| a != "Lendo o arquivo…"));
    assert_eq!(v.read_with(cx, |v, _| v.aviso()).as_deref(), Some("Não foi possível exibir esta imagem."));
    assert!(cx.debug_bounds("midia-imagem").is_none(), "sem imagem quando não decodifica");
    assert_eq!(v.read_with(cx, |v, _| v.status()).as_deref(), Some("Imagem"), "a faixa fica, sem tamanho");

    v.update(cx, |v, cx| v.trocar_caminho(wav, cx));
    esperar(cx, &v, |v| v.aviso().is_some());
    assert!(v.read_with(cx, |v, _| v.aviso()).unwrap().starts_with("Não foi possível tocar este áudio"));
}

#[gpui_kit::test]
fn imagem_e_audio_renomeados_recarregam(cx: &mut TestAppContext) {
    let _s = comum::serial();
    let tmp = Tmp::new("renomear");
    let foto = copiar(&tmp, "logo.png", "foto.png");
    let (v, cx) = abrir(cx, foto.clone());
    esperar(cx, &v, |v| v.tamanho().is_some());
    let nova = tmp.caminho("foto-nova.png");
    std::fs::rename(&foto, &nova).unwrap();
    v.update(cx, |v, cx| v.trocar_caminho(nova.clone(), cx));
    esperar(cx, &v, |v| v.tamanho() == Some((32, 32)));
    assert_eq!(v.read_with(cx, |v, _| v.caminho().to_path_buf()), nova);

    let tom = tmp.caminho("tom.wav");
    gerar(&tom, None, Some(pcm()), None, 2.).unwrap();
    v.update(cx, |v, cx| v.trocar_caminho(tom.clone(), cx));
    esperar(cx, &v, |v| v.estado_reproducao().is_some_and(|e| e.info.pronto));
    let novo = tmp.caminho("tom-novo.wav");
    std::fs::rename(&tom, &novo).unwrap();
    v.update(cx, |v, cx| v.trocar_caminho(novo.clone(), cx));
    esperar(cx, &v, |v| v.estado_reproducao().is_some_and(|e| e.info.pronto));
    let d = v.read_with(cx, |v, _| v.estado_reproducao().unwrap().info.duracao.unwrap());
    assert!((d - 2.).abs() < 0.05, "áudio renomeado recarregado (duração {d})");
}

#[gpui_kit::test]
fn video_decodifica_cabe_no_painel_nao_toca_sozinho_e_toca_e_pausa_pelo_gesto(cx: &mut TestAppContext) {
    let _s = comum::serial();
    let tmp = Tmp::new("video");
    let c = copiar(&tmp, "clipe.webm", "clipe.webm");
    let (v, cx) = abrir(cx, c);
    esperar(cx, &v, |v| v.tamanho().is_some());
    esperar(cx, &v, |_| true);
    let e = v.read_with(cx, |v, _| v.estado_reproducao().unwrap());
    assert!(!e.tocando, "vídeo não toca sozinho");
    assert_eq!(e.info.video, Some((64, 48)), "WebM decodificado com o tamanho do arquivo");
    assert!((e.info.duracao.unwrap() - 2.).abs() < 0.1, "duração do WebM: {:?}", e.info.duracao);
    assert_eq!(v.read_with(cx, |v, _| v.status()).as_deref(), Some("64 × 48 px"));

    // Primeiro quadro na tela, contido e ocupando o painel.
    let inicio = Instant::now();
    while cx.debug_bounds("midia-video").is_none() {
        assert!(inicio.elapsed() < Duration::from_secs(5), "sem primeiro quadro");
        cx.executor().advance_clock(Duration::from_millis(10));
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
    let area = cx.debug_bounds("midia-video-area").unwrap();
    let quadro = cx.debug_bounds("midia-video").unwrap();
    assert!(area.contains(&quadro.origin) && quadro.size.height > px(48.), "vídeo fora do painel ou sem ocupá-lo");

    std::thread::sleep(Duration::from_millis(300));
    cx.run_until_parked();
    assert!(!v.read_with(cx, |v, _| v.tocando()), "continua parado depois de um instante");

    // Clicar no quadro alterna tocar e pausar.
    let centro = area.center();
    cx.simulate_click(centro, Default::default());
    esperar(cx, &v, |v| v.tocando() && v.estado_reproducao().unwrap().posicao > 0.05);
    cx.simulate_click(centro, Default::default());
    esperar(cx, &v, |v| !v.tocando());
    let em = posicao(cx, &v);
    std::thread::sleep(Duration::from_millis(250));
    cx.run_until_parked();
    assert_eq!(posicao(cx, &v), em, "pausado não avança");
}

#[gpui_kit::test]
fn video_pausa_ao_sair_da_vista_e_ao_fechar(cx: &mut TestAppContext) {
    let _s = comum::serial();
    let tmp = Tmp::new("video2");
    let c = copiar(&tmp, "clipe.webm", "clipe.webm");
    let antes = player::decodificadores_ativos();
    let (v, cx) = abrir(cx, c);
    esperar(cx, &v, |v| v.tamanho().is_some());
    v.update(cx, |v, cx| v.tocar(cx));
    esperar(cx, &v, |v| v.tocando());
    v.update(cx, |v, cx| v.definir_visivel(false, cx));
    assert!(!v.read_with(cx, |v, _| v.tocando()));
    v.update(cx, |v, cx| v.definir_visivel(true, cx));
    std::thread::sleep(Duration::from_millis(200));
    cx.run_until_parked();
    assert!(!v.read_with(cx, |v, _| v.tocando()), "voltar à aba não retoma sozinho");

    v.update(cx, |v, cx| v.tocar(cx));
    esperar(cx, &v, |v| v.tocando());
    let fraca = v.downgrade();
    cx.update(|window, _| window.remove_window());
    drop(v);
    cx.run_until_parked();
    cx.cx.update(|_| {});

    assert!(fraca.upgrade().is_none(), "a entidade sobreviveu ao fechamento da janela");
    let inicio = Instant::now();
    while player::decodificadores_ativos() != antes {
        assert!(inicio.elapsed() < Duration::from_secs(5), "decodificador continuou vivo depois de fechar");
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui_kit::test]
fn video_com_falha_de_leitura_ou_bytes_invalidos_mostra_aviso(cx: &mut TestAppContext) {
    let _s = comum::serial();
    let tmp = Tmp::new("video-falha");
    let ruim = tmp.caminho("ruim.webm");
    std::fs::write(&ruim, [0u8, 1, 2, 3]).unwrap();
    let (v, cx) = abrir(cx, tmp.caminho("sumiu.mp4"));
    esperar(cx, &v, |v| v.aviso().is_some());
    assert!(v.read_with(cx, |v, _| v.aviso()).unwrap().starts_with("Não foi possível tocar este vídeo"));
    v.update(cx, |v, cx| v.trocar_caminho(ruim, cx));
    esperar(cx, &v, |v| v.aviso().is_some());
    assert!(v.read_with(cx, |v, _| v.aviso()).unwrap().starts_with("Não foi possível tocar este vídeo"));
}

#[gpui_kit::test]
fn atalhos_do_controle(cx: &mut TestAppContext) {
    let _s = comum::serial();
    let tmp = Tmp::new("atalhos");
    let c = tmp.caminho("tom.wav");
    gerar(&c, None, Some(pcm()), None, 2.).unwrap();
    let (v, cx) = abrir(cx, c);
    esperar(cx, &v, |v| v.estado_reproducao().is_some_and(|e| e.info.pronto));
    cx.update(|window, cx| {
        let foco = v.read(cx).foco();
        window.focus(&foco, cx)
    });
    cx.simulate_keystrokes("right");
    assert!((posicao(cx, &v) - 2.).abs() < 0.01, "→ avança 5 s (até o fim)");
    cx.simulate_keystrokes("left");
    assert!(posicao(cx, &v) < 0.01, "← volta 5 s");
    cx.simulate_keystrokes("m");
    assert!(v.read_with(cx, |v, _| v.estado_reproducao().unwrap().mudo));
    cx.simulate_keystrokes("down");
    assert!((v.read_with(cx, |v, _| v.estado_reproducao().unwrap().volume) - 0.95).abs() < 0.001);
    cx.simulate_keystrokes("space");
    esperar(cx, &v, |v| v.tocando());
    cx.simulate_keystrokes("k");
    assert!(!v.read_with(cx, |v, _| v.tocando()));
}
