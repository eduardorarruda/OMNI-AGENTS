import type { Execucao } from "./fluxosService";

export interface NoAtivo { id: string; rotulo: string; status: "rodando" | "aguardando"; atividade: string; inicioMs: number }

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
