//! Busca as tarefas na API externa. As regras moram em `omni_rag::tarefas`; aqui só ficam a requisição
//! e a ordem das coisas. A API pode ser um servidor frágil: nada aqui roda sozinho, toda busca nasce de
//! um gesto na tela e passa pelo bloqueio gravado no cache.
use crate::conhecimento::bloqueante;
use omni_rag::{
    agora_ms, caminho_padrao,
    tarefas::{self, Cache, ConfigEmDisco, ConfigNaTela, Configuracao, EstadoDaTela},
};
use reqwest::header::{HeaderName, HeaderValue};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

const TEMPO_LIMITE: Duration = Duration::from_secs(60);
const RESPOSTA_MAXIMA: usize = 64 * 1024 * 1024;
static BUSCANDO: AtomicBool = AtomicBool::new(false);

/// Duas telas abertas não viram duas requisições: o bloqueio de 60 s só existe depois de gravado.
struct BuscaEmAndamento;

impl BuscaEmAndamento {
    fn tomar() -> Result<Self, String> {
        BUSCANDO.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map(|_| Self).map_err(|_| "Já existe uma busca em andamento.".to_string())
    }
}

impl Drop for BuscaEmAndamento {
    fn drop(&mut self) {
        BUSCANDO.store(false, Ordering::SeqCst);
    }
}

fn base() -> Result<PathBuf, String> {
    caminho_padrao().ok_or_else(|| "pasta de dados não encontrada".to_string())
}

fn arquivo_de_config() -> Result<PathBuf, String> {
    Ok(tarefas::arquivo_de_config(&base()?))
}

fn arquivo_de_cache() -> Result<PathBuf, String> {
    Ok(tarefas::arquivo_de_cache(&base()?))
}

fn estado_atual() -> Result<EstadoDaTela, String> {
    let configurada = tarefas::carregar_config(&arquivo_de_config()?)?.configuracao.pronta();
    Ok(tarefas::estado_da_tela(configurada, tarefas::ler_cache(&arquivo_de_cache()?), agora_ms()))
}

#[tauri::command]
pub async fn tarefas_config() -> Result<ConfigNaTela, String> {
    bloqueante(|| Ok(tarefas::carregar_config(&arquivo_de_config()?)?.para_a_tela())).await
}

#[tauri::command]
pub async fn tarefas_salvar(configuracao: Configuracao, chave: Option<String>, remover_chave: bool) -> Result<(), String> {
    // Uma busca em andamento regravaria o cache com o resultado da configuração antiga.
    let _busca = BuscaEmAndamento::tomar()?;
    // Cabeçalho sem chave não passa na validação, e a pessoa ficaria sem como apagar a chave.
    let configuracao = if remover_chave { Configuracao { cabecalho: String::new(), ..configuracao } } else { configuracao };
    bloqueante(move || {
        let caminho = arquivo_de_config()?;
        let atual = tarefas::carregar_config(&caminho)?;
        let chave = tarefas::chave_atualizada(&atual.chave, chave.as_deref(), remover_chave);
        configuracao.validar(!chave.is_empty())?;
        let mudou = configuracao != atual.configuracao;
        tarefas::salvar_config(&caminho, &ConfigEmDisco { configuracao, chave })?;
        if !mudou { return Ok(()); }
        let cache = arquivo_de_cache()?;
        tarefas::gravar_cache(&cache, &tarefas::cache_sem_tarefas(&tarefas::ler_cache(&cache)))
    }).await
}

#[tauri::command]
pub async fn tarefas_estado() -> Result<EstadoDaTela, String> {
    bloqueante(estado_atual).await
}

/// Confere o bloqueio e já grava a tentativa: se a requisição travar ou o app cair, ela conta igual.
fn preparar_busca() -> Result<(ConfigEmDisco, i64), String> {
    let arquivo = tarefas::carregar_config(&arquivo_de_config()?)?;
    if !arquivo.configuracao.pronta() { return Err("Configure a API de tarefas primeiro.".into()); }
    let caminho = arquivo_de_cache()?;
    let cache = tarefas::ler_cache(&caminho);
    let agora = agora_ms();
    let espera = tarefas::espera_ms(&cache, agora);
    if espera > 0 { return Err(format!("Aguarde {} s para buscar de novo.", (espera + 999) / 1000)); }
    tarefas::gravar_cache(&caminho, &Cache { tentado_em_ms: agora, ..cache })?;
    Ok((arquivo, agora))
}

/// A URL fica de fora: ela pode levar parâmetros, e o erro vai para a tela.
fn descrever(erro: reqwest::Error) -> String {
    if erro.is_timeout() { return format!("A API não respondeu em {} s.", TEMPO_LIMITE.as_secs()); }
    format!("Falha ao falar com a API: {}", erro.without_url())
}

fn autenticar(pedido: reqwest::RequestBuilder, arquivo: &ConfigEmDisco) -> Result<reqwest::RequestBuilder, String> {
    if arquivo.configuracao.cabecalho.is_empty() { return Ok(pedido); }
    let nome = HeaderName::from_bytes(arquivo.configuracao.cabecalho.as_bytes()).map_err(|_| "Nome do cabeçalho inválido.".to_string())?;
    let mut valor = HeaderValue::from_str(&arquivo.chave).map_err(|_| "A chave tem caracteres inválidos.".to_string())?;
    valor.set_sensitive(true);
    Ok(pedido.header(nome, valor))
}

async fn ler_corpo(mut resposta: reqwest::Response) -> Result<Vec<u8>, String> {
    let mut corpo = Vec::new();
    while let Some(pedaco) = resposta.chunk().await.map_err(descrever)? {
        if corpo.len() + pedaco.len() > RESPOSTA_MAXIMA { return Err("A resposta da API é grande demais.".into()); }
        corpo.extend_from_slice(&pedaco);
    }
    Ok(corpo)
}

/// Sem seguir redirecionamento: o cabeçalho da chave iria junto para outro endereço, até em http.
async fn buscar_json(arquivo: &ConfigEmDisco) -> Result<Value, String> {
    let url = reqwest::Url::parse(&arquivo.configuracao.url).map_err(|_| "URL inválida.".to_string())?;
    tarefas::validar_url(url.as_str())?;
    // Mesmo provedor TLS de `icones_remotos.rs`; sem ele o reqwest "no-provider" não conecta.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let cliente = reqwest::Client::builder().timeout(TEMPO_LIMITE).redirect(reqwest::redirect::Policy::none()).build().map_err(descrever)?;
    let resposta = autenticar(cliente.get(url), arquivo)?.send().await.map_err(descrever)?;
    let status = resposta.status();
    if !status.is_success() { return Err(format!("A API respondeu {}.", status.as_u16())); }
    serde_json::from_slice(&ler_corpo(resposta).await?).map_err(|_| "A resposta da API não é JSON.".to_string())
}

fn concluir_busca(configuracao: &Configuracao, resposta: &Value, tentado_em_ms: i64) -> Result<EstadoDaTela, String> {
    let resultado = tarefas::processar(configuracao, resposta)?;
    let cache = Cache {
        buscado_em_ms: agora_ms(),
        tentado_em_ms,
        recebidas: resultado.recebidas,
        avisos: resultado.avisos,
        tarefas: resultado.tarefas,
    };
    tarefas::gravar_cache(&arquivo_de_cache()?, &cache)?;
    estado_atual()
}

#[tauri::command]
pub async fn tarefas_atualizar() -> Result<EstadoDaTela, String> {
    let _busca = BuscaEmAndamento::tomar()?;
    let (arquivo, tentado_em_ms) = bloqueante(preparar_busca).await?;
    let resposta = buscar_json(&arquivo).await?;
    bloqueante(move || concluir_busca(&arquivo.configuracao, &resposta, tentado_em_ms)).await
}

#[tauri::command]
pub async fn tarefas_texto(id: String) -> Result<String, String> {
    bloqueante(move || {
        let cache = tarefas::ler_cache(&arquivo_de_cache()?);
        let tarefa = cache.tarefas.iter().find(|tarefa| tarefa.id == id).ok_or("Tarefa não encontrada no cache.")?;
        Ok(tarefas::texto_para_conversa(tarefa, tarefas::LIMITE_DO_TEXTO))
    }).await
}
