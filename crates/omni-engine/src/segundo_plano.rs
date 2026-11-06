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
    pub desde_ms: u64,
    /// A CLI mostra agente em segundo plano pendente (só o Claude).
    pub em_segundo_plano: bool,
    /// Pasta `<conversa>/subagents` do transcript da sessão.
    pub pasta_de_subagentes: Option<PathBuf>,
}

/// Une sessões, subagentes e fluxos vivos. `execucoes` é o formato de `fluxos::listar()`.
pub fn reunir(sessoes: &[SessaoNoOmni], execucoes: &[Value], agora_ms: u64) -> Vec<ItemDeFundo> {
    let mut itens = Vec::new();
    for s in sessoes {
        if s.provider == "claude" && s.em_segundo_plano {
            itens.push(ItemDeFundo { tipo: "sessao".into(), titulo: s.titulo.clone(), atividade: "Esperando um agente em segundo plano terminar".into(),
                desde_ms: s.desde_ms, projeto_id: s.projeto_id.clone(), sessao: Some(s.conversa_id.clone()), execucao: None });
        }
        let subagentes = s.pasta_de_subagentes.as_deref().filter(|_| s.provider == "claude").map(|p| subagentes_ativos(p, agora_ms)).unwrap_or_default();
        for a in subagentes {
            itens.push(ItemDeFundo { tipo: "subagente".into(), titulo: a.nome,
                atividade: if a.ultima_acao.is_empty() { "Trabalhando".into() } else { a.ultima_acao },
                desde_ms: a.desde_ms, projeto_id: s.projeto_id.clone(), sessao: Some(s.conversa_id.clone()), execucao: None });
        }
    }
    for e in execucoes {
        if !matches!(e["status"].as_str(), Some("rodando" | "aguardando")) { continue; }
        itens.push(ItemDeFundo { tipo: "fluxo".into(), titulo: e["fluxo"].as_str().unwrap_or("Fluxo").to_string(),
            atividade: if e["status"] == "aguardando" { "Esperando a sua aprovação".into() } else { "Rodando".into() },
            desde_ms: e["criado_ms"].as_u64().unwrap_or(0), projeto_id: e["projeto_id"].as_str().unwrap_or("").to_string(),
            sessao: None, execucao: e["id"].as_str().map(str::to_string) });
    }
    itens.sort_by_key(|i| i.desde_ms);
    itens
}

fn pasta_de_subagentes(transcript: &str) -> PathBuf {
    let caminho = Path::new(transcript);
    caminho.with_extension("").join("subagents")
}

/// As sessões vivas do OMNI. Conversa que só existe no histórico da CLI não passa por aqui.
fn sessoes_do_omni(state: &EngineState) -> Vec<SessaoNoOmni> {
    let dir = state.state_file.parent().unwrap();
    omni_core::conversations::read_index(dir).into_iter().filter_map(|c| {
        let viva = mobile::linked_session(state, &c)?;
        let segmento = c.segments.last()?;
        let texto = viva.interaction.lock().ok().map(|i| i.parser.screen().contents()).unwrap_or_default();
        Some(SessaoNoOmni {
            titulo: omni_core::conversations::display_title(&c), projeto_id: c.project_id.clone(), conversa_id: c.id.clone(),
            provider: segmento.provider.clone(), desde_ms: segmento.started_at_ms,
            em_segundo_plano: segmento.provider == "claude" && interaction::agente_em_segundo_plano(&texto),
            pasta_de_subagentes: segmento.transcript_path.as_deref().map(pasta_de_subagentes),
        })
    }).collect()
}

pub fn coletar(state: &EngineState, projeto: Option<&str>) -> Vec<ItemDeFundo> {
    let itens = reunir(&sessoes_do_omni(state), &fluxos::listar(), now_ms());
    filtrar_por_projeto(itens, projeto)
}

#[derive(Deserialize)] struct Filtro { #[serde(default)] projeto: Option<String> }

async fn rota(State(state): State<Arc<EngineState>>, Query(filtro): Query<Filtro>) -> Json<Value> {
    let itens = tokio::task::spawn_blocking(move || coletar(&state, filtro.projeto.as_deref().filter(|p| !p.is_empty()))).await.unwrap_or_default();
    Json(json!({ "itens": itens }))
}

pub fn rotas() -> Router<Arc<EngineState>> { Router::new().route("/segundo-plano", get(rota)) }

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    fn sessao(conversa: &str, projeto: &str, pasta: Option<PathBuf>, em_fundo: bool) -> SessaoNoOmni {
        SessaoNoOmni { conversa_id: conversa.into(), titulo: format!("conversa {conversa}"), projeto_id: projeto.into(), provider: "claude".into(),
            desde_ms: 5, em_segundo_plano: em_fundo, pasta_de_subagentes: pasta }
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
        assert_eq!(so_a.iter().map(|i| i.tipo.as_str()).collect::<Vec<_>>().len(), 3);
        assert!(so_a.iter().all(|i| i.projeto_id == "pa"));
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
        assert!(reunir(&sessoes_do_omni(&state), &[], agora()).is_empty(), "o registro global de fluxos fica de fora: outros testes o usam");
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
}
