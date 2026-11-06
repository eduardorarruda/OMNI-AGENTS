//! A sessão que viu tarefa externa fica marcada: o que ela iniciar espera um humano. A marca na memória
//! do processo não basta, porque `--resume` e reconexão abrem outro MCP e há CLI que filtra o ambiente.
//! Por isso ela vai para o disco, um arquivo por pasta da conversa. É por pasta porque o MCP não recebe
//! identidade de sessão: o `initialize` só traz o nome do cliente. Vale `VALIDADE_DA_MARCA`, contada pela
//! data do arquivo, para o projeto não ficar preso para sempre.
use crate::engine::Contexto;
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::Duration,
};

pub const VALIDADE_DA_MARCA: Duration = Duration::from_secs(24 * 60 * 60);
pub const PASTA_DAS_MARCAS: &str = "marcas-de-fora";

fn arquivo_da_marca(contexto: &Contexto, pasta: &Path) -> Option<PathBuf> {
    if contexto.pasta_do_engine.as_os_str().is_empty() { return None; }
    let real = pasta.canonicalize().unwrap_or_else(|_| pasta.to_path_buf());
    let mut hash = DefaultHasher::new();
    real.hash(&mut hash);
    Some(contexto.pasta_do_engine.join(PASTA_DAS_MARCAS).join(format!("{:016x}", hash.finish())))
}

/// Sem gravar, quem chamou não entrega a tarefa: texto de fora sem marca é o que isto existe para evitar.
pub fn marcar(contexto: &Contexto) -> Result<(), String> {
    contexto.veio_de_fora.store(true, Ordering::SeqCst);
    let Some(pasta) = contexto.pasta_atual.as_deref() else { return Ok(()) };
    let arquivo = arquivo_da_marca(contexto, pasta).ok_or("Não achei a pasta de dados do OMNI para marcar a conversa.")?;
    let gravado = arquivo.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(&arquivo, pasta.to_string_lossy().as_bytes()));
    gravado.map_err(|erro| format!("Não consegui marcar a conversa como tarefa de fora, então a tarefa não foi entregue: {erro}"))
}

/// Data no futuro (relógio mexido) conta como marca nova: na dúvida, para.
fn no_prazo(arquivo: &Path) -> Option<bool> {
    let gravada = fs::metadata(arquivo).and_then(|dados| dados.modified()).ok()?;
    Some(gravada.elapsed().map_or(true, |idade| idade < VALIDADE_DA_MARCA))
}

pub fn marcada(contexto: &Contexto) -> bool {
    if contexto.veio_de_fora.load(Ordering::SeqCst) { return true; }
    let Some(arquivo) = contexto.pasta_atual.as_deref().and_then(|pasta| arquivo_da_marca(contexto, pasta)) else { return false };
    match no_prazo(&arquivo) {
        Some(true) => true,
        Some(false) => { let _ = fs::remove_file(&arquivo); false }
        None => false,
    }
}
