import type { Mensagem } from "../../mobile/api";
import { pedidoDeParada } from "../../mobile/TrabalhoAoVivo";
import { engineApi } from "../terminal/terminalService";

/** O que o engine sabe de um subagente (`GET /conversas/{id}/subagentes/{agente}`). Texto já sem segredos. */
export interface DetalheDoSubagente {
  id: string;
  tipo: string;
  descricao: string;
  estado: "rodando" | "concluido" | "parado";
  ultima_acao: string | null;
  atualizado_ms: number;
  modelo: string | null;
  desde_ms: number | null;
  /** O pedido que ele recebeu, inteiro. */
  prompt: string;
  /** O que entregou ao terminar. */
  resultado: string | null;
}

export interface LeituraDoSubagente {
  detalhe: DetalheDoSubagente | null;
  mensagens: Mensagem[];
  /** Há ações mais antigas que as mostradas. */
  cortado: boolean;
}

const rota = (conversaId: string, agenteId: string) =>
  `/conversas/${encodeURIComponent(conversaId)}/subagentes/${encodeURIComponent(agenteId)}`;

function lerCorpo(r: { status: number; body: string | null }) {
  const dados = r.body ? JSON.parse(r.body) : null;
  if (r.status >= 400) throw new Error(dados?.error ?? `Falha HTTP ${r.status}`);
  return dados;
}

export async function lerSubagente(conversaId: string, agenteId: string): Promise<LeituraDoSubagente> {
  const dados = lerCorpo(await engineApi("GET", rota(conversaId, agenteId), null));
  const timeline = dados?.timeline;
  return {
    detalhe: dados?.subagente ?? null,
    mensagens: Array.isArray(timeline?.messages) ? timeline.messages : [],
    cortado: typeof timeline?.prev_cursor === "number",
  };
}

/** Para só este subagente: primeiro pelo painel da CLI; não deu, pede ao agente principal (`TaskStop`). */
export async function pararSubagente(conversaId: string, detalhe: Pick<DetalheDoSubagente, "id" | "tipo" | "descricao">): Promise<"na_tela" | "pedido"> {
  const json: [string, string][] = [["Content-Type", "application/json"]];
  const direto = lerCorpo(await engineApi("POST", `${rota(conversaId, detalhe.id)}/parar`, "{}", { headers: json }));
  if (direto?.na_tela) return "na_tela";
  lerCorpo(await engineApi("POST", `/conversas/${encodeURIComponent(conversaId)}/prompt`,
    JSON.stringify({ texto: pedidoDeParada(detalhe) }), { headers: json }));
  return "pedido";
}
