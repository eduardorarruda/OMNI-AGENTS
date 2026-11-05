import { invoke } from "@tauri-apps/api/core";
import { normalizar } from "../../components/SettingsSearch";

export interface FiltroDeCampo {
  campo: string;
  valor: string;
}

/** Sem a chave de propósito: ela vai só da tela para o disco, nunca volta. */
export interface ConfiguracaoDeTarefas {
  url: string;
  cabecalho: string;
  caminho_lista: string;
  campos: { id: string; titulo: string; descricao: string; status: string; responsavel: string };
  filtro: FiltroDeCampo;
  excluir_se: string;
  comentarios: { caminho: string; campo_tarefa: string; campo_texto: string; filtro: FiltroDeCampo };
  pessoas: { caminho: string; campo_id: string; campo_nome: string };
}

export const CONFIGURACAO_VAZIA: ConfiguracaoDeTarefas = {
  url: "",
  cabecalho: "",
  caminho_lista: "",
  campos: { id: "", titulo: "", descricao: "", status: "", responsavel: "" },
  filtro: { campo: "", valor: "" },
  excluir_se: "",
  comentarios: { caminho: "", campo_tarefa: "", campo_texto: "", filtro: { campo: "", valor: "" } },
  pessoas: { caminho: "", campo_id: "", campo_nome: "" },
};

export interface ConfiguracaoNaTela {
  configuracao: ConfiguracaoDeTarefas;
  chave_configurada: boolean;
}

export interface TarefaExterna {
  id: string;
  titulo: string;
  descricao: string;
  status: string;
  responsavel: string;
  comentarios: string[];
}

export interface EstadoDasTarefas {
  configurada: boolean;
  tarefas: TarefaExterna[];
  /** Tamanho da lista antes dos filtros: mostra se o filtro comeu tudo. */
  recebidas: number;
  avisos: string[];
  atualizado_em_ms: number | null;
  espera_ms: number;
  deve_buscar: boolean;
}

export const lerConfiguracao = () => invoke<ConfiguracaoNaTela>("tarefas_config");
export const salvarConfiguracao = (configuracao: ConfiguracaoDeTarefas, chave: string | null, removerChave = false) =>
  invoke<void>("tarefas_salvar", { configuracao, chave, removerChave });
/** Só lê o disco. */
export const estadoDasTarefas = () => invoke<EstadoDasTarefas>("tarefas_estado");
/** Vai à API. Chame só por clique, ou ao abrir com `deve_buscar`. */
export const atualizarTarefas = () => invoke<EstadoDasTarefas>("tarefas_atualizar");
export const textoDaTarefa = (id: string) => invoke<string>("tarefas_texto", { id });

export function tempoDesde(ms: number, agora: number): string {
  const minutos = Math.floor((agora - ms) / 60_000);
  if (minutos < 1) return "agora mesmo";
  if (minutos < 60) return `há ${minutos} min`;
  if (minutos < 24 * 60) return `há ${Math.floor(minutos / 60)} h`;
  return `há ${Math.floor(minutos / (24 * 60))} d`;
}

export function filtrarTarefas(tarefas: TarefaExterna[], busca: string): TarefaExterna[] {
  const termo = normalizar(busca.trim());
  if (!termo) return tarefas;
  return tarefas.filter((tarefa) => normalizar(`${tarefa.id} ${tarefa.titulo} ${tarefa.status} ${tarefa.responsavel}`).includes(termo));
}
