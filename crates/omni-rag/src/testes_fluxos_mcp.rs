//! Ensaio, espera e os limites novos das ferramentas de fluxos, contra um engine falso.
use crate::{engine::Contexto, mcp_fluxos};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use tempfile::{tempdir, TempDir};

/// Cada GET devolve o próximo corpo da lista e repete o último. `/projetos` aponta para `projeto`.
fn engine_em_sequencia(projeto: &Path, corpos: Vec<Value>) -> (u16, Arc<Mutex<Vec<String>>>) {
    let ouvinte = TcpListener::bind("127.0.0.1:0").expect("porta");
    let porta = ouvinte.local_addr().expect("endereço").port();
    let pedidos: Arc<Mutex<Vec<String>>> = Arc::default();
    let guardados = pedidos.clone();
    let projetos = json!({"projects": [{"id": "p", "name": "Proj", "path": projeto}]});
    thread::spawn(move || {
        let mut restantes = corpos.into_iter().peekable();
        for conexao in ouvinte.incoming().flatten() {
            let mut linha = String::new();
            if BufReader::new(&conexao).read_line(&mut linha).is_err() { continue; }
            let pedido: Value = serde_json::from_str(&linha).expect("json");
            let caminho = pedido["path"].as_str().unwrap_or_default().to_owned();
            guardados.lock().expect("pedidos").push(format!("{} {caminho}", pedido["method"].as_str().unwrap_or_default()));
            let corpo = if caminho == "/projetos" { projetos.clone() } else {
                let atual = restantes.next().unwrap_or(json!({}));
                if restantes.peek().is_none() { restantes = vec![atual.clone()].into_iter().peekable(); }
                atual
            };
            let mut saida = &conexao;
            let _ = writeln!(saida, "{}", json!({"type": "api", "status": 200, "body": corpo.to_string()}));
        }
    });
    (porta, pedidos)
}

struct Ambiente { pasta: TempDir, contexto: Contexto }

fn ambiente(corpos: Vec<Value>) -> (Ambiente, Arc<Mutex<Vec<String>>>) {
    let pasta = tempdir().expect("pasta");
    let raiz = pasta.path().join("casa/usuario");
    let projeto = raiz.join("trabalho/projeto");
    fs::create_dir_all(raiz.join("engine")).expect("engine");
    fs::write(raiz.join("engine/engine.token"), "segredo\n").expect("token");
    fs::create_dir_all(&projeto).expect("projeto");
    let (porta, pedidos) = engine_em_sequencia(&projeto, corpos);
    let contexto = Contexto { pasta_do_engine: raiz.join("engine"), porta, fluxos_globais: Some(raiz.join(".omni/fluxos")),
        agentes_globais: Some(raiz.join(".omni/agents")), pasta_atual: Some(projeto) };
    (Ambiente { pasta, contexto }, pedidos)
}

fn chamar(local: &Ambiente, nome: &str, argumentos: Value) -> Result<String, String> {
    mcp_fluxos::chamar(&local.contexto, nome, &argumentos).expect("ferramenta de fluxos")
}

fn agente(nome: &str, cli: &str, ferramentas: Value) -> Value {
    json!({"name": nome, "description": "d", "cli": cli, "instrucoes": "x", "ferramentas": ferramentas})
}

/// entrada → agente → fim, com o que o teste quiser por cima.
fn fluxo_de_agente(id: &str, nome_do_agente: &str, no: Value, fluxo: Value) -> Value {
    let mut etapa = json!({"id": "a", "tipo": "agente", "titulo": "A", "agente": nome_do_agente});
    for (chave, valor) in no.as_object().cloned().unwrap_or_default() { etapa[chave] = valor; }
    let mut base = json!({"id": id, "nome": "N", "descricao": "d", "nos": [{"id": "e", "tipo": "entrada"}, etapa, {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "a"}, {"de": "a", "para": "f"}]});
    for (chave, valor) in fluxo.as_object().cloned().unwrap_or_default() { base[chave] = valor; }
    base
}

fn criar(local: &Ambiente, fluxo: Value, agentes_novos: Value) -> Result<String, String> {
    chamar(local, "criar_fluxo", json!({"fluxo": fluxo, "agentes_novos": agentes_novos}))
}

#[test]
fn pasta_extra_com_agente_que_grava_precisa_de_aprovacao() {
    let (local, _) = ambiente(vec![]);
    let leitor = agente("leitor", "claude", json!(["Read"]));
    let erro = criar(&local, fluxo_de_agente("com-extra", "leitor", json!({}), json!({"pastas_extras": ["../irmao"]})), json!([leitor])).expect_err("sem aprovação");
    assert!(erro.contains("aprovação"), "{erro}");
    assert!(!local.contexto.fluxos_globais.as_ref().expect("pasta").join("agente-com-extra.json").exists());
}

#[test]
fn pasta_extra_com_etapa_de_escrita_limitada_nao_exige_aprovacao() {
    let (local, _) = ambiente(vec![]);
    let leitor = agente("leitor", "claude", json!(["Read"]));
    criar(&local, fluxo_de_agente("so-le", "leitor", json!({"escrita": []}), json!({"pastas_extras": ["../irmao"]})), json!([leitor])).expect("só lê");
}

/// Proibir o Bash não tira o risco: a regra olha o modelo de agente, não a promessa da etapa.
#[test]
fn proibidas_e_escrita_nao_liberam_agente_com_bash_antes_da_aprovacao() {
    let (local, _) = ambiente(vec![]);
    let dev = agente("com-shell", "claude", json!(["Bash"]));
    let erro = criar(&local, fluxo_de_agente("shell-sem-ok", "com-shell", json!({"proibidas": ["Bash"], "escrita": []}), json!({})), json!([dev])).expect_err("sem aprovação");
    assert!(erro.contains("aprovação"), "{erro}");
}

#[test]
fn criar_fluxo_recusa_caminho_que_alarga_e_marcador_sem_etapa() {
    let (local, _) = ambiente(vec![]);
    let leitor = || json!([agente("leitor", "claude", json!(["Read"]))]);
    assert!(criar(&local, fluxo_de_agente("a1", "leitor", json!({"escrita": ["../fora"]}), json!({})), leitor()).expect_err("..").contains("escrita"));
    assert!(criar(&local, fluxo_de_agente("a2", "leitor", json!({"escrita": []}), json!({"pastas_extras": ["/"]})), leitor()).expect_err("/").contains("raiz"));
    assert!(criar(&local, fluxo_de_agente("a3", "leitor", json!({"config": {"prompt": "{{no:nada}}"}}), json!({})), leitor()).expect_err("marcador").contains("nada"));
}

#[test]
fn ensaiar_aponta_o_que_falta_sem_abrir_agente() {
    let (local, pedidos) = ambiente(vec![]);
    gravar_fluxo(&local, &fluxo_de_agente("agente-falho", "fantasma", json!({"config": {"prompt": "{{no:f}}"}}), json!({"pastas_extras": ["../nao-existe"]})));
    let texto = chamar(&local, "ensaiar_fluxo", json!({"id": "agente-falho"})).expect("ensaio");
    for trecho in ["fantasma", "não roda antes", "não existe neste PC", "rodar_fluxo vai recusar", "Nenhum agente foi aberto", "Agentes: 1"] {
        assert!(texto.contains(trecho), "{trecho}: {texto}");
    }
    assert!(!texto.contains("pronto para rodar"), "{texto}");
    assert!(pedidos.lock().expect("pedidos").iter().all(|pedido| pedido.starts_with("GET")), "ensaio não dispara nada");
}

/// Direto no arquivo: `agentes::salvar` só aceita as CLIs reais, e o teste não pode depender delas.
fn gravar_agente(local: &Ambiente, nome: &str, cli: &str) {
    let pasta = local.contexto.agentes_globais.clone().expect("agentes");
    let _ = omni_core::agentes::listar(Some(&pasta), None);
    fs::create_dir_all(&pasta).expect("agentes");
    fs::write(pasta.join(format!("{nome}.md")), format!("---\nname: {nome}\ndescription: d\ncli: {cli}\nferramentas: [Read]\nsaida: texto\n---\nx\n")).expect("agente");
}

fn gravar_fluxo(local: &Ambiente, fluxo: &Value) {
    let pasta = local.contexto.fluxos_globais.clone().expect("fluxos");
    fs::create_dir_all(&pasta).expect("fluxos");
    fs::write(pasta.join(format!("{}.json", fluxo["id"].as_str().expect("id"))), fluxo.to_string()).expect("grava");
}

#[test]
fn ensaiar_fluxo_certo_fica_pronto_e_conta() {
    let (local, _) = ambiente(vec![]);
    let projeto = local.contexto.pasta_atual.clone().expect("projeto");
    fs::create_dir_all(projeto.parent().expect("pai").join("irmao")).expect("irmao");
    // `sh` existe em qualquer máquina de teste: a conferência da CLI é real, sem depender do claude.
    gravar_agente(&local, "leitor", "sh");
    gravar_fluxo(&local, &fluxo_de_agente("meu-fluxo", "leitor", json!({"escrita": []}), json!({"pastas_extras": ["../irmao"]})));
    let texto = chamar(&local, "ensaiar_fluxo", json!({"id": "meu-fluxo"})).expect("ensaio");
    assert!(texto.contains("Agentes: 1") && texto.contains("aprovações: 0"), "{texto}");
    assert!(texto.contains("pronto para rodar") && !texto.contains("Problemas"), "{texto}");
}

#[test]
fn ensaiar_acusa_cli_que_nao_esta_no_path() {
    let (local, _) = ambiente(vec![]);
    gravar_agente(&local, "sumido", "cli-que-nao-existe");
    gravar_fluxo(&local, &fluxo_de_agente("cli", "sumido", json!({}), json!({})));
    let texto = chamar(&local, "ensaiar_fluxo", json!({"id": "cli"})).expect("ensaio");
    assert!(texto.contains("“cli-que-nao-existe” não foi encontrada"), "{texto}");
}

#[test]
fn esperar_volta_assim_que_pede_aprovacao() {
    let rodando = json!({"id": "x", "status": "rodando", "fluxo": {"nos": []}, "nos": {}, "custo_usd": 0.1});
    let aguardando = json!({"id": "x", "status": "aguardando", "fluxo": {"nos": []}, "nos": {}, "custo_usd": 0.1});
    let (local, pedidos) = ambiente(vec![rodando, aguardando]);
    let inicio = Instant::now();
    let texto = chamar(&local, "esperar_execucao", json!({"id": "x", "segundos": 30})).expect("espera");
    assert!(inicio.elapsed() < Duration::from_secs(10), "voltou na segunda consulta");
    assert!(texto.contains("aguardando") && texto.contains("Você não pode aprovar"), "{texto}");
    assert_eq!(pedidos.lock().expect("pedidos").len(), 2, "uma consulta por ritmo, sem girar");
}

#[test]
fn esperar_nao_espera_o_que_ja_parou() {
    for status in ["concluido", "falhou", "pausado", "cancelado", "aguardando"] {
        let (local, pedidos) = ambiente(vec![json!({"id": "x", "status": status, "fluxo": {"nos": []}, "nos": {}})]);
        let texto = chamar(&local, "esperar_execucao", json!({"id": "x", "segundos": 300})).expect("espera");
        assert!(texto.contains(status), "{texto}");
        assert_eq!(pedidos.lock().expect("pedidos").len(), 1);
    }
}

#[test]
fn esperar_devolve_o_andamento_quando_o_tempo_acaba() {
    let (local, _) = ambiente(vec![json!({"id": "x", "status": "rodando", "fluxo": {"nos": []}, "nos": {}})]);
    let texto = chamar(&local, "esperar_execucao", json!({"id": "x", "segundos": 1})).expect("espera");
    assert!(texto.contains("Ainda rodando depois de 1 s"), "{texto}");
}

#[test]
fn esperar_recusa_id_com_caminho() {
    let (local, pedidos) = ambiente(vec![]);
    assert!(chamar(&local, "esperar_execucao", json!({"id": "../x"})).is_err());
    assert!(pedidos.lock().expect("pedidos").is_empty());
}

#[test]
fn limite_de_custo_so_o_usuario_retoma() {
    let (local, pedidos) = ambiente(vec![json!({"id": "x", "status": "pausado", "parou_por_custo": true})]);
    let erro = chamar(&local, "controlar_execucao", json!({"id": "x", "acao": "retomar"})).expect_err("recusa");
    assert!(erro.contains("limite de custo"), "{erro}");
    assert!(!pedidos.lock().expect("pedidos").iter().any(|pedido| pedido.starts_with("POST")));
}

#[test]
fn pausa_comum_o_mcp_retoma() {
    let (local, pedidos) = ambiente(vec![json!({"id": "x", "status": "pausado", "parou_por_custo": false})]);
    chamar(&local, "controlar_execucao", json!({"id": "x", "acao": "retomar"})).expect("retoma");
    assert!(pedidos.lock().expect("pedidos").iter().any(|pedido| pedido == "POST /fluxos/execucoes/x/retomar"));
}

#[test]
fn rodar_fluxo_manda_usar_a_espera_e_nenhuma_ferramenta_nova_aprova() {
    let ferramentas = mcp_fluxos::ferramentas();
    let rodar = ferramentas.iter().find(|f| f["name"] == "rodar_fluxo").expect("rodar");
    assert!(rodar["description"].as_str().expect("texto").contains("esperar_execucao"));
    for nome in ["esperar_execucao", "ensaiar_fluxo"] { assert!(ferramentas.iter().any(|f| f["name"] == nome), "{nome}"); }
    assert!(!ferramentas.iter().any(|f| f["name"].as_str().is_some_and(|nome| nome.contains("aprov"))));
}

#[cfg(unix)]
#[test]
fn ensaio_fala_com_o_mcp_de_verdade_pelo_registro() {
    use std::os::unix::fs::PermissionsExt;
    let (local, _) = ambiente(vec![]);
    let home = local.pasta.path().join("casa/usuario");
    let script = home.join("mcp-falso.sh");
    fs::write(&script, "#!/bin/sh\nread linha\necho '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"serverInfo\":{\"name\":\"omni-rag\"}}}'\n").expect("script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("permissão");
    let registrar = |comando: &Path| fs::write(home.join(".claude.json"), json!({"mcpServers": {"omni-rag": {"command": comando, "args": []}}}).to_string()).expect("registro");
    gravar_fluxo(&local, &json!({"id": "vazio", "nome": "V", "nos": [{"id": "e", "tipo": "entrada"}, {"id": "f", "tipo": "fim"}], "conexoes": [{"de": "e", "para": "f"}]}));

    registrar(&script);
    assert!(!chamar(&local, "ensaiar_fluxo", json!({"id": "vazio"})).expect("ensaio").contains("MCP omni-rag"));
    registrar(Path::new("/caminho/que/nao/existe"));
    assert!(chamar(&local, "ensaiar_fluxo", json!({"id": "vazio"})).expect("ensaio").contains("MCP omni-rag: não abriu"));
}
