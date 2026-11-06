//! Busca de arquivo pelo nome dentro de uma pasta: o agente cita `analise.md` ou `plans/x.md` e o
//! arquivo mora em outra subpasta do projeto.

use std::path::{Path, PathBuf};

const PASTAS_IGNORADAS: [&str; 4] = ["node_modules", ".git", "target", "dist"];
const MAX_RESULTADOS: usize = 20;
/// Teto de entradas lidas, para um projeto enorme não prender a busca.
const MAX_ENTRADAS: usize = 200_000;

fn nome_valido(nome: &str) -> bool {
    !nome.is_empty() && nome != "." && nome != ".." && !nome.contains(['/', '\\'])
}

/// Caminhos de arquivos chamados `nome` sob `raiz`, no máximo `MAX_RESULTADOS`, em ordem estável.
pub fn procurar(raiz: &Path, nome: &str) -> Result<Vec<String>, String> {
    if !nome_valido(nome) {
        return Err("O nome do arquivo não pode ter pasta.".into());
    }
    if !raiz.is_dir() {
        return Err("A pasta da busca não existe.".into());
    }
    let mut achados: Vec<PathBuf> = Vec::new();
    let mut pendentes = vec![raiz.to_path_buf()];
    let mut lidas = 0usize;
    while let Some(pasta) = pendentes.pop() {
        let Ok(entradas) = std::fs::read_dir(&pasta) else { continue };
        let mut entradas: Vec<_> = entradas.flatten().collect();
        entradas.sort_by_key(|e| e.file_name());
        for entrada in entradas {
            lidas += 1;
            if lidas > MAX_ENTRADAS { return Ok(finalizar(achados)); }
            let Ok(tipo) = entrada.file_type() else { continue };
            let caminho = entrada.path();
            if tipo.is_dir() {
                let ignorada = entrada.file_name().to_str().is_some_and(|n| PASTAS_IGNORADAS.contains(&n));
                if !ignorada { pendentes.push(caminho); }
            } else if entrada.file_name().to_str() == Some(nome) {
                achados.push(caminho);
                if achados.len() >= MAX_RESULTADOS { return Ok(finalizar(achados)); }
            }
        }
    }
    Ok(finalizar(achados))
}

fn finalizar(mut achados: Vec<PathBuf>) -> Vec<String> {
    achados.sort();
    achados.into_iter().map(|p| p.to_string_lossy().into_owned()).collect()
}

#[tauri::command]
pub async fn localizar_arquivo(raiz: String, nome: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || procurar(Path::new(&raiz), &nome))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn criar(raiz: &Path, rel: &str) {
        let p = raiz.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "x").unwrap();
    }

    #[test]
    fn acha_em_subpasta() {
        let t = tempfile::tempdir().unwrap();
        criar(t.path(), "docs/analise-360/financeiro.md");
        let r = procurar(t.path(), "financeiro.md").unwrap();
        assert_eq!(r.len(), 1);
        assert!(r[0].ends_with("docs/analise-360/financeiro.md"));
    }

    #[test]
    fn ignora_pastas_de_dependencia_e_build() {
        let t = tempfile::tempdir().unwrap();
        for p in ["node_modules", ".git", "target", "dist"] { criar(t.path(), &format!("{p}/a.md")); }
        criar(t.path(), "src/a.md");
        let r = procurar(t.path(), "a.md").unwrap();
        assert_eq!(r.len(), 1);
        assert!(r[0].ends_with("src/a.md"));
    }

    #[test]
    fn para_em_vinte() {
        let t = tempfile::tempdir().unwrap();
        for i in 0..30 { criar(t.path(), &format!("p{i}/a.md")); }
        assert_eq!(procurar(t.path(), "a.md").unwrap().len(), 20);
    }

    #[test]
    fn recusa_nome_com_separador() {
        let t = tempfile::tempdir().unwrap();
        assert!(procurar(t.path(), "docs/a.md").is_err());
        assert!(procurar(t.path(), "..\\a.md").is_err());
        assert!(procurar(t.path(), "..").is_err());
    }
}
