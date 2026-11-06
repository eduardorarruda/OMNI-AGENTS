//! Fila do OMNI: mensagem mandada com o agente ocupado fica aqui, e não na CLI, até ele ficar livre.
//! Enquanto ela não entrou, dá para tirar. Só as regras: quem grava e quem cola é o engine.

use serde::{Deserialize, Serialize};

/// Quem segura a mensagem até o agente ficar livre. `Cli` só vale para a CLI medida numa tela real.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModoDeFila { Cli, Omni }

/// Toda CLI começa na fila do OMNI: nenhuma teve o X medido numa tela real.
pub fn modo_de_fila(_provider: &str) -> ModoDeFila { ModoDeFila::Omni }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemDaFila {
    pub id: String,
    pub texto: String,
    #[serde(default)] pub anexos: Vec<String>,
    pub em_ms: u64,
}

/// Quantos ids entregues ficam guardados: o bastante para o X de uma tela atrasada receber "já foi".
pub const ENTREGUES_GUARDADOS: usize = 64;
/// Teto por conversa: a fila é de mensagens de uma pessoa, não um depósito.
pub const ITENS_MAX: usize = 128;
/// Depois de uma entrega, quanto esperar o turno aparecer. `/comando` local não abre turno nenhum.
pub const ESPERA_DO_TURNO_MS: u64 = 10_000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilaDaConversa {
    pub conversa: String,
    #[serde(default)] pub itens: Vec<ItemDaFila>,
    /// Depois de Parar: nada entra sozinho até o usuário mandar algo novo ou tirar os itens.
    #[serde(default)] pub pausada: bool,
    /// Saiu da fila e está sendo colada. Volta para a frente se a entrega falhar.
    #[serde(default)] pub entregando: Option<ItemDaFila>,
    #[serde(default)] pub entregues: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tirada { Tirada, JaEntregue, Ausente }

/// Mensagem acabou de entrar na CLI: a próxima espera o turno dela, senão as duas colam juntas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AguardoDoTurno { pub desde_ms: u64, pub turnos: u64 }

impl AguardoDoTurno {
    pub fn liberado(&self, agora_ms: u64, turnos: u64) -> bool {
        turnos > self.turnos || agora_ms.saturating_sub(self.desde_ms) >= ESPERA_DO_TURNO_MS
    }
}

impl FilaDaConversa {
    pub fn nova(conversa: &str) -> Self { Self { conversa: conversa.to_owned(), ..Self::default() } }
    pub fn vazia(&self) -> bool { self.itens.is_empty() && self.entregando.is_none() }
    /// Entra no fim. Mensagem nova é o usuário de volta: tira a pausa.
    pub fn guardar(&mut self, item: ItemDaFila) {
        self.itens.push(item);
        self.pausada = false;
    }
    /// Era a vez dela, mas a tela mudou antes de colar: passa na frente das que chegaram depois.
    pub fn guardar_na_frente(&mut self, item: ItemDaFila) { self.itens.insert(0, item); }
    pub fn tirar(&mut self, id: &str) -> Tirada {
        if self.entregando.as_ref().is_some_and(|item| item.id == id) || self.entregues.iter().any(|e| e == id) {
            return Tirada::JaEntregue;
        }
        let Some(posicao) = self.itens.iter().position(|item| item.id == id) else { return Tirada::Ausente };
        self.itens.remove(posicao);
        if self.itens.is_empty() { self.pausada = false; }
        Tirada::Tirada
    }
    /// Pausa só o que existe: Parar sem nada guardado não deixa marca.
    pub fn pausar(&mut self) { self.pausada = !self.itens.is_empty(); }
    pub fn pode_entregar(&self) -> bool { !self.pausada && self.entregando.is_none() && !self.itens.is_empty() }
    /// A próxima da vez, já marcada como em entrega: a partir daqui o X responde "já foi".
    pub fn proxima(&mut self) -> Option<ItemDaFila> {
        if !self.pode_entregar() { return None; }
        let item = self.itens.remove(0);
        self.entregando = Some(item.clone());
        Some(item)
    }
    pub fn entregue(&mut self) {
        let Some(item) = self.entregando.take() else { return };
        self.entregues.push(item.id);
        let sobra = self.entregues.len().saturating_sub(ENTREGUES_GUARDADOS);
        self.entregues.drain(..sobra);
    }
    pub fn devolver(&mut self) {
        if let Some(item) = self.entregando.take() { self.itens.insert(0, item); }
    }
}

const MARCADOR_ANEXOS: &str = "[Anexos enviados pelo OMNI]";

/// Os caminhos da lista de anexos que `montarPrompt` põe no fim da mensagem.
pub fn anexos_do_texto(texto: &str) -> Vec<String> {
    let Some((_, lista)) = texto.rsplit_once(MARCADOR_ANEXOS) else { return Vec::new() };
    let linhas: Vec<&str> = lista.lines().map(str::trim).filter(|linha| !linha.is_empty()).collect();
    // Marcador citado no meio da frase, sem a lista logo depois, não é anexo.
    if !linhas.iter().all(|linha| linha.starts_with("- ")) { return Vec::new(); }
    linhas.iter().map(|linha| linha[2..].to_owned()).collect()
}

/// Nome do arquivo da conversa. O id vem da URL: nada dele vira separador de pasta, e ids diferentes
/// nunca dão o mesmo nome.
pub fn nome_do_arquivo(conversa: &str) -> String {
    let seguro: String = conversa.bytes().map(|byte| match byte {
        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' => (byte as char).to_string(),
        outro => format!("%{outro:02x}"),
    }).collect();
    format!("{seguro}.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> ItemDaFila { ItemDaFila { id: id.into(), texto: format!("texto {id}"), anexos: vec![], em_ms: 1 } }
    fn fila(ids: &[&str]) -> FilaDaConversa {
        let mut fila = FilaDaConversa::nova("c");
        ids.iter().for_each(|id| fila.guardar(item(id)));
        fila
    }

    #[test] fn toda_cli_comeca_na_fila_do_omni() {
        for cli in ["claude", "codex", "cursor", "antigravity", ""] { assert_eq!(modo_de_fila(cli), ModoDeFila::Omni); }
    }

    #[test] fn sai_na_ordem_e_uma_de_cada_vez() {
        let mut fila = fila(&["a", "b"]);
        assert_eq!(fila.proxima().unwrap().id, "a");
        assert!(fila.proxima().is_none(), "com uma em entrega, a outra espera");
        fila.entregue();
        assert_eq!(fila.proxima().unwrap().id, "b");
    }

    #[test] fn entrega_que_falha_volta_para_a_frente() {
        let mut fila = fila(&["a", "b"]);
        fila.proxima();
        fila.devolver();
        assert_eq!(fila.itens.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert!(fila.entregando.is_none());
    }

    #[test] fn tirar_distingue_tirada_entregue_e_ausente() {
        let mut fila = fila(&["a", "b", "c"]);
        fila.proxima(); fila.entregue();
        fila.proxima();
        assert_eq!(fila.tirar("c"), Tirada::Tirada);
        assert_eq!(fila.tirar("c"), Tirada::Ausente);
        assert_eq!(fila.tirar("a"), Tirada::JaEntregue);
        assert_eq!(fila.tirar("b"), Tirada::JaEntregue, "colando agora já não dá para tirar");
    }

    #[test] fn pausa_segura_e_sai_com_mensagem_nova() {
        let mut fila = fila(&["a"]);
        fila.pausar();
        assert!(!fila.pode_entregar());
        assert!(fila.proxima().is_none());
        fila.guardar(item("b"));
        assert!(fila.pode_entregar());
        assert_eq!(fila.proxima().unwrap().id, "a");
    }

    #[test] fn pausa_sai_quando_tira_o_ultimo_item_e_nao_antes() {
        let mut fila = fila(&["a", "b"]);
        fila.pausar();
        fila.tirar("a");
        assert!(fila.pausada);
        fila.tirar("b");
        assert!(!fila.pausada);
    }

    #[test] fn parar_sem_fila_nao_pausa() {
        let mut fila = FilaDaConversa::nova("c");
        fila.pausar();
        assert!(!fila.pausada);
    }

    #[test] fn lembra_so_as_ultimas_entregues() {
        let mut fila = FilaDaConversa::nova("c");
        for n in 0..ENTREGUES_GUARDADOS + 5 {
            fila.guardar(item(&n.to_string()));
            fila.proxima(); fila.entregue();
        }
        assert_eq!(fila.entregues.len(), ENTREGUES_GUARDADOS);
        assert_eq!(fila.tirar("0"), Tirada::Ausente);
        assert_eq!(fila.tirar(&(ENTREGUES_GUARDADOS + 4).to_string()), Tirada::JaEntregue);
    }

    #[test] fn aguardo_libera_com_turno_novo_ou_com_o_tempo() {
        let aguardo = AguardoDoTurno { desde_ms: 1_000, turnos: 3 };
        assert!(!aguardo.liberado(1_500, 3));
        assert!(aguardo.liberado(1_500, 4));
        assert!(aguardo.liberado(1_000 + ESPERA_DO_TURNO_MS, 3));
    }

    #[test] fn le_os_anexos_do_fim_da_mensagem() {
        assert_eq!(anexos_do_texto("olha\n\n[Anexos enviados pelo OMNI]\n- /tmp/a.png\n- /tmp/b.txt"), ["/tmp/a.png", "/tmp/b.txt"]);
        assert!(anexos_do_texto("sem anexo").is_empty());
        assert!(anexos_do_texto("cita [Anexos enviados pelo OMNI] no meio da frase").is_empty());
    }

    #[test] fn id_estranho_nao_vira_caminho() {
        assert_eq!(nome_do_arquivo("conversa-1_a"), "conversa-1_a.json");
        for id in ["../../etc/passwd", "a/b", "a\\b", "..", "c:d"] {
            let nome = nome_do_arquivo(id);
            assert!(!nome.contains(['/', '\\', ':']) && !nome.starts_with(".."), "{nome}");
        }
        assert_ne!(nome_do_arquivo("a/b"), nome_do_arquivo("a_b"));
    }
}
