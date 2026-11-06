//! Árvore de processos sem tocar no sistema: quem recebe a lista de pares é que lê o SO.

/// Todos os descendentes de `raiz`, filhos antes dos pais (a ordem de matar de baixo para cima).
/// `pares` são `(pid, ppid)`. A própria raiz não entra.
pub fn descendentes(raiz: u32, pares: &[(u32, u32)]) -> Vec<u32> {
    let mut vistos = vec![raiz];
    let mut ordem = Vec::new();
    let mut fila = vec![raiz];
    while let Some(pai) = fila.pop() {
        for (pid, _) in pares.iter().filter(|(_, ppid)| *ppid == pai) {
            if vistos.contains(pid) { continue; }
            vistos.push(*pid);
            ordem.push(*pid);
            fila.push(*pid);
        }
    }
    ordem.reverse();
    ordem
}

/// Lê `(pid, ppid)` da saída de `ps -axo pid=,ppid=`; linha que não é número é ignorada.
pub fn parse_ps_pares(saida: &str) -> Vec<(u32, u32)> {
    saida.lines().filter_map(|l| {
        let mut c = l.split_whitespace();
        Some((c.next()?.parse().ok()?, c.next()?.parse().ok()?))
    }).collect()
}

/// Pares `(pid, ppid)` de todos os processos do sistema (`ps` existe no Linux e no macOS).
#[cfg(unix)]
pub fn pares_do_sistema() -> Result<Vec<(u32, u32)>, String> {
    let saida = std::process::Command::new("ps").args(["-axo", "pid=,ppid="]).output().map_err(|e| format!("ps: {e}"))?;
    if !saida.status.success() { return Err(format!("ps saiu com {}", saida.status)); }
    Ok(parse_ps_pares(&String::from_utf8_lossy(&saida.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descendentes_vem_antes_do_pai() {
        assert_eq!(descendentes(1, &[(2, 1), (3, 2), (9, 8)]), vec![3, 2]);
    }

    #[test]
    fn descendentes_aguenta_ciclo_e_raiz_sem_filhos() {
        assert_eq!(descendentes(5, &[(1, 2)]), Vec::<u32>::new());
        assert_eq!(descendentes(1, &[(2, 1), (1, 2)]), vec![2]);
    }

    #[test]
    fn parse_ps_pares_ignora_lixo() {
        assert_eq!(parse_ps_pares("    1     0\n  200   1\nlixo\n"), vec![(1, 0), (200, 1)]);
    }

    #[test]
    fn descendentes_junta_irmaos_e_netos() {
        let v = descendentes(1, &[(2, 1), (3, 1), (4, 3)]);
        assert_eq!(v.len(), 3);
        let pos = |pid| v.iter().position(|p| *p == pid).unwrap();
        assert!(pos(4) < pos(3));
    }
}
