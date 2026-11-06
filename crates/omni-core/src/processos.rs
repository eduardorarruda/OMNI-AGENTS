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
    fn descendentes_junta_irmaos_e_netos() {
        let v = descendentes(1, &[(2, 1), (3, 1), (4, 3)]);
        assert_eq!(v.len(), 3);
        let pos = |pid| v.iter().position(|p| *p == pid).unwrap();
        assert!(pos(4) < pos(3));
    }
}
