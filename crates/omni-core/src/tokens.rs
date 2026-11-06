//! Tokens por conta somados dos históricos locais (só leitura). Claude: `projects/**/*.jsonl`, uma
//! mensagem do assistente por `message.id`. Codex: o último `token_count` acumulado de cada rollout.
//! Sem fonte (Antigravity, Cursor), o resultado é "não disponível", nunca zero.
use serde::Serialize;
use std::{collections::HashMap, path::{Path, PathBuf}};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Contagem { pub entrada: u64, pub saida: u64, pub cache: u64 }

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TokensDaConta {
    pub hoje: Option<Contagem>,
    pub sete_dias: Option<Contagem>,
    pub motivo: Option<String>,
}

/// Um ponto lido do arquivo: `id` só no Claude (dedupe); no Codex `c` é o total acumulado até `ts_ms`.
#[derive(Debug, Clone)]
struct Registro { ts_ms: i64, id: Option<String>, c: Contagem }

/// Chaveado por caminho; vale enquanto mtime e tamanho não mudam.
#[derive(Default)]
pub struct CacheDeArquivos {
    arquivos: HashMap<PathBuf, (u64, u64, Vec<Registro>)>,
    /// Quantas vezes um arquivo foi de fato lido (para o teste de cache).
    pub leituras: u64,
}

const MOTIVO_SEM_LEITURA: &str = "Não foi possível ler os históricos desta conta";
const MS_DIA: i64 = 86_400_000;

/// Meia-noite local do dia de `ms`, `dias_atras` dias antes.
fn inicio_do_dia_local(ms: i64, dias_atras: i64) -> i64 {
    use chrono::{Local, TimeZone};
    let dia = chrono::DateTime::from_timestamp_millis(ms).unwrap_or_default().with_timezone(&Local).date_naive() - chrono::Duration::days(dias_atras);
    dia.and_hms_opt(0, 0, 0).and_then(|d| Local.from_local_datetime(&d).earliest()).map_or(ms - dias_atras * MS_DIA, |d| d.timestamp_millis())
}

fn numero(v: &serde_json::Value) -> u64 { v.as_u64().unwrap_or(0) }

fn epoch_ms(v: &serde_json::Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|d| d.timestamp_millis())
}

fn carimbo(caminho: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(caminho).ok()?;
    let mtime = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;
    Some((mtime, meta.len()))
}

/// Registros do arquivo, do cache quando mtime e tamanho não mudaram.
/// `None` quando o arquivo não abre (nada é guardado, a próxima chamada tenta de novo).
fn registros<'a>(cache: &'a mut CacheDeArquivos, caminho: &Path, ler: fn(&Path) -> Option<Vec<Registro>>) -> Option<&'a [Registro]> {
    let carimbo = carimbo(caminho).unwrap_or((0, 0));
    let valido = cache.arquivos.get(caminho).is_some_and(|(m, t, _)| (*m, *t) == carimbo);
    if !valido {
        cache.leituras += 1;
        let lidos = ler(caminho)?;
        cache.arquivos.insert(caminho.to_path_buf(), (carimbo.0, carimbo.1, lidos));
    }
    Some(&cache.arquivos[caminho].2)
}

fn ler_claude(caminho: &Path) -> Option<Vec<Registro>> {
    use std::io::BufRead;
    let arquivo = std::fs::File::open(caminho).ok()?;
    let mut saida = Vec::new();
    for linha in std::io::BufReader::new(arquivo).lines().map_while(Result::ok) {
        if !linha.contains("\"usage\"") { continue; }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&linha) else { continue };
        let uso = &v["message"]["usage"];
        let (Some(id), Some(ts_ms)) = (v["message"]["id"].as_str(), epoch_ms(&v["timestamp"])) else { continue };
        if !uso.is_object() { continue; }
        saida.push(Registro { ts_ms, id: Some(id.to_owned()), c: Contagem {
            entrada: numero(&uso["input_tokens"]), saida: numero(&uso["output_tokens"]),
            cache: numero(&uso["cache_creation_input_tokens"]) + numero(&uso["cache_read_input_tokens"]),
        } });
    }
    Some(saida)
}

fn ler_codex(caminho: &Path) -> Option<Vec<Registro>> {
    use std::io::BufRead;
    let arquivo = crate::abrir(caminho).ok()?;
    let mut saida = Vec::new();
    for linha in std::io::BufReader::new(arquivo).lines().map_while(Result::ok) {
        if !linha.contains("token_count") { continue; }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&linha) else { continue };
        if v["payload"]["type"].as_str() != Some("token_count") { continue; }
        let total = &v["payload"]["info"]["total_token_usage"];
        let Some(ts_ms) = epoch_ms(&v["timestamp"]) else { continue };
        if !total.is_object() { continue; }
        let cache = numero(&total["cached_input_tokens"]);
        saida.push(Registro { ts_ms, id: None, c: Contagem {
            entrada: numero(&total["input_tokens"]).saturating_sub(cache), saida: numero(&total["output_tokens"]), cache,
        } });
    }
    saida.sort_by_key(|r| r.ts_ms);
    Some(saida)
}

fn jsonl_recursivo(pasta: &Path, fundo: usize, achados: &mut Vec<PathBuf>) {
    if fundo > 6 { return; }
    let Ok(entradas) = std::fs::read_dir(pasta) else { return };
    for e in entradas.flatten() {
        let Ok(tipo) = e.file_type() else { continue };
        if tipo.is_dir() { jsonl_recursivo(&e.path(), fundo + 1, achados); }
        else if tipo.is_file() && e.path().extension().is_some_and(|x| x == "jsonl") { achados.push(e.path()); }
    }
}

fn soma(a: Contagem, b: Contagem) -> Contagem {
    Contagem { entrada: a.entrada + b.entrada, saida: a.saida + b.saida, cache: a.cache + b.cache }
}

fn diferenca(a: Contagem, b: Contagem) -> Contagem {
    Contagem { entrada: a.entrada.saturating_sub(b.entrada), saida: a.saida.saturating_sub(b.saida), cache: a.cache.saturating_sub(b.cache) }
}

/// Tokens do Claude: cada `message.id` conta uma vez (a CLI regrava a mesma resposta em várias linhas).
pub fn tokens_claude(pasta: &Path, agora_ms: i64, cache: &mut CacheDeArquivos) -> TokensDaConta {
    let raiz = pasta.join("projects");
    if !raiz.is_dir() { return tokens_indisponiveis("Esta conta ainda não tem histórico do Claude Code"); }
    let (ini_hoje, ini_7) = (inicio_do_dia_local(agora_ms, 0), inicio_do_dia_local(agora_ms, 6));
    let mut arquivos = Vec::new();
    jsonl_recursivo(&raiz, 0, &mut arquivos);
    let mut por_id: HashMap<String, Registro> = HashMap::new();
    let (mut tentados, mut lidos) = (0, 0);
    for caminho in arquivos {
        if carimbo(&caminho).is_some_and(|(m, _)| (m as i64) < ini_7) { continue; }
        tentados += 1;
        let Some(lista) = registros(cache, &caminho, ler_claude) else { continue };
        lidos += 1;
        for r in lista.iter().filter(|r| r.ts_ms >= ini_7 && r.ts_ms <= agora_ms) {
            let Some(id) = r.id.clone() else { continue };
            // Resposta regravada: vale a versão mais completa.
            if por_id.get(&id).is_none_or(|antigo| r.c.saida >= antigo.c.saida) { por_id.insert(id, r.clone()); }
        }
    }
    if tentados > 0 && lidos == 0 { return tokens_indisponiveis(MOTIVO_SEM_LEITURA); }
    let (mut hoje, mut sete) = (Contagem::default(), Contagem::default());
    for r in por_id.values() {
        sete = soma(sete, r.c);
        if r.ts_ms >= ini_hoje { hoje = soma(hoje, r.c); }
    }
    TokensDaConta { hoje: Some(hoje), sete_dias: Some(sete), motivo: None }
}

/// Tokens do Codex: o rollout guarda o total acumulado; o consumo da janela é o último total menos o
/// último total anterior ao início dela.
pub fn tokens_codex(pasta: &Path, agora_ms: i64, cache: &mut CacheDeArquivos) -> TokensDaConta {
    let raiz = pasta.join("sessions");
    if !raiz.is_dir() { return tokens_indisponiveis("Esta conta ainda não tem histórico do Codex"); }
    let (ini_hoje, ini_7) = (inicio_do_dia_local(agora_ms, 0), inicio_do_dia_local(agora_ms, 6));
    let (mut hoje, mut sete) = (Contagem::default(), Contagem::default());
    let (mut tentados, mut lidos) = (0, 0);
    for caminho in crate::rollouts(&raiz) {
        if carimbo(&caminho).is_some_and(|(m, _)| (m as i64) < ini_7) { continue; }
        tentados += 1;
        let Some(lista) = registros(cache, &caminho, ler_codex) else { continue };
        lidos += 1;
        let Some(fim) = lista.iter().rfind(|r| r.ts_ms <= agora_ms).map(|r| r.c) else { continue };
        let antes = |inicio: i64| lista.iter().rfind(|r| r.ts_ms < inicio).map(|r| r.c).unwrap_or_default();
        hoje = soma(hoje, diferenca(fim, antes(ini_hoje)));
        sete = soma(sete, diferenca(fim, antes(ini_7)));
    }
    if tentados > 0 && lidos == 0 { return tokens_indisponiveis(MOTIVO_SEM_LEITURA); }
    TokensDaConta { hoje: Some(hoje), sete_dias: Some(sete), motivo: None }
}

/// Provedor sem fonte local: "não disponível" com o motivo, nunca zero.
pub fn tokens_indisponiveis(motivo: &str) -> TokensDaConta {
    TokensDaConta { hoje: None, sete_dias: None, motivo: Some(motivo.to_owned()) }
}

#[cfg(test)]
mod testes {
    use super::*;
    use chrono::{Duration, Local, TimeZone};
    use std::fs;

    fn meia_noite_local_ms() -> i64 {
        let hoje = Local::now().date_naive().and_hms_opt(0, 0, 0).unwrap();
        Local.from_local_datetime(&hoje).earliest().unwrap().timestamp_millis()
    }
    fn iso(ms: i64) -> String { chrono::DateTime::from_timestamp_millis(ms).unwrap().to_rfc3339() }
    fn msg_claude(id: &str, ms: i64, ent: u64, sai: u64, cc: u64, cl: u64) -> String {
        format!(r#"{{"type":"assistant","timestamp":"{}","message":{{"id":"{id}","usage":{{"input_tokens":{ent},"output_tokens":{sai},"cache_creation_input_tokens":{cc},"cache_read_input_tokens":{cl}}}}}}}"#, iso(ms))
    }
    fn pasta_claude(linhas: &[String]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("projects/proj");
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("a.jsonl"), linhas.join("\n")).unwrap();
        d
    }
    fn evento_codex(ms: i64, ent: u64, cache: u64, sai: u64) -> String {
        format!(r#"{{"timestamp":"{}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{ent},"cached_input_tokens":{cache},"output_tokens":{sai},"total_tokens":{}}}}}}}}}"#, iso(ms), ent + sai)
    }

    #[test]
    fn soma_entrada_saida_e_cache() {
        let agora = meia_noite_local_ms() + 3_600_000;
        let d = pasta_claude(&[msg_claude("m1", agora - 1000, 10, 20, 100, 1000), msg_claude("m2", agora - 500, 1, 2, 3, 4)]);
        let r = tokens_claude(d.path(), agora, &mut CacheDeArquivos::default());
        assert_eq!(r.hoje, Some(Contagem { entrada: 11, saida: 22, cache: 1107 }));
        assert_eq!(r.sete_dias, r.hoje);
        assert_eq!(r.motivo, None);
    }

    #[test]
    fn mesma_mensagem_repetida_conta_uma_vez() {
        let agora = meia_noite_local_ms() + 3_600_000;
        let m = msg_claude("m1", agora - 1000, 10, 20, 0, 0);
        let d = pasta_claude(&[m.clone(), m]);
        let r = tokens_claude(d.path(), agora, &mut CacheDeArquivos::default());
        assert_eq!(r.hoje, Some(Contagem { entrada: 10, saida: 20, cache: 0 }));
    }

    #[test]
    fn hoje_usa_o_fuso_local() {
        let meia = meia_noite_local_ms();
        let agora = meia + 3_600_000;
        let d = pasta_claude(&[msg_claude("ontem", meia - 60_000, 5, 5, 0, 0), msg_claude("hoje", meia + 60_000, 1, 1, 0, 0),
            msg_claude("velha", meia - Duration::days(9).num_milliseconds(), 100, 100, 0, 0)]);
        let r = tokens_claude(d.path(), agora, &mut CacheDeArquivos::default());
        assert_eq!(r.hoje, Some(Contagem { entrada: 1, saida: 1, cache: 0 }));
        assert_eq!(r.sete_dias, Some(Contagem { entrada: 6, saida: 6, cache: 0 }));
    }

    #[test]
    fn mesmo_id_em_dois_arquivos_conta_uma_vez() {
        let agora = meia_noite_local_ms() + 3_600_000;
        let d = pasta_claude(&[msg_claude("m1", agora - 1000, 10, 20, 0, 0)]);
        let outro = d.path().join("projects/outro");
        fs::create_dir_all(&outro).unwrap();
        fs::write(outro.join("b.jsonl"), msg_claude("m1", agora - 1000, 10, 20, 0, 0)).unwrap();
        let r = tokens_claude(d.path(), agora, &mut CacheDeArquivos::default());
        assert_eq!(r.hoje, Some(Contagem { entrada: 10, saida: 20, cache: 0 }));
    }

    #[cfg(unix)]
    #[test]
    fn arquivos_ilegiveis_viram_nao_disponivel_e_nao_zero() {
        use std::os::unix::fs::PermissionsExt;
        let agora = meia_noite_local_ms() + 3_600_000;
        let d = pasta_claude(&[msg_claude("m1", agora - 1000, 10, 20, 0, 0)]);
        let arq = d.path().join("projects/proj/a.jsonl");
        fs::set_permissions(&arq, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::File::open(&arq).is_ok() { return; } // como root a permissão não vale
        let r = tokens_claude(d.path(), agora, &mut CacheDeArquivos::default());
        assert_eq!((r.hoje, r.sete_dias), (None, None));
        assert!(r.motivo.is_some());
    }

    #[test]
    fn linha_corrompida_e_ignorada() {
        let agora = meia_noite_local_ms() + 3_600_000;
        let d = pasta_claude(&["{quebrada".into(), msg_claude("m1", agora - 1000, 3, 4, 0, 0), "".into()]);
        let r = tokens_claude(d.path(), agora, &mut CacheDeArquivos::default());
        assert_eq!(r.hoje, Some(Contagem { entrada: 3, saida: 4, cache: 0 }));
    }

    #[test]
    fn arquivo_sem_mudanca_nao_e_relido() {
        let agora = meia_noite_local_ms() + 3_600_000;
        let d = pasta_claude(&[msg_claude("m1", agora - 1000, 3, 4, 0, 0)]);
        let mut cache = CacheDeArquivos::default();
        let a = tokens_claude(d.path(), agora, &mut cache);
        assert_eq!(cache.leituras, 1);
        let b = tokens_claude(d.path(), agora, &mut cache);
        assert_eq!(cache.leituras, 1, "mesmo mtime e tamanho: nada relido");
        assert_eq!(a, b);
        fs::write(d.path().join("projects/proj/a.jsonl"), msg_claude("m9", agora - 10, 50, 0, 0, 0)).unwrap();
        let c = tokens_claude(d.path(), agora, &mut cache);
        assert_eq!(cache.leituras, 2);
        assert_eq!(c.hoje, Some(Contagem { entrada: 50, saida: 0, cache: 0 }));
    }

    /// Formato (amostra sintética): `event_msg`/`token_count` com `info.total_token_usage` acumulado;
    /// `input_tokens` já inclui o cacheado. Vale o último total, não a soma dos eventos.
    #[test]
    fn codex_usa_o_ultimo_total_do_rollout() {
        let meia = meia_noite_local_ms();
        let agora = meia + 3_600_000;
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("sessions/2026/11/06");
        fs::create_dir_all(&dir).unwrap();
        // Começou ontem: 1000 de entrada (200 em cache) e 50 de saída até ontem; hoje chega a 1500/300/80.
        let linhas = [evento_codex(meia - 60_000, 1000, 200, 50), evento_codex(meia + 1000, 1200, 250, 60), evento_codex(meia + 2000, 1500, 300, 80)];
        fs::write(dir.join("rollout-a.jsonl"), linhas.join("\n")).unwrap();
        let r = tokens_codex(d.path(), agora, &mut CacheDeArquivos::default());
        assert_eq!(r.sete_dias, Some(Contagem { entrada: 1200, saida: 80, cache: 300 }));
        assert_eq!(r.hoje, Some(Contagem { entrada: 400, saida: 30, cache: 100 }));
    }

    #[test]
    fn antigravity_fica_nao_disponivel() {
        let r = tokens_indisponiveis("O Antigravity não guarda contagem de tokens localmente");
        assert_eq!(r.hoje, None);
        assert_eq!(r.sete_dias, None);
        assert!(r.motivo.unwrap().contains("Antigravity"));
    }
}
