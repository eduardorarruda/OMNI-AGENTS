//! `GET /segundo-plano?projeto=<id>`: o que roda por baixo dos panos, numa lista só. Sessões do
//! Claude esperando agente em segundo plano, os subagentes dele (pelos `.jsonl`) e os fluxos vivos.
//! Só entram sessões abertas pelo OMNI: as outras não têm como ser paradas daqui.
use super::*;
use axum::{Router, Json, extract::{State, Query}, routing::get};
use omni_core::segundo_plano::{ItemDeFundo, filtrar_por_projeto, subagentes_ativos};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Sessão do OMNI com o que a tela e o disco dizem dela. Montada em `coletar`; o resto é puro.
pub struct SessaoNoOmni {
    pub conversa_id: String,
    pub titulo: String,
    pub projeto_id: String,
    pub provider: String,
    /// A CLI mostra agente em segundo plano pendente (só o Claude).
    pub em_segundo_plano: bool,
    /// Pasta `<conversa>/subagents` do transcript da sessão.
    pub pasta_de_subagentes: Option<PathBuf>,
}

/// Quantos subagentes concluídos o painel guarda (os mais recentes).
pub const CONCLUIDOS_MAX: usize = 10;

/// Une sessões, subagentes e fluxos vivos. `execucoes` é o formato de `fluxos::listar()`. Subagentes que
/// terminaram vêm com `concluido_ms` (no máximo [`CONCLUIDOS_MAX`]).
pub fn reunir(sessoes: &[SessaoNoOmni], execucoes: &[Value], agora_ms: u64) -> Vec<ItemDeFundo> {
    let mut itens = Vec::new();
    for s in sessoes {
        if s.provider == "claude" && s.em_segundo_plano {
            itens.push(ItemDeFundo { tipo: "sessao".into(), titulo: s.titulo.clone(), atividade: "Esperando um agente em segundo plano terminar".into(),
                desde_ms: None, projeto_id: s.projeto_id.clone(), sessao: Some(s.conversa_id.clone()), execucao: None, subagente: None, concluido_ms: None });
        }
        let subagentes = s.pasta_de_subagentes.as_deref().filter(|_| s.provider == "claude").map(|p| subagentes_ativos(p, agora_ms)).unwrap_or_default();
        for a in subagentes {
            let atividade = if !a.ultima_acao.is_empty() { a.ultima_acao } else if a.concluido { "Concluído".into() } else { "Trabalhando".into() };
            itens.push(ItemDeFundo { tipo: "subagente".into(), titulo: a.nome, atividade,
                desde_ms: Some(a.desde_ms), projeto_id: s.projeto_id.clone(), sessao: Some(s.conversa_id.clone()), execucao: None,
                subagente: Some(a.id), concluido_ms: a.concluido.then_some(a.atualizado_ms) });
        }
    }
    for e in execucoes {
        if !matches!(e["status"].as_str(), Some("rodando" | "aguardando")) { continue; }
        itens.push(ItemDeFundo { tipo: "fluxo".into(), titulo: e["fluxo"].as_str().unwrap_or("Fluxo").to_string(),
            atividade: if e["status"] == "aguardando" { "Esperando a sua aprovação".into() } else { "Rodando".into() },
            desde_ms: e["criado_ms"].as_u64(), projeto_id: e["projeto_id"].as_str().unwrap_or("").to_string(),
            sessao: None, execucao: e["id"].as_str().map(str::to_string), subagente: None, concluido_ms: None });
    }
    itens.sort_by_key(|i| i.desde_ms.unwrap_or(0));
    let mut fins: Vec<u64> = itens.iter().filter_map(|i| i.concluido_ms).collect();
    fins.sort_unstable_by(|a, b| b.cmp(a));
    let corte = fins.get(CONCLUIDOS_MAX - 1).copied().unwrap_or(0);
    let mut guardados = 0;
    itens.retain(|i| match i.concluido_ms {
        Some(fim) if fim >= corte && guardados < CONCLUIDOS_MAX => { guardados += 1; true }
        Some(_) => false,
        None => true,
    });
    itens
}

fn pasta_de_subagentes(transcript: &str) -> PathBuf {
    let caminho = Path::new(transcript);
    caminho.with_extension("").join("subagents")
}

/// Só em memória: a rota é chamada a cada 4 s, e em repouso não vale ler e abrir o índice de conversas.
fn ha_conversa_viva(state: &EngineState) -> bool {
    state.sessions.lock().is_ok_and(|sessoes| sessoes.values().any(|entrada| matches!(entrada,
        SessionEntry::Live(sessao) if sessao.meta.lock().is_ok_and(|meta| meta.conversation_id.is_some()))))
}

/// As sessões vivas do OMNI. Conversa que só existe no histórico da CLI não passa por aqui.
fn sessoes_do_omni(state: &EngineState, ler_indice: impl FnOnce() -> Vec<omni_core::conversations::Conversation>) -> Vec<SessaoNoOmni> {
    if !ha_conversa_viva(state) { return Vec::new(); }
    ler_indice().into_iter().filter_map(|c| {
        let viva = mobile::linked_session(state, &c)?;
        let segmento = c.segments.last()?;
        let texto = viva.interaction.lock().ok().map(|i| i.parser.screen().contents()).unwrap_or_default();
        Some(SessaoNoOmni {
            titulo: omni_core::conversations::display_title(&c), projeto_id: c.project_id.clone(), conversa_id: c.id.clone(),
            provider: segmento.provider.clone(),
            em_segundo_plano: segmento.provider == "claude" && interaction::agente_em_segundo_plano(&texto),
            pasta_de_subagentes: segmento.transcript_path.as_deref().map(pasta_de_subagentes),
        })
    }).collect()
}

fn indice(state: &EngineState) -> Vec<omni_core::conversations::Conversation> {
    omni_core::conversations::read_index(state.state_file.parent().unwrap())
}

fn coletar_com(state: &EngineState, execucoes: &[Value], projeto: Option<&str>, ler_indice: impl FnOnce() -> Vec<omni_core::conversations::Conversation>) -> Vec<ItemDeFundo> {
    filtrar_por_projeto(reunir(&sessoes_do_omni(state, ler_indice), execucoes, now_ms()), projeto)
}

pub fn coletar(state: &EngineState, projeto: Option<&str>) -> Vec<ItemDeFundo> {
    coletar_com(state, &fluxos::listar(), projeto, || indice(state))
}

#[derive(Deserialize)] struct Filtro { #[serde(default)] projeto: Option<String> }

/// `itens`: o que roda agora. `concluidos`: subagentes da sessão viva que terminaram há pouco.
async fn rota(State(state): State<Arc<EngineState>>, Query(filtro): Query<Filtro>) -> Json<Value> {
    let todos = tokio::task::spawn_blocking(move || coletar(&state, filtro.projeto.as_deref().filter(|p| !p.is_empty()))).await.unwrap_or_default();
    let (concluidos, itens): (Vec<_>, Vec<_>) = todos.into_iter().partition(|i| i.concluido_ms.is_some());
    Json(json!({ "itens": itens, "concluidos": concluidos }))
}

pub fn rotas() -> Router<Arc<EngineState>> { Router::new().route("/segundo-plano", get(rota)) }

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    fn sessao(conversa: &str, projeto: &str, pasta: Option<PathBuf>, em_fundo: bool) -> SessaoNoOmni {
        SessaoNoOmni { conversa_id: conversa.into(), titulo: format!("conversa {conversa}"), projeto_id: projeto.into(), provider: "claude".into(),
            em_segundo_plano: em_fundo, pasta_de_subagentes: pasta }
    }

    fn subagente_recente(pasta: &Path) {
        std::fs::create_dir_all(pasta).unwrap();
        std::fs::write(pasta.join("agent-x.jsonl"), r#"{"type":"user","timestamp":"2026-11-06T18:00:00Z","message":{"content":"tarefa"}}"#).unwrap();
    }

    fn agora() -> u64 { now_ms() }

    #[test]
    fn rota_filtra_por_projeto() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        subagente_recente(&a);
        subagente_recente(&b);
        let sessoes = [sessao("ca", "pa", Some(a), true), sessao("cb", "pb", Some(b), false)];
        let execucoes = [json!({"id":"e1","fluxo":"Revisar","projeto_id":"pa","status":"rodando","criado_ms":9}),
            json!({"id":"e2","fluxo":"Velho","projeto_id":"pa","status":"concluido","criado_ms":9}),
            json!({"id":"e3","fluxo":"Outro","projeto_id":"pb","status":"aguardando","criado_ms":9})];
        let todos = reunir(&sessoes, &execucoes, agora());
        assert_eq!(todos.len(), 5, "sessão+subagente de pa, subagente de pb, fluxos e1 e e3 (e2 terminou)");
        let so_a = filtrar_por_projeto(todos, Some("pa"));
        assert_eq!(so_a.iter().map(|i| i.tipo.as_str()).count(), 3);
        assert!(so_a.iter().all(|i| i.projeto_id == "pa"));
        assert!(so_a.iter().find(|i| i.tipo == "sessao").is_some_and(|i| i.desde_ms.is_none()), "sessão não tem início do segundo plano: sem tempo");
        assert!(so_a.iter().any(|i| i.tipo == "fluxo" && i.execucao.as_deref() == Some("e1")));
    }

    #[test]
    fn sessao_fora_do_omni_nao_entra() {
        // Subagente recente no disco, mas nenhuma sessão do OMNI aponta para ele.
        let dir = tempfile::tempdir().unwrap();
        subagente_recente(&dir.path().join("subagents"));
        assert!(reunir(&[], &[], agora()).is_empty());
        // E uma conversa do índice sem sessão viva (só histórico) não vira linha.
        let state = state_de_teste(dir.path());
        std::fs::write(dir.path().join("conversations.json"), json!({"version":1,"conversations":[{
            "id":"c","project_id":"p","cwd":"/x","title":"T","created_at_ms":1,"segments":[{"provider":"claude","profile_id":null,
            "external_session_id":null,"transcript_path":dir.path().join("t.jsonl").to_string_lossy(),"terminal_session_id":null,
            "started_at_ms":1,"ended_at_ms":null}]}]}).to_string()).unwrap();
        assert!(reunir(&sessoes_do_omni(&state, || indice(&state)), &[], agora()).is_empty(), "o registro global de fluxos fica de fora: outros testes o usam");
    }

    #[test]
    fn em_repouso_nao_le_o_indice_e_mantem_os_fluxos() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_de_teste(dir.path());
        let execucoes = [json!({"id":"e1","fluxo":"Revisar","projeto_id":"pa","status":"rodando","criado_ms":9})];
        let itens = coletar_com(&state, &execucoes, None, || panic!("sem sessão viva, o índice não pode ser lido"));
        assert_eq!(itens.iter().map(|i| i.execucao.as_deref()).collect::<Vec<_>>(), [Some("e1")]);
    }

    #[test]
    fn sessao_que_ja_terminou_nao_custa_a_leitura_do_indice() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_de_teste(dir.path());
        let historica = serde_json::from_value(json!({"id":"s","project_id":"p","name":"T","cwd":"/x","shell":"sh","state":"stopped","pid":null,
            "created_at_ms":1,"last_activity_at_ms":1,"output_seq":0,"rows":20,"cols":100,"conversation_id":"c"})).unwrap();
        state.sessions.lock().unwrap().insert("s".into(), SessionEntry::Historical(historica));
        coletar_com(&state, &[], None, || panic!("só sessão terminada: o índice não pode ser lido"));
    }

    fn state_de_teste(dir: &Path) -> Arc<EngineState> {
        Arc::new(EngineState { persist_lock: Mutex::new(()), mobile: mobile::MobileRuntime::new(dir), token: "t".into(),
            state_file: dir.join("sessions.json"), sessions: Mutex::new(HashMap::new()), id_sequence: AtomicU64::new(1) })
    }

    #[tokio::test]
    async fn rota_responde_a_lista_e_aceita_o_filtro() {
        let dir = tempfile::tempdir().unwrap();
        let app = rotas().with_state(state_de_teste(dir.path()));
        for uri in ["/segundo-plano", "/segundo-plano?projeto=p"] {
            let r = app.clone().oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(r.status(), 200);
            let corpo: Value = serde_json::from_slice(&axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap()).unwrap();
            assert!(corpo["itens"].is_array());
        }
    }

    /// Precisa de tela real do Claude Code com "Waiting for N background agent": `claude.segundoplano.*.txt`
    /// em `tests-fixtures/telas/`. Capturar depende do usuário (rede + CLI), então fica ignorado até lá.
    #[test]
    #[ignore = "falta capturar a tela real de agente em segundo plano no acervo (LEIA.md); depende do usuário"]
    fn tela_capturada_marca_segundo_plano() {
        let pasta = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests-fixtures/telas");
        let telas: Vec<_> = std::fs::read_dir(pasta).unwrap().flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("claude.segundoplano.")).collect();
        assert!(!telas.is_empty(), "nenhuma tela real capturada ainda");
        for t in telas {
            let texto = std::fs::read_to_string(t.path()).unwrap();
            assert!(interaction::agente_em_segundo_plano(&texto), "{:?}", t.file_name());
        }
    }

    #[test]
    fn subagente_leva_o_id_e_concluido_vai_a_parte() {
        let dir = tempfile::tempdir().unwrap();
        let pasta = dir.path().join("subagents");
        subagente_recente(&pasta);
        std::fs::write(pasta.join("agent-y.jsonl"), [
            r#"{"type":"user","isSidechain":true,"timestamp":"2026-11-06T18:00:00Z","message":{"content":"outra"}}"#,
            r#"{"type":"assistant","isSidechain":true,"message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"pronto"}]}}"#,
        ].join("\n")).unwrap();
        let itens = reunir(&[sessao("ca", "pa", Some(pasta), false)], &[], agora());
        let x = itens.iter().find(|i| i.subagente.as_deref() == Some("x")).unwrap();
        assert_eq!((x.sessao.as_deref(), x.concluido_ms), (Some("ca"), None));
        assert!(itens.iter().find(|i| i.subagente.as_deref() == Some("y")).is_some_and(|i| i.concluido_ms.is_some()));
    }

    #[test]
    fn guarda_so_os_concluidos_mais_recentes() {
        let dir = tempfile::tempdir().unwrap();
        let pasta = dir.path().join("subagents");
        std::fs::create_dir_all(&pasta).unwrap();
        for n in 0..CONCLUIDOS_MAX + 3 {
            std::fs::write(pasta.join(format!("agent-c{n}.jsonl")), r#"{"type":"assistant","message":{"stop_reason":"end_turn","content":[]}}"#).unwrap();
        }
        let itens = reunir(&[sessao("ca", "pa", Some(pasta), false)], &[], agora());
        assert_eq!(itens.iter().filter(|i| i.concluido_ms.is_some()).count(), CONCLUIDOS_MAX);
    }
}
