//! Quadro binário da saída dos terminais: `[u8 n][projeto n bytes][u8 m][id m bytes][u64 seq LE][bytes]`.
//! `n` = 0 é o terminal sem projeto. Um envio do canal leva vários, cada um com o tamanho na frente (`push`).
//! O frontend decodifica em `src/backend/term-frame.ts`.

fn len(project: &[u8], id: &[u8], bytes: &[u8]) -> usize {
    2 + project.len() + id.len() + 8 + bytes.len()
}

fn write(out: &mut Vec<u8>, project: &[u8], id: &[u8], seq: u64, bytes: &[u8]) {
    out.push(u8::try_from(project.len()).expect("id de projeto curto"));
    out.extend_from_slice(project);
    out.push(u8::try_from(id.len()).expect("id de terminal curto"));
    out.extend_from_slice(id);
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(bytes);
}

pub fn encode(project: Option<&str>, id: &str, seq: u64, bytes: &[u8]) -> Vec<u8> {
    let (project, id) = (project.unwrap_or("").as_bytes(), id.as_bytes());
    let mut out = Vec::with_capacity(len(project, id, bytes));
    write(&mut out, project, id, seq, bytes);
    out
}

/// Acrescenta a `out` um quadro com o tamanho na frente (`[u32 LE][quadro]`).
pub fn push(out: &mut Vec<u8>, project: Option<&str>, id: &str, seq: u64, bytes: &[u8]) {
    let (project, id) = (project.unwrap_or("").as_bytes(), id.as_bytes());
    let n = len(project, id, bytes);
    out.reserve(4 + n);
    out.extend_from_slice(&u32::try_from(n).expect("quadro abaixo de 4 GB").to_le_bytes());
    write(out, project, id, seq, bytes);
}

#[cfg(test)]
mod tests {
    use super::{encode, push};

    #[test]
    fn quadro_com_projeto_e_sem_projeto() {
        let q = encode(Some("project-3"), "shell-1", 258, &[0xe2, 0x82]);
        assert_eq!(&q[..11], b"\x09project-3\x07");
        assert_eq!(&q[11..18], b"shell-1");
        assert_eq!(&q[18..26], &[2, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&q[26..], &[0xe2, 0x82]);
        let q = encode(None, "orchestrator", 1, b"");
        assert_eq!(&q[..2], b"\x00\x0c");
        assert_eq!(q.len(), 2 + 12 + 8);
    }

    #[test]
    fn varios_quadros_com_o_tamanho_na_frente() {
        let mut out = Vec::new();
        push(&mut out, Some("p"), "a", 1, b"xy");
        push(&mut out, None, "b", 2, b"");
        let first = encode(Some("p"), "a", 1, b"xy");
        assert_eq!(&out[..4], &(first.len() as u32).to_le_bytes());
        assert_eq!(&out[4..4 + first.len()], &first[..]);
        let rest = &out[4 + first.len()..];
        assert_eq!(&rest[4..], &encode(None, "b", 2, b"")[..]);
    }
}
