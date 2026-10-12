//! O programa de delegação do orquestrador. É de console, de propósito: o PowerShell espera o término
//! de um programa de console e recebe o código de saída dele. Um programa de janela o PowerShell não
//! espera (medido: `$LASTEXITCODE` fica vazio e a saída chega depois). A lógica está em
//! `orquestrador_lib::delegate`.
use std::ffi::OsString;

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    std::process::exit(orquestrador_lib::delegate::run(&args));
}
