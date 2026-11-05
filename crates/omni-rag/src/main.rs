//! `omni-rag mcp` é o que o Claude executa. Os outros comandos servem para testar pelo terminal.
use omni_rag::{caminho_padrao, mcp, Base};
use std::{io, path::Path, process::ExitCode};

fn executar() -> Result<(), String> {
    let argumentos: Vec<String> = std::env::args().skip(1).collect();
    let caminho = caminho_padrao().ok_or("não achei a pasta de dados")?;
    let mut base = Base::abrir(&caminho)?;
    match argumentos.first().map(String::as_str) {
        Some("mcp") => mcp::servir(&base, io::stdin().lock(), io::stdout().lock()).map_err(|erro| erro.to_string()),
        Some("add") => {
            for arquivo in &argumentos[1..] {
                let fonte = base.adicionar(Path::new(arquivo))?;
                println!("{}: {} ({} trechos) {}", fonte.nome, fonte.estado, fonte.trechos, fonte.erro.unwrap_or_default());
            }
            Ok(())
        }
        Some("search") => {
            for item in base.buscar(&argumentos[1..].join(" "), 5)? {
                println!("#{} [{} · {}] {}", item.id, item.fonte, item.titulo, item.resumo);
            }
            Ok(())
        }
        Some("list") => {
            for fonte in base.listar()? {
                println!("{} {} {} trechos={} {}", fonte.id, fonte.nome, fonte.estado, fonte.trechos, fonte.erro.unwrap_or_default());
            }
            Ok(())
        }
        _ => Err("uso: omni-rag mcp | add <arquivos> | search <palavras> | list".into()),
    }
}

fn main() -> ExitCode {
    match executar() {
        Ok(()) => ExitCode::SUCCESS,
        Err(erro) => { eprintln!("{erro}"); ExitCode::FAILURE }
    }
}
