import { invoke } from "@tauri-apps/api/core";
import { KANBAN_STATUSES, type KanbanExterna, type KanbanState, type KanbanTask, type ResumoDaImportacao } from "../types/kanban";
import { emptyKanban } from "../features/kanban/kanbanReducer";
import { definirAvisoDoKanban, limparAvisoDoKanban } from "../features/kanban/kanbanAviso";
import { engineApi } from "../features/terminal/terminalService";

/** Cópia antiga do quadro. Só é LIDA (para migrar ou quando o engine está fora): nunca é apagada nem reescrita. */
export const KANBAN_STORAGE_KEY = "omni-agents-kanban";

export const AVISO_ENGINE_FORA =
  "O engine do OMNI está fora do ar: mostrando o Kanban salvo neste computador. Mudanças só serão gravadas quando ele voltar.";
export const AVISO_MIGRACAO_FALHOU =
  "Não foi possível levar o Kanban deste computador para o engine. Mostrando a cópia local; novas mudanças só serão aceitas depois que a migração passar.";
export const AVISO_GRAVACAO_FALHOU =
  "Não foi possível gravar no Kanban. As mudanças que não foram gravadas foram desfeitas.";
export const AVISO_SEM_MIGRACAO =
  "O Kanban deste computador ainda não subiu para o engine. Tente de novo quando o engine voltar.";

function isTask(value: unknown): value is KanbanTask {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<KanbanTask>;
  return (
    typeof candidate.id === "string" &&
    typeof candidate.projectId === "string" &&
    typeof candidate.title === "string" &&
    typeof candidate.description === "string" &&
    typeof candidate.status === "string" &&
    KANBAN_STATUSES.includes(candidate.status) &&
    typeof candidate.position === "number" &&
    typeof candidate.createdAt === "string" &&
    typeof candidate.updatedAt === "string"
  );
}

function normalize(raw: unknown): KanbanState | null {
  if (!raw || typeof raw !== "object") return null;
  const candidate = raw as Partial<KanbanState>;
  if (candidate.version !== 1 || !Array.isArray(candidate.tasks)) return null;
  return {
    version: 1,
    // Task individualmente inválida é descartada sozinha, não o board inteiro.
    tasks: candidate.tasks.filter(isTask),
    triagem: Array.isArray(candidate.triagem) ? candidate.triagem : [],
    agentByProject:
      candidate.agentByProject && typeof candidate.agentByProject === "object" ? candidate.agentByProject : {},
    dispatcherEnabled: candidate.dispatcherEnabled === true,
    failureTimestamps: Array.isArray(candidate.failureTimestamps) ? candidate.failureTimestamps : [],
    pausedUntil: typeof candidate.pausedUntil === "string" ? candidate.pausedUntil : null,
  };
}

/** Lê o quadro antigo do `localStorage`; corrompido vira quadro vazio, sem mexer na chave. */
export function loadKanban(): KanbanState {
  try {
    const raw = localStorage.getItem(KANBAN_STORAGE_KEY);
    if (raw) {
      const normalized = normalize(JSON.parse(raw));
      if (normalized) return normalized;
    }
  } catch {
    // cai pro board vazio abaixo
  }
  return emptyKanban();
}

interface QuadroDoEngine {
  versao: number;
  migrado?: boolean;
  tarefas: unknown[];
  triagem?: unknown[];
  ajustes?: Record<string, unknown>;
}

async function chamar<T>(metodo: string, caminho: string, corpo?: unknown): Promise<T> {
  const resposta = await engineApi(metodo, caminho, corpo === undefined ? null : JSON.stringify(corpo));
  if (resposta.status >= 400) throw new Error(`Kanban: ${metodo} ${caminho} respondeu ${resposta.status}`);
  return JSON.parse(resposta.body) as T;
}

function quadroParaEstado(quadro: QuadroDoEngine): KanbanState {
  return normalize({ version: 1, tasks: quadro.tarefas, triagem: quadro.triagem, ...(quadro.ajustes ?? {}) }) ?? emptyKanban();
}

const AJUSTES = ["agentByProject", "dispatcherEnabled", "failureTimestamps", "pausedUntil"] as const;
const CAMPOS_DO_CARTAO = [
  "projectId", "title", "description", "status", "position", "loopTotal", "loopRemaining", "resultNote", "updatedAt", "externa",
] as const;

function ajustesDoEstado(estado: KanbanState): Record<string, unknown> {
  return Object.fromEntries(AJUSTES.map((chave) => [chave, estado[chave]]));
}

function lerQuadro(): Promise<QuadroDoEngine> {
  return chamar<QuadroDoEngine>("GET", "/kanban");
}

/** Lê o quadro que o engine guarda. Lança se o engine não responder. */
export async function lerDoEngine(): Promise<KanbanState> {
  return quadroParaEstado(await lerQuadro());
}

/** O engine já tem dono do quadro: tem cartão, ou já migrou/recebeu cartão alguma vez (então vazio é vazio de verdade). */
function quadroJaTemDono(quadro: QuadroDoEngine): boolean {
  return quadro.migrado === true || quadro.tarefas.length > 0 || (quadro.triagem?.length ?? 0) > 0;
}

function temAlgoParaMigrar(local: KanbanState): boolean {
  const padrao = ajustesDoEstado(emptyKanban());
  return (
    local.tasks.length > 0 ||
    Object.entries(ajustesDoEstado(local)).some(([chave, valor]) => JSON.stringify(valor) !== JSON.stringify(padrao[chave]))
  );
}

/** Leva o quadro do `localStorage` para o engine, uma vez. O engine só aceita com o quadro dele vazio;
 * a chave antiga nunca é apagada. */
export async function migrarSePreciso(): Promise<"migrou" | "nada" | "falhou"> {
  try {
    return await migrarSeVazio(await lerQuadro());
  } catch {
    return "falhou";
  }
}

async function migrarSeVazio(quadro: QuadroDoEngine): Promise<"migrou" | "nada" | "falhou"> {
  if (quadroJaTemDono(quadro)) return "nada";
  const local = loadKanban();
  if (!temAlgoParaMigrar(local)) return "nada";
  try {
    const resposta = await chamar<{ migrou: boolean }>("POST", "/kanban/migrar", {
      versao: 1,
      tarefas: local.tasks,
      ajustes: ajustesDoEstado(local),
    });
    return resposta.migrou ? "migrou" : "nada";
  } catch {
    return "falhou";
  }
}

/** Só chamada pelo clique em Atualizar. A migração vem antes: triagem no engine impediria o quadro
 * antigo de subir depois. */
export async function atualizarExternas(): Promise<ResumoDaImportacao> {
  if ((await migrarSePreciso()) === "falhou") throw new Error(AVISO_SEM_MIGRACAO);
  return invoke<ResumoDaImportacao>("kanban_atualizar_externas");
}

export function escolherProjetoDaExterna(tarefa: KanbanExterna, projectId: string): Promise<KanbanTask> {
  return chamar<KanbanTask>("POST", `/kanban/externas/${encodeURIComponent(tarefa.id)}/projeto`, { projectId, origem: tarefa.origem });
}

export interface ProjetoDoEngine {
  id: string;
  name: string;
}

export async function listarProjetosDoEngine(): Promise<ProjetoDoEngine[]> {
  return (await chamar<{ projects: ProjetoDoEngine[] }>("GET", "/projetos")).projects;
}

export type OrigemDoKanban = "engine" | "local";

/** Carrega o quadro: do engine (migrando o antigo se ele estiver vazio) ou, com o engine fora do ar,
 * da cópia local, avisando. */
export async function carregarKanbanComOrigem(): Promise<{ estado: KanbanState; origem: OrigemDoKanban }> {
  let quadro: QuadroDoEngine;
  try {
    quadro = await lerQuadro();
  } catch {
    definirAvisoDoKanban(AVISO_ENGINE_FORA);
    return { estado: loadKanban(), origem: "local" };
  }
  if (!quadroJaTemDono(quadro)) {
    const resultado = await migrarSeVazio(quadro);
    if (resultado === "falhou") {
      // Engine de pé e vazio, mas a migração caiu: a tela fica com a cópia local e NÃO aceita cartão novo,
      // senão o primeiro cartão novo marcaria o engine como migrado e os antigos nunca mais subiriam.
      definirAvisoDoKanban(AVISO_MIGRACAO_FALHOU);
      return { estado: loadKanban(), origem: "local" };
    }
    if (resultado === "migrou") {
      try {
        quadro = await lerQuadro();
      } catch {
        definirAvisoDoKanban(AVISO_ENGINE_FORA);
        return { estado: loadKanban(), origem: "local" };
      }
    }
  }
  limparAvisoDoKanban();
  return { estado: quadroParaEstado(quadro), origem: "engine" };
}

export async function carregarKanban(): Promise<KanbanState> {
  return (await carregarKanbanComOrigem()).estado;
}

/** Grava no engine a diferença entre dois estados: no máximo uma operação por cartão mudado, mais os
 * ajustes que mudaram. Nunca manda o quadro inteiro. */
export async function aplicarNoEngine(antes: KanbanState, depois: KanbanState): Promise<void> {
  const anteriores = new Map(antes.tasks.map((t) => [t.id, t]));
  const atuais = new Set(depois.tasks.map((t) => t.id));
  for (const tarefa of depois.tasks) {
    const anterior = anteriores.get(tarefa.id);
    if (!anterior) {
      await chamar("POST", "/kanban/cartoes", tarefa);
      continue;
    }
    const mudancas: Record<string, unknown> = {};
    for (const campo of CAMPOS_DO_CARTAO) {
      if (JSON.stringify(anterior[campo]) !== JSON.stringify(tarefa[campo])) mudancas[campo] = tarefa[campo] ?? null;
    }
    if (Object.keys(mudancas).length > 0) await chamar("PATCH", `/kanban/cartoes/${encodeURIComponent(tarefa.id)}`, mudancas);
  }
  for (const tarefa of antes.tasks) {
    if (!atuais.has(tarefa.id)) await chamar("DELETE", `/kanban/cartoes/${encodeURIComponent(tarefa.id)}`);
  }
  const ajustesAntes = ajustesDoEstado(antes);
  const ajustesDepois = ajustesDoEstado(depois);
  const mudados = Object.fromEntries(
    AJUSTES.filter((c) => JSON.stringify(ajustesAntes[c]) !== JSON.stringify(ajustesDepois[c])).map((c) => [c, ajustesDepois[c]]),
  );
  if (Object.keys(mudados).length > 0) await chamar("PATCH", "/kanban/ajustes", mudados);
}
