//! `GET /contas/{id}/tokens`: tokens de hoje e dos últimos 7 dias somados dos históricos locais da
//! conta. Só leitura; o cache por arquivo vive no processo para não reler tudo a cada abertura.
use super::*;
use axum::{extract::{Path as RoutePath, State}, http::StatusCode, routing::get, Json, Router};
use omni_core::tokens::{tokens_claude, tokens_codex, tokens_indisponiveis, CacheDeArquivos, TokensDaConta};
use serde_json::{json, Value};
use std::{path::Path, sync::{Arc, Mutex, OnceLock}};

pub fn rotas() -> Router<Arc<EngineState>> {
    Router::new().route("/contas/{id}/tokens", get(tokens_da_rota))
}

fn cache() -> &'static Mutex<CacheDeArquivos> {
    static CACHE: OnceLock<Mutex<CacheDeArquivos>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(CacheDeArquivos::default()))
}

/// `None` quando a conta não existe.
pub fn tokens_da_conta(dir: &Path, id: &str, agora_ms: i64, cache: &mut CacheDeArquivos) -> Option<TokensDaConta> {
    let perfil = omni_core::profiles(dir).into_iter().find(|p| p.id == id)?;
    let pasta = Path::new(&perfil.config_dir);
    Some(match perfil.provider.as_str() {
        "claude" => tokens_claude(pasta, agora_ms, cache),
        "codex" => tokens_codex(pasta, agora_ms, cache),
        "antigravity" => tokens_indisponiveis("O Antigravity não guarda a contagem de tokens nos arquivos locais"),
        "cursor" => tokens_indisponiveis("O Cursor não guarda a contagem de tokens nos arquivos locais"),
        _ => tokens_indisponiveis("Este agente não informa tokens"),
    })
}

async fn tokens_da_rota(State(state): State<Arc<EngineState>>, RoutePath(id): RoutePath<String>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tokio::task::spawn_blocking(move || {
        let dir = state.state_file.parent().unwrap().to_path_buf();
        let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
        tokens_da_conta(&dir, &id, now_ms() as i64, &mut cache)
            .map(|tokens| Json(json!(tokens)))
            .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "Conta não encontrada"}))))
    }).await.map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Falha ao somar os tokens"}))))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_com_contas(pasta_claude: &Path) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let perfis = json!({"profiles": [
            {"id": "c1", "provider": "claude", "name": "Pessoal", "config_dir": pasta_claude, "created_at_ms": 1},
            {"id": "a1", "provider": "antigravity", "name": "Antigravity", "config_dir": d.path(), "created_at_ms": 1},
        ]});
        std::fs::write(d.path().join("profiles.json"), perfis.to_string()).unwrap();
        d
    }

    #[test]
    fn conta_desconhecida_nao_existe() {
        let d = dir_com_contas(Path::new("/nao/existe"));
        assert!(tokens_da_conta(d.path(), "zzz", 0, &mut CacheDeArquivos::default()).is_none());
    }

    #[test]
    fn antigravity_responde_nao_disponivel_e_claude_sem_historico_tambem() {
        let d = dir_com_contas(Path::new("/nao/existe"));
        let mut cache = CacheDeArquivos::default();
        let a = tokens_da_conta(d.path(), "a1", 0, &mut cache).unwrap();
        assert!(a.hoje.is_none() && a.sete_dias.is_none() && a.motivo.is_some());
        let c = tokens_da_conta(d.path(), "c1", 0, &mut cache).unwrap();
        assert!(c.hoje.is_none(), "sem pasta de histórico não é zero: é não disponível");
    }

    #[test]
    fn claude_soma_o_historico_da_pasta_da_conta() {
        let conta = tempfile::tempdir().unwrap();
        let agora = 1_780_000_000_000_i64;
        let projeto = conta.path().join("projects/p");
        std::fs::create_dir_all(&projeto).unwrap();
        let ts = "2026-05-28T20:26:39+00:00";
        std::fs::write(projeto.join("s.jsonl"), format!(r#"{{"timestamp":"{ts}","message":{{"id":"m","usage":{{"input_tokens":7,"output_tokens":9}}}}}}"#)).unwrap();
        let d = dir_com_contas(conta.path());
        let r = tokens_da_conta(d.path(), "c1", agora, &mut CacheDeArquivos::default()).unwrap();
        assert_eq!(r.sete_dias.map(|c| (c.entrada, c.saida)), Some((7, 9)));
    }
}
