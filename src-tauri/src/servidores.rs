//! Servidores locais de projeto (`npm run dev`, Vite, Next, Django…) rodando no PC, com o botão de
//! derrubar. Do netstat inteiro sobra só o que é processo de dev (node, python…) rodando um script
//! de dentro de uma pasta de projeto: sistema, apps instalados (Spotify, Docker, Discord) e
//! ferramentas globais (`AppData`, `Program Files`, extensões do VS Code) ficam de fora.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;

use crate::servidores_leitura::Escuta;

/// Uma leitura: portas em escuta com o dono e a tabela de processos. Um PowerShell só (~1 s).
#[cfg(windows)]
const LEITURA: &str = "[Console]::OutputEncoding=[Text.Encoding]::UTF8;\
$c=@(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | % { @{p=[int]$_.LocalPort;pid=[int]$_.OwningProcess} });\
$p=@(Get-CimInstance Win32_Process | % { @{pid=[int]$_.ProcessId;nome=$_.Name;cmd=$_.CommandLine} });\
@{c=$c;p=$p} | ConvertTo-Json -Compress -Depth 3";

const PROGRAMAS_DE_DEV: &[&str] = &["node", "bun", "deno", "python", "pythonw", "php", "ruby", "java", "dotnet", "go", "cargo", "uvicorn", "next-server"];
/// Caminho de programa instalado ou ferramenta global, não de projeto (separador já normalizado
/// para `\\`, minúsculo). Vale por trecho do comando: o `node.exe` mora em `Program Files`, mas o
/// script que ele roda está na pasta do projeto.
const FORA_DE_PROJETO: &[&str] = &[
    // Windows
    "\\program files", "\\appdata\\", "\\.vscode", "npm-cache", "\\_npx", "\\windows\\",
    // Linux e macOS
    "\\usr\\", "\\opt\\", "\\snap\\", "\\.nvm\\", "\\.cargo\\", "\\.rustup\\", "\\.local\\",
    "\\.npm\\", "\\.cache\\", "\\.config\\", "\\.volta\\", "\\.asdf\\", "\\.pyenv\\", "\\library\\", "\\applications\\",
];
const MARCAS_DE_PROJETO: &[&str] = &["package.json", "pyproject.toml", "requirements.txt", "manage.py", "Cargo.toml", "go.mod", "composer.json", "Gemfile", "deno.json"];

#[derive(Deserialize)]
struct Processo { pid: u32, nome: Option<String>, cmd: Option<String>, #[serde(default)] pasta: Option<String> }
#[derive(Deserialize)]
struct Leitura { c: Vec<Escuta>, p: Vec<Processo> }

/// Um projeto com servidor de pé. Agrupado por pasta: o Next abre duas portas, o Vite às vezes também.
#[derive(Debug, Serialize, PartialEq)]
pub struct Servidor {
    pub pasta: String,
    pub nome: String,
    pub tipo: String,
    pub portas: Vec<u16>,
    pub pids: Vec<u32>,
    pub comando: String,
    /// É o próprio OMNI rodando em `npm run dev`: derrubar fecharia esta janela.
    pub proprio: bool,
}

/// Caminho absoluto de Windows (`C:\\x`, `C:/x`) ou de Unix (`/x`). O resto (`node`, `--inspect`,
/// `server.js`) não é caminho e não diz onde o projeto está.
fn caminho_absoluto(trecho: &str) -> Option<(String, bool)> {
    let b = trecho.as_bytes();
    if b.len() >= 4 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/') {
        return Some((trecho.replace('/', "\\"), true));
    }
    (b.len() >= 2 && b[0] == b'/').then(|| (trecho.to_owned(), false))
}

fn fora_de_projeto(caminho: &str) -> bool {
    let normalizado = format!("{}\\", caminho.to_lowercase().replace('/', "\\"));
    FORA_DE_PROJETO.iter().any(|marca| normalizado.contains(marca))
}

/// Sobe de `base` até achar `package.json` (ou parecido), no máximo 6 níveis. Corta o texto no
/// separador do próprio caminho, para valer igual no Windows e no Unix sem depender do SO de quem roda.
fn subir_ate_marca(base: &str, separador: char, existe: &impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut atual = base.trim_end_matches(separador);
    for _ in 0..6 {
        if atual.is_empty() { return None; }
        if MARCAS_DE_PROJETO.iter().any(|m| existe(&Path::new(atual).join(m))) { return Some(PathBuf::from(atual)); }
        atual = &atual[..atual.rfind(separador)?];
    }
    None
}

/// Pasta do projeto a partir da linha de comando: o primeiro caminho absoluto fora de programa
/// instalado, cortado antes de `node_modules`, subindo até achar `package.json` (ou parecido).
/// Comando sem caminho (`node server.js`) cai na pasta onde o processo roda, quando se sabe.
fn projeto_do_comando(cmd: &str, pasta_do_processo: Option<&str>, existe: &impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut trechos = Vec::new();
    for (i, parte) in cmd.split('"').enumerate() {
        if i % 2 == 1 { trechos.push(parte.to_owned()); } else { trechos.extend(parte.split_whitespace().map(str::to_owned)); }
    }
    let do_caminho = trechos.iter().find_map(|trecho| {
        let (caminho, windows) = caminho_absoluto(trecho.rsplit('=').next()?)?;
        if fora_de_projeto(&caminho) { return None; }
        let separador = if windows { '\\' } else { '/' };
        let corte = format!("{separador}node_modules{separador}");
        let base = caminho.to_lowercase().find(&corte).map_or(caminho.as_str(), |i| &caminho[..i]);
        subir_ate_marca(base, separador, existe)
    });
    do_caminho.or_else(|| {
        let (pasta, windows) = caminho_absoluto(pasta_do_processo?)?;
        if fora_de_projeto(&pasta) { return None; }
        subir_ate_marca(&pasta, if windows { '\\' } else { '/' }, existe)
    })
}

fn tipo(cmd: &str, programa: &str) -> String {
    let cmd = cmd.to_lowercase().replace('/', "\\");
    let conhecidos = [("storybook", "Storybook"), ("\\vite\\", "Vite"), ("\\.bin\\vite", "Vite"), ("\\next\\", "Next"),
        ("\\.bin\\next", "Next"), ("next@", "Next"), ("next-server", "Next"), ("\\nuxt", "Nuxt"),
        ("\\astro\\", "Astro"), ("webpack", "Webpack"), ("react-scripts", "Create React App"), ("manage.py", "Django"),
        ("uvicorn", "Uvicorn"), ("flask", "Flask")];
    conhecidos.iter().find(|(marca, _)| cmd.contains(marca)).map_or_else(|| programa.to_owned(), |(_, nome)| (*nome).to_owned())
}

/// `node.exe` e `node` são o mesmo programa; `python3.12` conta como `python`.
fn programa(nome: &str) -> String {
    let minusculo = nome.to_lowercase();
    let sem_exe = minusculo.trim_end_matches(".exe");
    match sem_exe.strip_prefix("python") {
        Some(resto) if resto.chars().all(|c| c.is_ascii_digit() || c == '.') => "python".to_owned(),
        _ => sem_exe.to_owned(),
    }
}

/// O executável do OMNI está dentro da pasta do projeto? Texto com separador normalizado, para
/// valer igual nos dois sistemas (o Windows ainda ignora maiúsculas).
fn contem_o_executavel(pasta: &str, executavel: &str) -> bool {
    let norma = |t: &str| {
        let t = t.replace('\\', "/");
        if t.as_bytes().get(1) == Some(&b':') { t.to_lowercase() } else { t }
    };
    let (pasta, exe) = (norma(pasta), norma(executavel));
    exe == pasta || exe.starts_with(&format!("{}/", pasta.trim_end_matches('/')))
}

fn nome_da_pasta(pasta: &str) -> String {
    pasta.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().filter(|n| !n.is_empty()).unwrap_or(pasta).to_owned()
}

fn classificar(leitura: Leitura, existe: impl Fn(&Path) -> bool, onde_estou: Option<&Path>) -> Vec<Servidor> {
    let processos: BTreeMap<u32, Processo> = leitura.p.into_iter().map(|p| (p.pid, p)).collect();
    let mut grupos: BTreeMap<PathBuf, Servidor> = BTreeMap::new();
    for escuta in leitura.c.iter().filter(|e| e.p > 1024) {
        let Some(processo) = processos.get(&escuta.pid) else { continue };
        let programa = programa(processo.nome.as_deref().unwrap_or_default());
        let cmd = processo.cmd.as_deref().unwrap_or_default();
        if !PROGRAMAS_DE_DEV.contains(&programa.as_str()) { continue; }
        let Some(pasta) = projeto_do_comando(cmd, processo.pasta.as_deref(), &existe) else { continue };
        let servidor = grupos.entry(pasta.clone()).or_insert_with(|| Servidor {
            pasta: pasta.display().to_string(),
            nome: nome_da_pasta(&pasta.display().to_string()),
            tipo: tipo(cmd, &programa),
            portas: vec![],
            pids: vec![],
            comando: cmd.to_owned(),
            proprio: onde_estou.is_some_and(|eu| contem_o_executavel(&pasta.display().to_string(), &eu.to_string_lossy())),
        });
        if !servidor.portas.contains(&escuta.p) { servidor.portas.push(escuta.p); }
        if !servidor.pids.contains(&escuta.pid) { servidor.pids.push(escuta.pid); }
    }
    let mut lista: Vec<Servidor> = grupos.into_values().collect();
    for servidor in &mut lista { servidor.portas.sort_unstable(); }
    lista.sort_by_key(|s| s.portas[0]);
    lista
}

#[cfg(windows)]
fn coletar() -> Result<Leitura, String> {
    let saida = crate::git_client::hidden(&mut Command::new("powershell"))
        .args(["-NoProfile", "-NonInteractive", "-Command", LEITURA])
        .output()
        .map_err(|erro| format!("PowerShell indisponível: {erro}"))?;
    if !saida.status.success() { return Err(String::from_utf8_lossy(&saida.stderr).trim().to_owned()); }
    let texto = String::from_utf8_lossy(&saida.stdout);
    serde_json::from_str(texto.trim_start_matches('\u{feff}').trim()).map_err(|erro| format!("Leitura das portas falhou: {erro}"))
}

#[cfg(unix)]
fn coletar() -> Result<Leitura, String> {
    let c = crate::servidores_leitura::escutas().map_err(|motivo| format!("Não deu para ler as portas: {motivo}"))?;
    let mut pids: Vec<u32> = c.iter().map(|e| e.pid).collect();
    pids.sort_unstable();
    pids.dedup();
    let p = pids.into_iter().filter_map(|pid| {
        let d = crate::servidores_leitura::dados_do_processo(pid)?;
        Some(Processo { pid, nome: Some(d.nome), cmd: Some(d.cmd), pasta: d.pasta })
    }).collect();
    Ok(Leitura { c, p })
}

fn ler() -> Result<Vec<Servidor>, String> {
    let eu = std::env::current_exe().ok();
    Ok(classificar(coletar()?, |p| p.exists(), eu.as_deref()))
}

#[tauri::command]
pub async fn servidores_locais() -> Result<Vec<Servidor>, String> {
    tauri::async_runtime::spawn_blocking(ler).await.map_err(|erro| erro.to_string())?
}

/// Derruba o servidor da pasta. Recebe a pasta, não PIDs: a lista é lida de novo aqui, então só
/// morre o que de fato é servidor de projeto agora — nunca um PID qualquer vindo da tela.
/// A árvore inteira vai junto (`npm` → `sh` → `node`); sem isso a porta ficava presa.
#[tauri::command]
pub async fn servidor_matar(pasta: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let servidor = ler()?.into_iter().find(|s| s.pasta == pasta).ok_or("O servidor já não está rodando.")?;
        if servidor.proprio { return Err("Este é o próprio OMNI AGENTS em desenvolvimento.".into()); }
        derrubar(&servidor.pids)
    })
    .await
    .map_err(|erro| erro.to_string())?
}

#[cfg(windows)]
fn derrubar(pids: &[u32]) -> Result<(), String> {
    for pid in pids {
        let saida = crate::git_client::hidden(&mut Command::new("taskkill")).args(["/PID", &pid.to_string(), "/T", "/F"]).output()
            .map_err(|erro| erro.to_string())?;
        if !saida.status.success() { return Err(String::from_utf8_lossy(&saida.stderr).trim().to_owned()); }
    }
    Ok(())
}

#[cfg(unix)]
fn derrubar(pids: &[u32]) -> Result<(), String> {
    for pid in pids { crate::servidores_leitura::derrubar_arvore(*pid, std::time::Duration::from_secs(2))?; }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Diagnóstico: `cargo test servidores_deste_pc -- --ignored --nocapture` lista o que o painel mostraria.
    #[test]
    #[ignore]
    fn servidores_deste_pc() {
        for s in ler().unwrap() { println!("{:?}", (s.nome, s.tipo, s.portas, s.proprio, s.pasta)); }
    }

    /// Leitura de verdade (portas e comandos copiados de um PC real), encurtada.
    #[test]
    fn sobra_so_servidor_de_projeto_agrupado_por_pasta() {
        let json = r#"{"c":[
            {"p":1520,"pid":10},{"p":3300,"pid":11},{"p":60785,"pid":11},{"p":135,"pid":12},
            {"p":57621,"pid":13},{"p":6333,"pid":14},{"p":9229,"pid":15},{"p":1420,"pid":16}],
          "p":[
            {"pid":10,"nome":"node.exe","cmd":"\"node\"   \"D:\\P\\SIAPLUS\\node_modules\\.bin\\\\..\\vite\\bin\\vite.js\""},
            {"pid":11,"nome":"node.exe","cmd":"\"C:\\Program Files\\nodejs\\node.exe\" D:\\P\\STUDYRATS\\node_modules\\.pnpm\\next@16\\node_modules\\next\\dist\\server\\lib\\start-server.js"},
            {"pid":12,"nome":"svchost.exe","cmd":null},
            {"pid":13,"nome":"Spotify.exe","cmd":"Spotify.exe --autostart"},
            {"pid":14,"nome":"node.exe","cmd":"\"C:\\Users\\F\\AppData\\Roaming\\npm\\node_modules\\x\\cli.js\""},
            {"pid":15,"nome":"node.exe","cmd":"node --inspect"},
            {"pid":16,"nome":"node.exe","cmd":"node D:\\P\\OMNI\\node_modules\\vite\\bin\\vite.js"}]}"#;
        let leitura: Leitura = serde_json::from_str(json).unwrap();
        let projetos = ["D:\\P\\SIAPLUS", "D:\\P\\STUDYRATS", "D:\\P\\OMNI", "C:\\Users\\F\\AppData\\Roaming\\npm\\node_modules\\x"];
        let existe = |p: &Path| projetos.iter().any(|d| Path::new(d).join("package.json") == p);
        let lista = classificar(leitura, existe, Some(Path::new("D:\\P\\OMNI\\src-tauri\\target\\debug\\omni.exe")));
        let resumo: Vec<_> = lista.iter().map(|s| (s.nome.as_str(), s.tipo.as_str(), s.portas.clone(), s.proprio)).collect();
        assert_eq!(resumo, vec![
            ("OMNI", "Vite", vec![1420], true),
            ("SIAPLUS", "Vite", vec![1520], false),
            ("STUDYRATS", "Next", vec![3300, 60785], false),
        ]);
    }

    fn unix(json: &str, projetos: &[&str], eu: Option<&str>) -> Vec<(String, String, Vec<u16>, bool)> {
        let leitura: Leitura = serde_json::from_str(json).unwrap();
        let existe = |p: &Path| projetos.iter().any(|d| Path::new(d).join("package.json") == p);
        classificar(leitura, existe, eu.map(Path::new)).into_iter().map(|s| (s.nome, s.tipo, s.portas, s.proprio)).collect()
    }

    #[test]
    fn caminho_unix_vira_projeto() {
        let json = r#"{"c":[{"p":5173,"pid":1}],"p":[
            {"pid":1,"nome":"node","cmd":"node /home/e/p/node_modules/.bin/vite","pasta":"/home/e/p"}]}"#;
        assert_eq!(unix(json, &["/home/e/p"], None), vec![("p".into(), "Vite".into(), vec![5173], false)]);
    }

    #[test]
    fn comando_relativo_usa_a_pasta_do_processo() {
        let json = r#"{"c":[{"p":3000,"pid":1}],"p":[
            {"pid":1,"nome":"node","cmd":"node server.js","pasta":"/home/e/p/api"}]}"#;
        assert_eq!(unix(json, &["/home/e/p"], None), vec![("p".into(), "node".into(), vec![3000], false)]);
    }

    #[test]
    fn fora_de_projeto_no_linux() {
        let json = r#"{"c":[{"p":4001,"pid":1},{"p":4002,"pid":2},{"p":4003,"pid":3},{"p":4004,"pid":4},{"p":4005,"pid":5}],"p":[
            {"pid":1,"nome":"node","cmd":"node /usr/lib/node_modules/x/cli.js","pasta":"/home/e"},
            {"pid":2,"nome":"node","cmd":"node /home/e/.nvm/versions/node/v20/lib/node_modules/y/cli.js"},
            {"pid":3,"nome":"cargo","cmd":"/home/e/.cargo/bin/cargo run","pasta":"/home/e/.cargo"},
            {"pid":4,"nome":"node","cmd":"node /home/e/.local/share/z/s.js"},
            {"pid":5,"nome":"node","cmd":"node /home/e/.npm/_npx/abc/node_modules/.bin/w"}]}"#;
        let tudo = ["/usr/lib/node_modules/x", "/home/e/.nvm/versions/node/v20/lib/node_modules/y", "/home/e/.cargo",
            "/home/e/.local/share/z", "/home/e/.npm/_npx/abc"];
        assert_eq!(unix(json, &tudo, None), vec![]);
    }

    #[test]
    fn python_versionado_conta_como_python() {
        let json = r#"{"c":[{"p":8000,"pid":1}],"p":[
            {"pid":1,"nome":"python3.12","cmd":"python3.12 manage.py runserver","pasta":"/home/e/site"}]}"#;
        assert_eq!(unix(json, &["/home/e/site"], None), vec![("site".into(), "Django".into(), vec![8000], false)]);
    }

    #[test]
    fn proprio_omni_marcado_no_linux() {
        let json = r#"{"c":[{"p":1420,"pid":1},{"p":3000,"pid":2}],"p":[
            {"pid":1,"nome":"node","cmd":"node /home/e/omni/node_modules/vite/bin/vite.js"},
            {"pid":2,"nome":"node","cmd":"node /home/e/omni2/node_modules/vite/bin/vite.js"}]}"#;
        let r = unix(json, &["/home/e/omni", "/home/e/omni2"], Some("/home/e/omni/src-tauri/target/debug/omni-agents"));
        assert_eq!(r.iter().map(|x| (x.0.as_str(), x.3)).collect::<Vec<_>>(), vec![("omni", true), ("omni2", false)]);
    }
}
