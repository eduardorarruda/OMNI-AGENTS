//! Regras puras do Kanban (sem I/O): o quadro, o cartão e as operações de um cartão por vez.
//! O engine guarda o arquivo; a tela manda uma operação por cartão, nunca o quadro inteiro, para dois
//! pedidos seguidos não se atropelarem.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub const STATUS_VALIDOS: [&str; 5] = ["backlog", "ready", "doing", "done", "failed"];
/// Chaves de `ajustes` que a tela conhece; qualquer outra é recusada.
pub const AJUSTES_VALIDOS: [&str; 4] = ["agentByProject", "dispatcherEnabled", "failureTimestamps", "pausedUntil"];
/// Teto da caixa de triagem. O quadro inteiro volta numa resposta só, que o engine corta em 4 MB.
pub const TRIAGEM_MAX: usize = 500;
const ID_MAX: usize = 80;
const TITULO_MAX: usize = 500;
const TEXTO_MAX: usize = 20_000;
const ORIGEM_MAX: usize = 200;
const ID_EXTERNO_MAX: usize = 200;

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
    /// Quem pegou sem passar pelo dispatcher. O dispatcher não tem sessão para esse cartão: sem isto,
    /// o daria como falho.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pego_por: Option<String>,
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

/// Uma tarefa como a busca a devolveu. A origem vem uma vez só, em `ImportacaoExterna`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TarefaDeFora {
    pub id: String,
    #[serde(default)]
    pub titulo: String,
    #[serde(default)]
    pub descricao: String,
}

/// O resultado inteiro de uma busca numa origem. A origem vem à parte porque lista vazia também é
/// resposta: tudo o que era daquela origem saiu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportacaoExterna {
    pub origem: String,
    pub tarefas: Vec<TarefaDeFora>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumoDaImportacao {
    pub novas: usize,
    /// Já estavam na triagem: título e descrição acompanham a origem.
    pub atualizadas: usize,
    /// Já viraram cartão de projeto: ficam como o usuário deixou.
    pub ja_no_quadro: usize,
    /// Saíram da origem nesta busca.
    pub sumiram: usize,
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
    /// Vira `true` na primeira migração ou no primeiro cartão criado. Daí em diante a cópia antiga do
    /// `localStorage` nunca mais volta, nem se o usuário apagar todos os cartões (quadro "vazio" de novo).
    #[serde(default)]
    pub migrado: bool,
}

fn objeto_vazio() -> Value {
    json!({})
}

impl Default for Quadro {
    fn default() -> Self {
        Self { versao: 1, tarefas: Vec::new(), triagem: Vec::new(), ajustes: objeto_vazio(), migrado: false }
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
    quadro.migrado = true;
    Ok(cartao)
}

const CAMPOS_EDITAVEIS: [&str; 11] = [
    "projectId", "title", "description", "status", "position", "loopTotal", "loopRemaining", "resultNote", "updatedAt", "externa", "pegoPor",
];
const PEGO_PELO_MCP: &str = "mcp";

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

/// Agente pegando um cartão. Conferir e mudar é uma operação só: quem chega depois recebe `Conflito`.
pub fn pegar(quadro: &mut Quadro, id: &str, agora: &str) -> Result<Cartao, ErroKanban> {
    let cartao = quadro.tarefas.iter_mut().find(|c| c.id == id).ok_or_else(|| ErroKanban::NaoEncontrado("Cartão não encontrado".into()))?;
    if !matches!(cartao.status.as_str(), "backlog" | "ready") {
        return Err(ErroKanban::Conflito("Só dá para pegar cartão do Backlog ou de Pronta".into()));
    }
    cartao.status = "doing".into();
    cartao.pego_por = Some(PEGO_PELO_MCP.into());
    cartao.updated_at = agora.to_owned();
    Ok(cartao.clone())
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

fn validar_importacao(importacao: &ImportacaoExterna) -> Result<(), ErroKanban> {
    if importacao.origem.is_empty() || importacao.origem.len() > ORIGEM_MAX {
        return Err(ErroKanban::Invalido("Origem da tarefa inválida".into()));
    }
    let invalida = |t: &TarefaDeFora| {
        t.id.is_empty() || t.id.len() > ID_EXTERNO_MAX || t.titulo.chars().count() > TITULO_MAX || t.descricao.chars().count() > TEXTO_MAX
    };
    if importacao.tarefas.iter().any(invalida) {
        return Err(ErroKanban::Invalido("Tarefa externa inválida".into()));
    }
    Ok(())
}

/// Confere o resultado de uma busca com o quadro. Chave: origem + id. Quem já virou cartão não volta
/// para a triagem e não é sobrescrito; o que saiu da origem fica, marcado, e perde a marca se voltar.
pub fn importar_externas(quadro: &mut Quadro, importacao: ImportacaoExterna) -> Result<ResumoDaImportacao, ErroKanban> {
    validar_importacao(&importacao)?;
    let ImportacaoExterna { origem, tarefas } = importacao;
    let mut vistas = HashSet::new();
    let mut resumo = ResumoDaImportacao::default();
    for tarefa in tarefas {
        if !vistas.insert(tarefa.id.clone()) {
            continue;
        }
        if let Some(cartao) = quadro.tarefas.iter_mut().find(|c| c.externa.as_ref().is_some_and(|e| e.origem == origem && e.id == tarefa.id)) {
            if let Some(externa) = cartao.externa.as_mut() {
                externa.sumiu = false;
            }
            resumo.ja_no_quadro += 1;
        } else if let Some(atual) = quadro.triagem.iter_mut().find(|t| t.origem == origem && t.id == tarefa.id) {
            atual.titulo = tarefa.titulo;
            atual.descricao = tarefa.descricao;
            atual.sumiu = false;
            resumo.atualizadas += 1;
        } else {
            quadro.triagem.push(CartaoExterno { origem: origem.clone(), id: tarefa.id, titulo: tarefa.titulo, descricao: tarefa.descricao, sumiu: false });
            resumo.novas += 1;
        }
    }
    if quadro.triagem.len() > TRIAGEM_MAX {
        return Err(ErroKanban::Invalido(format!("A triagem guarda até {TRIAGEM_MAX} tarefas. Filtre o que vem da API na configuração de Tarefas.")));
    }
    let saiu = |id: &str, sumiu: &mut bool, sumiram: &mut usize| {
        if !vistas.contains(id) && !*sumiu {
            *sumiu = true;
            *sumiram += 1;
        }
    };
    for tarefa in quadro.triagem.iter_mut().filter(|t| t.origem == origem) {
        saiu(&tarefa.id, &mut tarefa.sumiu, &mut resumo.sumiram);
    }
    for externa in quadro.tarefas.iter_mut().filter_map(|c| c.externa.as_mut()).filter(|e| e.origem == origem) {
        saiu(&externa.id, &mut externa.sumiu, &mut resumo.sumiram);
    }
    Ok(resumo)
}

/// Id de cartão vindo de fora: o id externo pode ter qualquer tamanho e caractere, e o do cartão vai
/// no caminho das rotas. FNV-1a basta: a mesma tarefa dá o mesmo id, e colisão cai em `Conflito`.
fn id_do_cartao_externo(origem: &str, id: &str) -> String {
    let hash = format!("{origem}\0{id}").bytes().fold(0xcbf2_9ce4_8422_2325_u64, |acumulado, byte| (acumulado ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    format!("externa-{hash:016x}")
}

/// O ISO que a tela também grava em `createdAt`/`updatedAt`.
pub fn instante_agora() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// O usuário escolheu o projeto: a tarefa sai da triagem e vira cartão no fim do Backlog. `origem` só é
/// obrigatória quando o mesmo id existe em duas origens.
pub fn escolher_projeto(quadro: &mut Quadro, id: &str, origem: Option<&str>, project_id: &str, agora: &str) -> Result<Cartao, ErroKanban> {
    let achadas: Vec<usize> = quadro.triagem.iter().enumerate()
        .filter(|(_, t)| t.id == id && origem.is_none_or(|o| o == t.origem))
        .map(|(indice, _)| indice)
        .collect();
    let indice = match achadas[..] {
        [] => return Err(ErroKanban::NaoEncontrado("Tarefa não encontrada na triagem".into())),
        [indice] => indice,
        _ => return Err(ErroKanban::Conflito("Esse id existe em mais de uma origem: informe a origem".into())),
    };
    let tarefa = quadro.triagem[indice].clone();
    let titulo = if tarefa.titulo.trim().is_empty() { format!("Tarefa {}", tarefa.id) } else { tarefa.titulo };
    let position = quadro.tarefas.iter().map(|c| c.position).max().unwrap_or(0) + 1;
    let cartao = Cartao {
        id: id_do_cartao_externo(&tarefa.origem, &tarefa.id), project_id: project_id.to_owned(), title: titulo, description: tarefa.descricao,
        status: "backlog".into(), position, loop_total: None, loop_remaining: None, result_note: None,
        created_at: agora.to_owned(), updated_at: agora.to_owned(),
        externa: Some(RefExterna { origem: tarefa.origem, id: tarefa.id, sumiu: tarefa.sumiu }), pego_por: None,
    };
    let criado = criar(quadro, cartao)?;
    quadro.triagem.remove(indice);
    Ok(criado)
}

/// Migração única do `localStorage`: só escreve se o quadro do engine estiver vazio. Cartão inválido
/// do material de origem é descartado sozinho. Devolve `true` se migrou.
pub fn migrar(quadro: &mut Quadro, origem: Quadro) -> bool {
    if quadro.migrado || !quadro.vazio() {
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
    novo.migrado = true;
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
            created_at: "2026-11-06T00:00:00Z".into(), updated_at: "2026-11-06T00:00:00Z".into(), externa: None, pego_por: None,
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
    fn apagar_todos_os_cartoes_nao_ressuscita_a_copia_antiga() {
        let mut q = Quadro::default();
        assert!(migrar(&mut q, Quadro { tarefas: vec![cartao("a")], ..Quadro::default() }));
        assert!(q.migrado);
        assert!(remover(&mut q, "a"));
        assert!(q.vazio());
        assert!(!migrar(&mut q, Quadro { tarefas: vec![cartao("a")], ..Quadro::default() }));
        assert!(q.tarefas.is_empty());
    }

    #[test]
    fn primeiro_cartao_novo_tambem_marca_migrado() {
        let mut q = Quadro::default();
        criar(&mut q, cartao("n")).unwrap();
        remover(&mut q, "n");
        assert!(q.migrado);
        assert!(!migrar(&mut q, Quadro { tarefas: vec![cartao("velho")], ..Quadro::default() }));
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
