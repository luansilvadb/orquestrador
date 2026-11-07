//! Quadro binário da saída dos terminais: `[u8 n][projeto n bytes][u8 m][id m bytes][u64 seq LE][bytes]`.
//! `n` = 0 é o terminal sem projeto. O frontend decodifica em `src/backend/term-frame.ts`.

pub fn encode(project: Option<&str>, id: &str, seq: u64, bytes: &[u8]) -> Vec<u8> {
    let (project, id) = (project.unwrap_or("").as_bytes(), id.as_bytes());
    let mut out = Vec::with_capacity(2 + project.len() + id.len() + 8 + bytes.len());
    out.push(u8::try_from(project.len()).expect("id de projeto curto"));
    out.extend_from_slice(project);
    out.push(u8::try_from(id.len()).expect("id de terminal curto"));
    out.extend_from_slice(id);
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(bytes);
    out
}

#[cfg(test)]
mod tests {
    use super::encode;

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
}
