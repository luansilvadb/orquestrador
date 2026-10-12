use std::path::PathBuf;

fn main() {
    // O programa de delegação vai no instalador por `bundle.resources`, e o Tauri exige que a pasta do recurso
    // exista já no build: por isso ela é criada aqui. Mas o `tauri-build` copia o que houver nela para a pasta
    // do perfil (`target\<perfil>`), onde o Cargo põe o próprio programa; uma cópia ali sobrescreveria o programa
    // do perfil certo, com um de outro perfil ou de um build anterior. Por isso a pasta sai sempre sem o
    // programa, e quem o põe é o `beforeBundleCommand` (`npm run copy:delegar`), no `tauri build`, depois de
    // compilar tudo.
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let delegar = out.ancestors().nth(4).expect("pasta target").join("delegar");
    std::fs::create_dir_all(&delegar).expect("pasta do recurso de delegação");
    let _ = std::fs::remove_file(delegar.join("orquestrador-delegar.exe"));
    tauri_build::build()
}
