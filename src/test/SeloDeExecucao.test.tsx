import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Execucao } from "../features/fluxos/fluxosService";

const listarExecucoes = vi.fn();
const obterExecucao = vi.fn();
vi.mock("../features/fluxos/fluxosService", () => ({
  listarExecucoes: (...a: unknown[]) => listarExecucoes(...a),
  obterExecucao: (...a: unknown[]) => obterExecucao(...a),
}));
const { SeloDeExecucao } = await import("../features/fluxos/SeloDeExecucao");
const { useExecucoesAtivas } = await import("../features/fluxos/useExecucoesAtivas");
const { criarArmazem } = await import("../features/fluxos/armazemDeExecucoes");
const { consumirPedidoDeExecucao } = await import("../features/fluxos/pedidoDeExecucao");

function exec(id: string, nome: string, status: Execucao["status"], no: { titulo: string; tipo?: string; status: string }): Execucao {
  return {
    id, status, projeto: "p", projeto_id: "p", entrada: "", pasta: "", branch: null, base: null, erro: null, criado_ms: 1, atualizado_ms: 1, pr_url: null, custo_usd: 0,
    fluxo: { id: "f", nome, descricao: "", conexoes: [], nos: [{ id: "n1", tipo: (no.tipo ?? "agente") as never, titulo: no.titulo, config: {}, x: 0, y: 0 }] },
    nos: { n1: { status: no.status as never, voltas: 0, inicio_ms: 1, fim_ms: 0, saida: "", session_id: null, custo_usd: 0, erro: null, cli: null, conta: null, caminho: null } },
  };
}
function servir(...lista: Execucao[]) {
  listarExecucoes.mockImplementation(async () => lista.map((e) => ({ id: e.id, status: e.status })));
  obterExecucao.mockImplementation(async (id: string) => lista.find((e) => e.id === id));
}
const abrir = vi.fn();
const montar = async () => { render(<SeloDeExecucao onAbrir={abrir} />); await act(async () => { await vi.advanceTimersByTimeAsync(0); }); };

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { vi.useRealTimers(); listarExecucoes.mockReset(); obterExecucao.mockReset(); abrir.mockReset(); });

it("sem execução o selo não aparece", async () => {
  servir();
  await montar();
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
});

it("uma execução mostra \"Fluxo X: etapa Y\"", async () => {
  servir(exec("e1", "Release", "rodando", { titulo: "Testar", status: "rodando" }));
  await montar();
  expect(screen.getByRole("button", { name: /Fluxo Release: Testar/ })).toBeInTheDocument();
});

it("duas execuções mostram a contagem e listam ao abrir", async () => {
  servir(exec("e1", "Release", "rodando", { titulo: "Testar", status: "rodando" }), exec("e2", "Docs", "rodando", { titulo: "Escrever", status: "rodando" }));
  await montar();
  const selo = screen.getByRole("button", { name: /2 fluxos rodando/ });
  await userEvent.setup({ advanceTimers: vi.advanceTimersByTime }).click(selo);
  expect(screen.getByText(/Fluxo Release: Testar/)).toBeInTheDocument();
  expect(screen.getByText(/Fluxo Docs: Escrever/)).toBeInTheDocument();
});

it("aprovação pendente muda o selo e o texto", async () => {
  servir(exec("e1", "Release", "aguardando", { titulo: "Publicar?", tipo: "aprovacao", status: "aguardando" }));
  await montar();
  const selo = screen.getByRole("button", { name: /Fluxo Release: aprovar Publicar\?/ });
  expect(selo).toHaveAttribute("data-tom", "aprovacao");
});

it("clicar leva à execução", async () => {
  servir(exec("e1", "Release", "rodando", { titulo: "Testar", status: "rodando" }));
  await montar();
  await userEvent.setup({ advanceTimers: vi.advanceTimersByTime }).click(screen.getByRole("button", { name: /Fluxo Release/ }));
  expect(abrir).toHaveBeenCalled();
  expect(consumirPedidoDeExecucao()).toBe("e1");
});

it("execução iniciada pela IA aparece igual", async () => {
  // A IA inicia pela mesma rota do engine: o selo só olha a lista, não quem iniciou.
  servir(exec("ia1", "Revisão", "rodando", { titulo: "Revisar", status: "rodando" }));
  await montar();
  expect(screen.getByRole("button", { name: /Fluxo Revisão: Revisar/ })).toBeInTheDocument();
});

it("uma consulta compartilhada: vários componentes, um temporizador, e para quando ninguém usa", async () => {
  servir(exec("e1", "Release", "rodando", { titulo: "Testar", status: "rodando" }));
  function Uso() { useExecucoesAtivas(); return null; }
  const a = render(<Uso />); const b = render(<Uso />); const c = render(<SeloDeExecucao onAbrir={abrir} />);
  await act(async () => { await vi.advanceTimersByTimeAsync(0); });
  expect(listarExecucoes).toHaveBeenCalledTimes(1);
  await act(async () => { await vi.advanceTimersByTimeAsync(4000); });
  expect(listarExecucoes).toHaveBeenCalledTimes(2);
  a.unmount(); b.unmount(); c.unmount();
  await act(async () => { await vi.advanceTimersByTimeAsync(20000); });
  expect(listarExecucoes).toHaveBeenCalledTimes(2);
});

it("erro de consulta mantém a última leitura e registra o erro", async () => {
  const leitor = vi.fn()
    .mockResolvedValueOnce([exec("e1", "Release", "rodando", { titulo: "Testar", status: "rodando" })])
    .mockRejectedValueOnce(new Error("engine fora"))
    .mockResolvedValueOnce([exec("e1", "Release", "rodando", { titulo: "Testar", status: "rodando" })]);
  const armazem = criarArmazem(leitor);
  const parar = armazem.assinar(() => undefined);
  await vi.advanceTimersByTimeAsync(0);
  expect(armazem.leitura().ativas).toHaveLength(1);
  expect(armazem.leitura().erro).toBeNull();
  await vi.advanceTimersByTimeAsync(4000);
  expect(armazem.leitura().ativas).toHaveLength(1);
  expect(armazem.leitura().erro).toBe("engine fora");
  await vi.advanceTimersByTimeAsync(4000);
  expect(armazem.leitura().erro).toBeNull();
  parar();
});

it("detalhe que falha (execução removida) não derruba as outras nem mostra erro", async () => {
  const boa = exec("e1", "Release", "rodando", { titulo: "Testar", status: "rodando" });
  listarExecucoes.mockImplementation(async () => [{ id: "e1", status: "rodando" }, { id: "sumiu", status: "rodando" }]);
  obterExecucao.mockImplementation(async (id: string) => { if (id === "sumiu") throw new Error("Execução não encontrada"); return boa; });
  function Espia() { const { erro } = useExecucoesAtivas(); return <i data-testid="erro">{erro ?? ""}</i>; }
  render(<><SeloDeExecucao onAbrir={abrir} /><Espia /></>);
  await act(async () => { await vi.advanceTimersByTimeAsync(0); });
  expect(screen.getByRole("button", { name: /Fluxo Release: Testar/ })).toBeInTheDocument();
  expect(screen.getByTestId("erro")).toBeEmptyDOMElement();
});
