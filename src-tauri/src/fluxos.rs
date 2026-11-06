//! Fluxos salvos (o editor visual). A regra mora em `omni_core::fluxos`; executar é com o engine.

use omni_core::fluxos::{self, Fluxo, FluxoSalvo};
use std::path::{Path, PathBuf};

fn pasta(escopo: &str, projeto: Option<&str>) -> Result<PathBuf, String> {
    match (escopo, projeto) {
        ("global", _) => fluxos::pasta_global().ok_or_else(|| "Pasta do usuário indisponível".into()),
        ("projeto", Some(p)) if !p.trim().is_empty() => Ok(fluxos::pasta_do_projeto(Path::new(p))),
        ("projeto", _) => Err("Abra um projeto para salvar nele".into()),
        _ => Err("Escopo precisa ser global ou projeto".into()),
    }
}

#[tauri::command(async)]
pub fn fluxos_listar(project_path: Option<String>) -> Vec<FluxoSalvo> {
    fluxos::listar(fluxos::pasta_global().as_deref(), project_path.as_deref().filter(|p| !p.is_empty()).map(Path::new))
}

#[tauri::command(async)]
pub fn fluxos_salvar(escopo: String, project_path: Option<String>, fluxo: Fluxo, id_anterior: Option<String>) -> Result<String, String> {
    let pasta = pasta(&escopo, project_path.as_deref())?;
    let caminho = fluxos::salvar(&pasta, &fluxo)?;
    if let Some(antigo) = id_anterior.filter(|i| i != &fluxo.id) { fluxos::apagar(&pasta, &antigo)?; }
    Ok(caminho.to_string_lossy().into_owned())
}

#[tauri::command(async)]
pub fn fluxos_apagar(escopo: String, project_path: Option<String>, id: String) -> Result<(), String> {
    fluxos::apagar(&pasta(&escopo, project_path.as_deref())?, &id)
}

#[tauri::command]
pub fn fluxos_validar(fluxo: Fluxo) -> Result<(), String> {
    fluxos::validar(&fluxo)
}

/// Avisos que não impedem salvar (hoje: ciclos entre subfluxos), comparando com os fluxos já salvos.
#[tauri::command(async)]
pub fn fluxos_avisos(fluxo: Fluxo, project_path: Option<String>) -> Vec<String> {
    let todos: Vec<Fluxo> = fluxos_listar(project_path).into_iter().map(|salvo| salvo.fluxo).collect();
    fluxos::avisos_de_ciclo(&fluxo, &todos)
}

/// Monta um fluxo inteiro a partir de uma frase. A IA conhece os modelos de agente que já existem e
/// pode propor novos; o resultado volta para a tela revisar antes de salvar (nada é gravado aqui).
/// JSON fora do formato: uma segunda tentativa com o erro, para a IA corrigir.
#[tauri::command(async)]
pub fn fluxos_criar_com_ia(descricao: String, project_path: Option<String>) -> Result<fluxos::FluxoGerado, String> {
    let descricao = descricao.trim();
    if descricao.len() < 10 { return Err("Descreva o fluxo com um pouco mais de detalhe".into()); }
    let agentes: Vec<omni_core::agentes::ModeloDeAgente> = crate::agentes::agentes_listar(project_path).into_iter().map(|a| a.modelo).collect();
    let nomes: Vec<String> = agentes.iter().map(|a| a.name.clone()).collect();
    let modelos = omni_core::agentes::modelos_disponiveis(&crate::profiles::list_profiles());
    let pedido = fluxos::pedido_de_fluxo(descricao, &agentes, &omni_core::agentes::modelos_em_texto(&modelos, "claude"));
    let primeira = crate::agentes::claude_sem_tela(fluxos::SISTEMA_CRIAR_FLUXO, &pedido, "opus", "high", None, crate::agentes::CRIACAO_MAX)?;
    match fluxos::fluxo_da_resposta(&primeira, &nomes) {
        Ok(f) => Ok(f),
        Err(erro) => {
            let correcao = format!("{pedido}\n\n## Sua resposta anterior\n{primeira}\n\n## Problema encontrado\n{erro}\n\nCorrija e devolva só o JSON completo.");
            let segunda = crate::agentes::claude_sem_tela(fluxos::SISTEMA_CRIAR_FLUXO, &correcao, "opus", "high", None, crate::agentes::CRIACAO_MAX)?;
            fluxos::fluxo_da_resposta(&segunda, &nomes).map_err(|e| format!("A IA não conseguiu montar um fluxo válido: {e}"))
        }
    }
}

