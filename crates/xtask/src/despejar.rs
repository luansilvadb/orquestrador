//! Gerador de saída intensa e determinístico (o `flood.mjs` da base): linhas de 120 colunas com cor SGR.
//! Uso: `xtask despejar <linhas> <tag> [linhas por segundo; 0 = o máximo] [--esperar <arquivo>]`. Com `ORQ_DONE_DIR`,
//! grava `inicio-<tag>` e `fim-<tag>` (ms desde a época) nessa pasta. `--esperar` só começa quando o arquivo existir.
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const ALFABETO: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789 _-./:=+";
const CORES: [u32; 12] = [31, 32, 33, 34, 35, 36, 91, 92, 93, 94, 95, 96];

pub struct Gerador {
    tag: String,
    seed: u32,
}

impl Gerador {
    pub fn new(tag: &str) -> Self {
        let c = tag.encode_utf16().next().unwrap_or(0) as u32;
        Gerador { tag: tag.to_string(), seed: 12345 + c }
    }

    fn rnd(&mut self) -> f64 {
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        self.seed as f64 / 4294967296.
    }

    pub fn linha(&mut self, i: usize, out: &mut Vec<u8>) {
        let mut body = format!("[{}] {i:07} ", self.tag).into_bytes();
        while body.len() < 118 {
            body.push(ALFABETO[(self.rnd() * ALFABETO.len() as f64) as usize]);
        }
        body.truncate(118);
        write!(out, "\x1b[{}m", CORES[i % CORES.len()]).unwrap();
        out.extend_from_slice(&body);
        write!(out, "\x1b[0m\x1b[1;3{}mOK\x1b[0m\r\n", (i % 7) + 1).unwrap();
    }
}

fn agora_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut pos = Vec::new();
    let mut esperar = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--esperar" {
            esperar = it.next().cloned();
        } else {
            pos.push(a.clone());
        }
    }
    let uso = || "uso: despejar <linhas> <tag> [linhas/s] [--esperar <arquivo>]".to_string();
    let linhas: usize = pos.first().and_then(|v| v.parse().ok()).ok_or_else(uso)?;
    let tag = pos.get(1).ok_or_else(uso)?.clone();
    let taxa: f64 = pos.get(2).map_or(Ok(0.), |v| v.parse().map_err(|_| uso()))?;
    let marca = |nome: &str| {
        if let Some(d) = std::env::var_os("ORQ_DONE_DIR") {
            let _ = std::fs::write(std::path::Path::new(&d).join(format!("{nome}-{tag}")), agora_ms().to_string());
        }
    };
    if let Some(e) = esperar {
        marca("pronto");
        while !std::path::Path::new(&e).exists() {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    marca("inicio");
    let mut g = Gerador::new(&tag);
    let mut saida = std::io::stdout().lock();
    let t0 = Instant::now();
    let mut buf = Vec::with_capacity(100 * 140);
    for i in (0..linhas).step_by(100) {
        if taxa > 0. {
            let alvo = t0 + Duration::from_secs_f64(i as f64 / taxa);
            let falta = alvo.saturating_duration_since(Instant::now());
            if !falta.is_zero() {
                std::thread::sleep(falta);
            }
        }
        buf.clear();
        for j in i..linhas.min(i + 100) {
            g.linha(j, &mut buf);
        }
        saida.write_all(&buf).and_then(|_| saida.flush()).map_err(|e| e.to_string())?;
    }
    marca("fim");
    Ok(())
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn linha_igual_a_do_flood_mjs() {
        let mut g = Gerador::new("1");
        let mut b = Vec::new();
        g.linha(0, &mut b);
        let s = String::from_utf8(b).unwrap();
        assert!(s.starts_with("\x1b[31m[1] 0000000 "));
        assert!(s.ends_with("\x1b[0m\x1b[1;31mOK\x1b[0m\r\n"));
        // 118 do corpo + "OK": 120 colunas visíveis.
        let visivel = regex::Regex::new("\x1b\\[[0-9;]*m").unwrap().replace_all(&s, "");
        assert_eq!(visivel.trim_end().len(), 120);
        // Mesma sequência do `rnd` do JavaScript para a semente 12345 + '1'.
        let mut h = Gerador::new("1");
        let r = h.rnd();
        assert!((r - ((12394u64 * 1664525 + 1013904223) % 4294967296) as f64 / 4294967296.).abs() < 1e-12);
    }
}
