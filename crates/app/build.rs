// Embute no exe do Windows o ícone (ID 1, o que a janela do gpui carrega) e o VERSIONINFO.
fn main() {
    println!("cargo:rerun-if-changed=empacotamento/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let raiz = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let icone = std::path::Path::new(&raiz).join("empacotamento").join("icon.ico");
    let versao = std::env::var("CARGO_PKG_VERSION").unwrap();
    let mut partes: Vec<u16> = versao
        .split(['.', '-', '+'])
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    partes.resize(4, 0);
    let numerica = partes.iter().map(u16::to_string).collect::<Vec<_>>().join(",");
    let rc = format!(
        r##"#pragma code_page(65001)
1 ICON "{icone}"
1 VERSIONINFO
FILEVERSION {numerica}
PRODUCTVERSION {numerica}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "Orquestrador"
      VALUE "ProductName", "Orquestrador"
      VALUE "FileVersion", "{versao}"
      VALUE "ProductVersion", "{versao}"
      VALUE "OriginalFilename", "orquestrador.exe"
      VALUE "InternalName", "orquestrador"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"##,
        icone = icone.display().to_string().replace('\\', "\\\\"),
    );
    let saida = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("orquestrador.rc");
    std::fs::write(&saida, rc).unwrap();
    embed_resource::compile(&saida, embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}
