//! Regras puras do Kanban (sem I/O): o quadro, o cartão e as operações de um cartão por vez.
//! O engine guarda o arquivo; a tela manda uma operação por cartão, nunca o quadro inteiro, para dois
//! pedidos seguidos não se atropelarem.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const STATUS_VALIDOS: [&str; 5] = ["backlog", "ready", "doing", "done", "failed"];
/// Chaves de `ajustes` que a tela conhece; qualquer outra é recusada.
pub const AJUSTES_VALIDOS: [&str; 4] = ["agentByProject", "dispatcherEnabled", "failureTimestamps", "pausedUntil"];
const ID_MAX: usize = 80;
const TITULO_MAX: usize = 500;
const TEXTO_MAX: usize = 20_000;

/// De onde veio um cartão de fora. `sumiu`: a origem não devolve mais a tarefa; o cartão fica, marcado.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefExterna {
    pub origem: String,
    pub id: String,
    #[serde(default)]
    pub sumiu: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cartao {
    pub id: String,
    pub project_id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub status: String,
    #[serde(default)]
    pub position: i64,
    #[serde(default)]
    pub loop_total: Option<i64>,
    #[serde(default)]
    pub loop_remaining: Option<i64>,
    #[serde(default)]
    pub result_note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub externa: Option<RefExterna>,
}

/// Tarefa externa ainda sem projeto (caixa de triagem). Vira `Cartao` quando o usuário escolhe o projeto.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CartaoExterno {
    pub origem: String,
    pub id: String,
    #[serde(default)]
    pub titulo: String,
    #[serde(default)]
    pub descricao: String,
    #[serde(default)]
    pub sumiu: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quadro {
    pub versao: u32,
    #[serde(default)]
    pub tarefas: Vec<Cartao>,
    #[serde(default)]
    pub triagem: Vec<CartaoExterno>,
    #[serde(default = "objeto_vazio")]
    pub ajustes: Value,
}

fn objeto_vazio() -> Value {
    json!({})
}

impl Default for Quadro {
    fn default() -> Self {
        Self { versao: 1, tarefas: Vec::new(), triagem: Vec::new(), ajustes: objeto_vazio() }
    }
}

impl Quadro {
    /// Sem cartão, sem triagem e sem ajuste: o único estado em que a migração pode escrever.
    pub fn vazio(&self) -> bool {
        self.tarefas.is_empty() && self.triagem.is_empty() && self.ajustes.as_object().is_none_or(Map::is_empty)
    }
}

fn validar(cartao: &Cartao) -> Result<(), String> {
    if cartao.id.is_empty() || cartao.id.len() > ID_MAX {
        return Err("Id do cartão inválido".into());
    }
    if cartao.project_id.is_empty() || cartao.project_id.len() > 200 {
        return Err("Projeto do cartão inválido".into());
    }
    if cartao.title.trim().is_empty() || cartao.title.chars().count() > TITULO_MAX {
        return Err("Título do cartão inválido".into());
    }
    if cartao.description.chars().count() > TEXTO_MAX
        || cartao.result_note.as_deref().map_or(0, |n| n.chars().count()) > TEXTO_MAX
    {
        return Err("Texto do cartão grande demais".into());
    }
    if !STATUS_VALIDOS.contains(&cartao.status.as_str()) {
        return Err("Status do cartão inválido".into());
    }
    Ok(())
}

/// Erro de operação: `Conflito` (id repetido), `NaoEncontrado` ou `Invalido`.
#[derive(Debug, Clone, PartialEq)]
pub enum ErroKanban {
    Invalido(String),
    Conflito(String),
    NaoEncontrado(String),
}

pub fn criar(quadro: &mut Quadro, cartao: Cartao) -> Result<Cartao, ErroKanban> {
    validar(&cartao).map_err(ErroKanban::Invalido)?;
    if quadro.tarefas.iter().any(|c| c.id == cartao.id) {
        return Err(ErroKanban::Conflito("Já existe um cartão com esse id".into()));
    }
    quadro.tarefas.push(cartao.clone());
    Ok(cartao)
}

const CAMPOS_EDITAVEIS: [&str; 10] = [
    "projectId", "title", "description", "status", "position", "loopTotal", "loopRemaining", "resultNote", "updatedAt", "externa",
];

/// Troca só os campos enviados de UM cartão. Campo desconhecido ou valor inválido recusa tudo e não muda nada.
pub fn editar(quadro: &mut Quadro, id: &str, mudancas: &Value) -> Result<Cartao, ErroKanban> {
    let mudancas = mudancas.as_object().ok_or_else(|| ErroKanban::Invalido("Corpo precisa ser um objeto".into()))?;
    if let Some(chave) = mudancas.keys().find(|k| !CAMPOS_EDITAVEIS.contains(&k.as_str())) {
        return Err(ErroKanban::Invalido(format!("Campo não editável: {chave}")));
    }
    let posicao = quadro
        .tarefas
        .iter()
        .position(|c| c.id == id)
        .ok_or_else(|| ErroKanban::NaoEncontrado("Cartão não encontrado".into()))?;
    let mut atual = serde_json::to_value(&quadro.tarefas[posicao]).map_err(|e| ErroKanban::Invalido(e.to_string()))?;
    let alvo = atual.as_object_mut().ok_or_else(|| ErroKanban::Invalido("Cartão ilegível".into()))?;
    for (chave, valor) in mudancas {
        alvo.insert(chave.clone(), valor.clone());
    }
    let novo: Cartao = serde_json::from_value(atual).map_err(|e| ErroKanban::Invalido(format!("Valor inválido: {e}")))?;
    validar(&novo).map_err(ErroKanban::Invalido)?;
    quadro.tarefas[posicao] = novo.clone();
    Ok(novo)
}

/// Idempotente: apagar o que já não existe não é erro (o pedido pode ter sido repetido). Devolve se algo saiu.
pub fn remover(quadro: &mut Quadro, id: &str) -> bool {
    let antes = quadro.tarefas.len();
    quadro.tarefas.retain(|c| c.id != id);
    quadro.tarefas.len() != antes
}

pub fn ajustar(quadro: &mut Quadro, mudancas: &Value) -> Result<(), ErroKanban> {
    let mudancas = mudancas.as_object().ok_or_else(|| ErroKanban::Invalido("Corpo precisa ser um objeto".into()))?;
    if let Some(chave) = mudancas.keys().find(|k| !AJUSTES_VALIDOS.contains(&k.as_str())) {
        return Err(ErroKanban::Invalido(format!("Ajuste desconhecido: {chave}")));
    }
    if !quadro.ajustes.is_object() {
        quadro.ajustes = objeto_vazio();
    }
    if let Some(alvo) = quadro.ajustes.as_object_mut() {
        for (chave, valor) in mudancas {
            alvo.insert(chave.clone(), valor.clone());
        }
    }
    Ok(())
}

/// Migração única do `localStorage`: só escreve se o quadro do engine estiver vazio. Cartão inválido
/// do material de origem é descartado sozinho. Devolve `true` se migrou.
pub fn migrar(quadro: &mut Quadro, origem: Quadro) -> bool {
    if !quadro.vazio() {
        return false;
    }
    let mut novo = Quadro::default();
    for cartao in origem.tarefas {
        let _ = criar(&mut novo, cartao);
    }
    if ajustar(&mut novo, &origem.ajustes).is_err() {
        // ajuste estranho na origem não impede os cartões de migrar
        novo.ajustes = objeto_vazio();
    }
    if novo.tarefas.is_empty() && novo.ajustes.as_object().is_none_or(Map::is_empty) {
        return false;
    }
    *quadro = novo;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cartao(id: &str) -> Cartao {
        Cartao {
            id: id.into(), project_id: "p".into(), title: format!("Tarefa {id}"), description: String::new(),
            status: "backlog".into(), position: 1, loop_total: None, loop_remaining: None, result_note: None,
            created_at: "2026-11-06T00:00:00Z".into(), updated_at: "2026-11-06T00:00:00Z".into(), externa: None,
        }
    }

    #[test]
    fn editar_troca_so_o_cartao_pedido() {
        let mut q = Quadro::default();
        criar(&mut q, cartao("a")).unwrap();
        criar(&mut q, cartao("b")).unwrap();
        editar(&mut q, "a", &json!({"status":"ready","loopTotal":null})).unwrap();
        assert_eq!(q.tarefas[0].status, "ready");
        assert_eq!(q.tarefas[1], cartao("b"));
    }

    #[test]
    fn editar_recusa_campo_estranho_status_ruim_e_nao_muda_nada() {
        let mut q = Quadro::default();
        criar(&mut q, cartao("a")).unwrap();
        assert!(matches!(editar(&mut q, "a", &json!({"id":"x"})), Err(ErroKanban::Invalido(_))));
        assert!(matches!(editar(&mut q, "a", &json!({"status":"voando"})), Err(ErroKanban::Invalido(_))));
        assert!(matches!(editar(&mut q, "zz", &json!({"title":"x"})), Err(ErroKanban::NaoEncontrado(_))));
        assert_eq!(q.tarefas[0], cartao("a"));
    }

    #[test]
    fn criar_recusa_id_repetido() {
        let mut q = Quadro::default();
        criar(&mut q, cartao("a")).unwrap();
        assert!(matches!(criar(&mut q, cartao("a")), Err(ErroKanban::Conflito(_))));
        assert_eq!(q.tarefas.len(), 1);
    }

    #[test]
    fn remover_e_idempotente() {
        let mut q = Quadro::default();
        criar(&mut q, cartao("a")).unwrap();
        assert!(remover(&mut q, "a"));
        assert!(!remover(&mut q, "a"));
    }

    #[test]
    fn migrar_descarta_cartao_invalido_e_so_vale_com_quadro_vazio() {
        let mut ruim = cartao("r");
        ruim.status = "voando".into();
        let origem = Quadro { tarefas: vec![cartao("a"), ruim], ..Quadro::default() };
        let mut q = Quadro::default();
        assert!(migrar(&mut q, origem));
        assert_eq!(q.tarefas.len(), 1);
        assert!(!migrar(&mut q, Quadro { tarefas: vec![cartao("z")], ..Quadro::default() }));
        assert_eq!(q.tarefas[0].id, "a");
    }

    #[test]
    fn migrar_com_origem_sem_nada_nao_conta_como_migracao() {
        let mut q = Quadro::default();
        assert!(!migrar(&mut q, Quadro::default()));
    }

    #[test]
    fn ajustes_so_aceita_chaves_conhecidas_e_mescla() {
        let mut q = Quadro::default();
        ajustar(&mut q, &json!({"dispatcherEnabled":true})).unwrap();
        ajustar(&mut q, &json!({"pausedUntil":null})).unwrap();
        assert_eq!(q.ajustes["dispatcherEnabled"], json!(true));
        assert!(ajustar(&mut q, &json!({"lixo":1})).is_err());
    }
}
