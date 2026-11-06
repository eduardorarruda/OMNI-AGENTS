import type { Execucao } from "./fluxosService";

export interface NoAtivo { id: string; rotulo: string; status: "rodando" | "aguardando"; atividade: string; inicioMs: number }

/** Aprovar só vale com a execução de pé: pausada, parada ou cancelada, o engine recusa a resposta. */
export const aceitaResposta = (status: Execucao["status"]) => status === "rodando" || status === "aguardando";

/** O que perguntar sobre um nó parado. Fora a Aprovação do fluxo, só para a etapa de tarefa vinda de fora, antes de rodar. */
export function perguntaDoNo(no: { id: string; tipo: string; titulo: string; config: Record<string, unknown> }): string {
  if (no.tipo === "aprovacao") return String(no.config.pergunta || no.titulo || "Aprovar para seguir?");
  const comando = no.tipo === "comando" && no.config.comando ? ` Comando: ${String(no.config.comando)}` : "";
  return `Tarefa de fora: liberar “${no.titulo || no.id}”? A etapa ainda não rodou.${comando}`;
}

/** Todos os nós que estão trabalhando ou esperando você agora (ramos em paralelo incluídos), pelo início. */
export function nosAtivos(execucao: Execucao): NoAtivo[] {
  const ativos: NoAtivo[] = [];
  for (const [id, estado] of Object.entries(execucao.nos)) {
    if (estado.status !== "rodando" && estado.status !== "aguardando") continue;
    const no = execucao.fluxo.nos.find((n) => n.id === id);
    const nome = no?.titulo || id;
    ativos.push({ id, rotulo: no?.agente ? `${nome} · ${no.agente}` : nome, status: estado.status, atividade: estado.atividade ?? "", inicioMs: estado.inicio_ms });
  }
  return ativos.sort((a, b) => a.inicioMs - b.inicioMs);
}
