import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Execucao } from "../features/fluxos/fluxosService";

const listarExecucoes = vi.fn();
const obterExecucao = vi.fn();
const aprovarNo = vi.fn();
vi.mock("../features/fluxos/fluxosService", () => ({
  listarExecucoes: (...a: unknown[]) => listarExecucoes(...a),
  obterExecucao: (...a: unknown[]) => obterExecucao(...a),
  aprovarNo: (...a: unknown[]) => aprovarNo(...a),
}));
const { FaixaDeAprovacao } = await import("../features/fluxos/FaixaDeAprovacao");

function exec(id: string, nome: string, pendentes: { no: string; pergunta: string }[]): Execucao {
  const nos = pendentes.map((p) => ({ id: p.no, tipo: "aprovacao", titulo: p.no, config: { pergunta: p.pergunta }, x: 0, y: 0 }));
  const estado = Object.fromEntries(pendentes.map((p) => [p.no, { status: "aguardando", voltas: 0, inicio_ms: 1, fim_ms: 0, saida: "", session_id: null, custo_usd: 0, erro: null, cli: null, conta: null, caminho: null }]));
  return {
    id, status: pendentes.length ? "aguardando" : "rodando", projeto: "p", projeto_id: "p", entrada: "", pasta: "", branch: null, base: null, erro: null, criado_ms: 1, atualizado_ms: 1, pr_url: null, custo_usd: 0,
    fluxo: { id: "f", nome, descricao: "", conexoes: [], nos }, nos: estado,
  } as unknown as Execucao;
}
function servir(...lista: Execucao[]) {
  listarExecucoes.mockImplementation(async () => lista.map((e) => ({ id: e.id, status: e.status })));
  obterExecucao.mockImplementation(async (id: string) => lista.find((e) => e.id === id));
}
const montar = async () => { render(<FaixaDeAprovacao />); await act(async () => { await vi.advanceTimersByTimeAsync(0); }); };

beforeEach(() => { vi.useFakeTimers({ shouldAdvanceTime: true }); });
afterEach(() => { vi.useRealTimers(); listarExecucoes.mockReset(); obterExecucao.mockReset(); aprovarNo.mockReset(); });

it("faixa mostra o texto do que será aprovado", async () => {
  servir(exec("e1", "Release", [{ no: "ok", pergunta: "Publicar a versão 0.7.8?" }]));
  await montar();
  expect(screen.getByText("Publicar a versão 0.7.8?")).toBeInTheDocument();
});

it("etapa de tarefa de fora parada aparece na faixa, sem nó Aprovação", async () => {
  const e = exec("e1", "Vinda de fora", [{ no: "c", pergunta: "" }]);
  e.fluxo.nos[0] = { ...e.fluxo.nos[0], tipo: "comando", titulo: "Rodar script", config: { comando: "./deploy.sh" } };
  servir(e);
  await montar();
  expect(screen.getByText(/Tarefa de fora: liberar “Rodar script”/)).toBeInTheDocument();
  expect(screen.getByText(/deploy\.sh/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: /^Aprovar/ }));
  expect(aprovarNo).toHaveBeenCalledWith("e1", "c", true);
});

it("Aprovar e Recusar têm nome acessível", async () => {
  servir(exec("e1", "Release", [{ no: "ok", pergunta: "Publicar?" }]));
  await montar();
  expect(screen.getByRole("button", { name: /^Aprovar/ })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /^Recusar/ })).toBeInTheDocument();
});

it("duas pendências aparecem nas duas faixas", async () => {
  servir(exec("e1", "Release", [{ no: "a", pergunta: "Primeira?" }, { no: "b", pergunta: "Segunda?" }]));
  await montar();
  expect(screen.getByText("Primeira?")).toBeInTheDocument();
  expect(screen.getByText("Segunda?")).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: /^Aprovar/ })).toHaveLength(2);
});

it("sem pendência a faixa some", async () => {
  servir(exec("e1", "Release", []));
  await montar();
  expect(screen.queryByRole("region", { name: "Aprovações pendentes" })).not.toBeInTheDocument();
});

it("o engine recusar mantém o erro visível e libera o botão", async () => {
  servir(exec("e1", "Release", [{ no: "ok", pergunta: "Publicar?" }]));
  aprovarNo.mockRejectedValue(new Error("Este nó não está esperando aprovação"));
  await montar();
  await userEvent.click(screen.getByRole("button", { name: /^Aprovar/ }));
  expect(await screen.findByRole("alert")).toHaveTextContent("não está esperando");
  expect(screen.getByRole("button", { name: /^Aprovar/ })).toBeEnabled();
});

it("botões ficam desabilitados enquanto o pedido roda", async () => {
  servir(exec("e1", "Release", [{ no: "ok", pergunta: "Publicar?" }]));
  let fim!: () => void;
  aprovarNo.mockReturnValue(new Promise<void>((r) => { fim = r; }));
  await montar();
  await userEvent.click(screen.getByRole("button", { name: /^Aprovar/ }));
  expect(screen.getByRole("button", { name: /^Aprovar/ })).toBeDisabled();
  expect(screen.getByRole("button", { name: /^Recusar/ })).toBeDisabled();
  await act(async () => { fim(); });
  expect(aprovarNo).toHaveBeenCalledTimes(1);
  expect(aprovarNo).toHaveBeenCalledWith("e1", "ok", true);
});
