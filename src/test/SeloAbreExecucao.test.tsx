import { afterEach, expect, it, vi } from "vitest";
import { act, render, waitFor } from "@testing-library/react";

const obterExecucao = vi.fn();
vi.mock("../features/fluxos/fluxosService", async (original) => ({
  ...(await original<typeof import("../features/fluxos/fluxosService")>()),
  listarFluxos: async () => [], listarExecucoes: async () => [], lerEventos: async () => [],
  obterExecucao: (...a: unknown[]) => obterExecucao(...a),
}));
vi.mock("../features/agentes/agentesService", async (original) => ({
  ...(await original<typeof import("../features/agentes/agentesService")>()),
  listarAgentes: async () => [], listarModelos: async () => ({}),
}));
vi.mock("../features/terminal/terminalService", async (original) => ({
  ...(await original<typeof import("../features/terminal/terminalService")>()), listProfiles: async () => [],
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: vi.fn() }));
const { FluxosView } = await import("../features/fluxos/FluxosView");
const { pedirExecucao, consumirPedidoDeExecucao } = await import("../features/fluxos/pedidoDeExecucao");

class RO { observe() {} unobserve() {} disconnect() {} }
vi.stubGlobal("ResizeObserver", RO);
afterEach(() => { obterExecucao.mockReset(); consumirPedidoDeExecucao(); });
const props = { projectId: null, projectPath: null, onAbrirSessao: () => undefined, onClose: () => undefined };

it("pedido pendente do selo abre a execução ao montar a tela de Fluxos", async () => {
  obterExecucao.mockRejectedValue(new Error("x"));
  pedirExecucao("ex-pendente");
  render(<FluxosView {...props} />);
  await waitFor(() => expect(obterExecucao).toHaveBeenCalledWith("ex-pendente"));
  expect(consumirPedidoDeExecucao()).toBeNull();
});

it("com a tela de Fluxos já aberta, o pedido do selo abre a execução na hora", async () => {
  obterExecucao.mockRejectedValue(new Error("x"));
  render(<FluxosView {...props} />);
  await act(async () => { await Promise.resolve(); });
  expect(obterExecucao).not.toHaveBeenCalled();
  act(() => pedirExecucao("ex-viva"));
  await waitFor(() => expect(obterExecucao).toHaveBeenCalledWith("ex-viva"));
});
