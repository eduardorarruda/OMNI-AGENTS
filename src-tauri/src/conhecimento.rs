//! Tela Conhecimento: a base mora no crate `omni-rag`; aqui só ficam a importação em segundo plano
//! e o registro do MCP no Claude.
use omni_rag::{caminho_padrao, Base, Fonte, Resultado};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Once,
};

const NOME_DO_MCP: &str = "omni-rag";
const RESULTADOS_NA_TELA: usize = 10;
static RECUPERAR: Once = Once::new();

fn abrir() -> Result<Base, String> {
    let base = Base::abrir(&caminho_padrao().ok_or("pasta de dados não encontrada")?)?;
    RECUPERAR.call_once(|| { let _ = base.marcar_interrompidas(); });
    Ok(base)
}

async fn bloqueante<T: Send + 'static>(tarefa: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(tarefa).await.map_err(|erro| erro.to_string())?
}

#[tauri::command]
pub async fn rag_listar() -> Result<Vec<Fonte>, String> {
    bloqueante(|| abrir()?.listar()).await
}

/// Volta na hora: um banco grande leva minutos, e a tela acompanha pela lista de fontes.
#[tauri::command]
pub async fn rag_adicionar(caminhos: Vec<String>) -> Result<(), String> {
    let mut base = bloqueante(abrir).await?;
    std::thread::spawn(move || {
        for caminho in caminhos {
            if let Err(erro) = base.adicionar(Path::new(&caminho)) {
                eprintln!("omni-rag: {caminho}: {erro}");
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn rag_remover(id: i64) -> Result<(), String> {
    bloqueante(move || abrir()?.remover(id)).await
}

#[tauri::command]
pub async fn rag_buscar(consulta: String) -> Result<Vec<Resultado>, String> {
    bloqueante(move || abrir()?.buscar(&consulta, RESULTADOS_NA_TELA)).await
}

fn configuracao_do_claude() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(|home| PathBuf::from(home).join(".claude.json"))
}

#[tauri::command]
pub async fn rag_claude_ligado() -> Result<bool, String> {
    bloqueante(|| {
        let Some(caminho) = configuracao_do_claude() else { return Ok(false) };
        let Ok(texto) = std::fs::read_to_string(caminho) else { return Ok(false) };
        let estado: Value = serde_json::from_str(&texto).map_err(|erro| erro.to_string())?;
        Ok(estado["mcpServers"][NOME_DO_MCP].is_object())
    }).await
}

/// O app aberto pelo menu não tem o PATH do terminal: quem acha o `claude` é o shell de login.
fn no_shell(comando: &str) -> Result<(), String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let saida = Command::new(shell).args(["-l", "-c", comando]).output().map_err(|erro| erro.to_string())?;
    if saida.status.success() { return Ok(()); }
    Err(format!("o Claude recusou: {}", String::from_utf8_lossy(&saida.stderr).trim()))
}

#[tauri::command]
pub async fn rag_claude_ligar(ligado: bool) -> Result<(), String> {
    bloqueante(move || {
        if !ligado { return no_shell(&format!("claude mcp remove --scope user {NOME_DO_MCP}")); }
        let executavel = std::env::current_exe().map_err(|erro| erro.to_string())?.with_file_name(NOME_DO_MCP);
        if !executavel.is_file() { return Err(format!("não achei {}", executavel.display())); }
        no_shell(&format!("claude mcp add --scope user {NOME_DO_MCP} -- '{}' mcp", executavel.display()))
    }).await
}
