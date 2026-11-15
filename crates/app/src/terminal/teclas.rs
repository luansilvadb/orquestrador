//! Teclas para sequências do terminal, como o xterm.js manda. Texto comum segue pelo `InputHandler` (com IME); daqui só
//! saem as teclas especiais e as combinações com Ctrl e Alt.
use gpui_kit::Keystroke;

/// Parâmetro de modificador do xterm: 1 + Shift(1) + Alt(2) + Ctrl(4); `None` sem modificador.
fn parametro(k: &Keystroke) -> Option<u8> {
    let m = k.modifiers;
    let n = (m.shift as u8) + (m.alt as u8) * 2 + (m.control as u8) * 4;
    (n > 0).then_some(n + 1)
}

fn controle(c: char) -> Option<u8> {
    match c {
        'a'..='z' => Some(c as u8 - b'a' + 1),
        '@' | '2' | ' ' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '/' | '7' => Some(0x1f),
        '8' => Some(0x7f),
        _ => None,
    }
}

/// `None` deixa a tecla para a entrada de texto.
pub fn sequencia(k: &Keystroke, app_cursor: bool) -> Option<Vec<u8>> {
    let m = k.modifiers;
    let p = parametro(k);
    let csi = |final_: &str| -> Vec<u8> {
        match p {
            Some(p) => format!("\x1b[1;{p}{final_}").into_bytes(),
            None if app_cursor => format!("\x1bO{final_}").into_bytes(),
            None => format!("\x1b[{final_}").into_bytes(),
        }
    };
    let til = |n: u8| -> Vec<u8> {
        match p {
            Some(p) => format!("\x1b[{n};{p}~").into_bytes(),
            None => format!("\x1b[{n}~").into_bytes(),
        }
    };
    let ss3 = |final_: &str| -> Vec<u8> {
        match p {
            Some(p) => format!("\x1b[1;{p}{final_}").into_bytes(),
            None => format!("\x1bO{final_}").into_bytes(),
        }
    };
    let com_alt = |mut b: Vec<u8>| {
        if m.alt {
            b.insert(0, 0x1b);
        }
        b
    };
    let seq = match k.key.as_str() {
        "enter" => com_alt(vec![b'\r']),
        "backspace" if m.control => com_alt(vec![0x08]),
        "backspace" => com_alt(vec![0x7f]),
        "tab" if m.shift => b"\x1b[Z".to_vec(),
        "tab" => com_alt(vec![b'\t']),
        "escape" => com_alt(vec![0x1b]),
        "up" => csi("A"),
        "down" => csi("B"),
        "right" => csi("C"),
        "left" => csi("D"),
        "home" => csi("H"),
        "end" => csi("F"),
        "insert" => til(2),
        "delete" => til(3),
        "pageup" => til(5),
        "pagedown" => til(6),
        "f1" => ss3("P"),
        "f2" => ss3("Q"),
        "f3" => ss3("R"),
        "f4" => ss3("S"),
        "f5" => til(15),
        "f6" => til(17),
        "f7" => til(18),
        "f8" => til(19),
        "f9" => til(20),
        "f10" => til(21),
        "f11" => til(23),
        "f12" => til(24),
        tecla => {
            let mut chars = tecla.chars();
            let c = chars.next()?;
            if chars.next().is_some() && tecla != "space" {
                return None;
            }
            let c = if tecla == "space" { ' ' } else { c };
            // Ctrl+Alt é o AltGr no Windows: o caractere vem pela entrada de texto.
            if m.control && m.alt {
                return None;
            }
            if m.control {
                return controle(c.to_ascii_lowercase()).map(|b| vec![b]);
            }
            if m.alt {
                let texto = k.key_char.clone().unwrap_or_else(|| c.to_string());
                return Some(com_alt(texto.into_bytes()));
            }
            return None;
        }
    };
    Some(seq)
}

#[cfg(test)]
mod testes {
    use super::*;

    fn k(s: &'static str) -> Keystroke {
        Keystroke::parse(s).expect("tecla")
    }

    #[test]
    fn teclas_especiais_como_o_xterm() {
        assert_eq!(sequencia(&k("enter"), false), Some(b"\r".to_vec()));
        assert_eq!(sequencia(&k("backspace"), false), Some(vec![0x7f]));
        assert_eq!(sequencia(&k("ctrl-backspace"), false), Some(vec![0x08]));
        assert_eq!(sequencia(&k("up"), false), Some(b"\x1b[A".to_vec()));
        assert_eq!(sequencia(&k("up"), true), Some(b"\x1bOA".to_vec()));
        assert_eq!(sequencia(&k("ctrl-left"), true), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(sequencia(&k("shift-tab"), false), Some(b"\x1b[Z".to_vec()));
        assert_eq!(sequencia(&k("delete"), false), Some(b"\x1b[3~".to_vec()));
        assert_eq!(sequencia(&k("f1"), false), Some(b"\x1bOP".to_vec()));
        assert_eq!(sequencia(&k("f12"), false), Some(b"\x1b[24~".to_vec()));
        assert_eq!(sequencia(&k("shift-f5"), false), Some(b"\x1b[15;2~".to_vec()));
    }

    #[test]
    fn ctrl_e_alt() {
        assert_eq!(sequencia(&k("ctrl-c"), false), Some(vec![3]));
        assert_eq!(sequencia(&k("ctrl-space"), false), Some(vec![0]));
        assert_eq!(sequencia(&k("alt-b"), false), Some(b"\x1bb".to_vec()));
        assert_eq!(sequencia(&k("a"), false), None, "texto vai pela entrada de texto");
        assert_eq!(sequencia(&k("ctrl-alt-q"), false), None, "AltGr");
    }
}
