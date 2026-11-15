// O FFmpeg do vcpkg vem com Media Foundation, D3D11VA e DXVA2; as bibliotecas do sistema que eles pedem não vêm pelo
// `ffmpeg-sys-next` quando a ligação é estática.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        for lib in ["mfuuid", "strmiids", "mfplat", "ole32", "user32", "bcrypt", "secur32", "ws2_32"] {
            println!("cargo:rustc-link-lib={lib}");
        }
    }
}
