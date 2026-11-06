import { describe, expect, it } from "vitest";
import { nosAtivos } from "../features/fluxos/execucaoModel";
import type { Execucao, EstadoDoNo } from "../features/fluxos/fluxosService";

const estado = (status: EstadoDoNo["status"], extra: Partial<EstadoDoNo> = {}): EstadoDoNo => ({
  status, voltas: 0, inicio_ms: 0, fim_ms: 0, saida: "", session_id: null, custo_usd: 0, erro: null, cli: null, conta: null, caminho: null, ...extra,
});
const execucao = (nos: Record<string, EstadoDoNo>): Execucao => ({
  id: "x", projeto: "p", projeto_id: "p", entrada: "", pasta: "", branch: null, base: null, status: "rodando", erro: null,
  criado_ms: 0, atualizado_ms: 0, pr_url: null, custo_usd: 0, nos,
  fluxo: {
    id: "f", nome: "F", descricao: "", conexoes: [],
    nos: [
      { id: "a", tipo: "agente", titulo: "Dev", agente: "dev", config: {}, x: 0, y: 0 },
      { id: "b", tipo: "agente", titulo: "QA", config: {}, x: 0, y: 0 },
      { id: "c", tipo: "aprovacao", titulo: "", config: {}, x: 0, y: 0 },
    ],
  },
});

describe("nosAtivos", () => {
  it("devolve os dois ramos em paralelo", () => {
    const r = nosAtivos(execucao({ a: estado("rodando", { atividade: "editando", inicio_ms: 5 }), b: estado("rodando", { inicio_ms: 9 }), c: estado("pendente") }));
    expect(r.map((n) => n.id)).toEqual(["a", "b"]);
    expect(r[0]).toEqual({ id: "a", rotulo: "Dev · dev", status: "rodando", atividade: "editando", inicioMs: 5 });
  });

  it("nó aguardando conta como ativo", () => {
    const r = nosAtivos(execucao({ a: estado("concluido"), c: estado("aguardando") }));
    expect(r).toEqual([{ id: "c", rotulo: "c", status: "aguardando", atividade: "", inicioMs: 0 }]);
  });
});
