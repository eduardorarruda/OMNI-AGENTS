import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";

const lerEventos = vi.fn();
vi.mock("../features/fluxos/fluxosService", () => ({ lerEventos: (...a: unknown[]) => lerEventos(...a) }));
const { LinhaDoTempo } = await import("../features/fluxos/LinhaDoTempo");

const ev = (seq: number, texto: string) => ({ seq, ms: 1000 * seq, no: "a", tipo: "atividade", texto });
beforeEach(() => vi.useFakeTimers());
afterEach(() => { vi.useRealTimers(); lerEventos.mockReset(); });
const passa = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });

it("LinhaDoTempo acrescenta sem repetir", async () => {
  lerEventos.mockResolvedValueOnce([ev(1, "um"), ev(2, "dois")]).mockResolvedValueOnce([ev(2, "dois"), ev(3, "três")]).mockResolvedValue([]);
  render(<LinhaDoTempo execucaoId="x" viva />);
  await passa(0); await passa(1500);
  expect(screen.getAllByText("dois")).toHaveLength(1);
  expect(screen.getAllByRole("listitem").map((l) => l.textContent)).toEqual([expect.stringContaining("um"), expect.stringContaining("dois"), expect.stringContaining("três")]);
});

it("LinhaDoTempo pede a partir do último seq", async () => {
  lerEventos.mockResolvedValueOnce([ev(1, "um"), ev(7, "sete")]).mockResolvedValue([]);
  render(<LinhaDoTempo execucaoId="x" viva />);
  await passa(0); await passa(1500);
  expect(lerEventos).toHaveBeenNthCalledWith(1, "x", 0);
  expect(lerEventos).toHaveBeenNthCalledWith(2, "x", 7);
});

it("LinhaDoTempo para de consultar quando a execução termina", async () => {
  lerEventos.mockResolvedValue([]);
  const { rerender } = render(<LinhaDoTempo execucaoId="x" viva />);
  await passa(0);
  rerender(<LinhaDoTempo execucaoId="x" viva={false} />);
  await passa(0);
  const chamadas = lerEventos.mock.calls.length;
  await passa(10000);
  expect(lerEventos.mock.calls.length).toBe(chamadas);
});

it("LinhaDoTempo mostra evento sem nó como Execução", async () => {
  lerEventos.mockResolvedValue([{ seq: 1, ms: 1000, no: null, tipo: "fim", texto: "Execução cancelada" }]);
  render(<LinhaDoTempo execucaoId="x" viva={false} />);
  await passa(0);
  expect(screen.getByText("Execução")).toBeInTheDocument();
  expect(screen.getByText("Execução cancelada")).toBeInTheDocument();
});

it("LinhaDoTempo avisa quando a leitura falha e some na leitura boa", async () => {
  lerEventos.mockRejectedValueOnce(new Error("fora")).mockResolvedValue([ev(1, "um")]);
  render(<LinhaDoTempo execucaoId="x" viva />);
  await passa(0);
  expect(screen.getByRole("status")).toHaveTextContent("Não foi possível atualizar a linha do tempo");
  await passa(1500);
  expect(screen.queryByRole("status")).toBeNull();
});

it("LinhaDoTempo de execução terminada repete a leitura enquanto vier página cheia", async () => {
  const pagina = Array.from({ length: 500 }, (_, i) => ev(i + 1, `e${i + 1}`));
  lerEventos.mockResolvedValueOnce(pagina).mockResolvedValueOnce([ev(501, "último")]).mockResolvedValue([]);
  render(<LinhaDoTempo execucaoId="x" viva={false} />);
  await passa(0);
  expect(lerEventos).toHaveBeenNthCalledWith(2, "x", 500);
  expect(lerEventos).toHaveBeenCalledTimes(2);
  expect(screen.getByText("último")).toBeInTheDocument();
});

it("LinhaDoTempo cursor reinicia ao trocar de execução", async () => {
  lerEventos.mockResolvedValue([ev(9, "nove")]);
  const { rerender } = render(<LinhaDoTempo execucaoId="x" viva />);
  await passa(0);
  rerender(<LinhaDoTempo execucaoId="y" viva />);
  await passa(0);
  expect(lerEventos).toHaveBeenLastCalledWith("y", 0);
});

it("LinhaDoTempo para de consultar ao desmontar", async () => {
  lerEventos.mockResolvedValue([]);
  const { unmount } = render(<LinhaDoTempo execucaoId="x" viva />);
  await passa(0);
  unmount();
  const n = lerEventos.mock.calls.length;
  await passa(10000);
  expect(lerEventos.mock.calls.length).toBe(n);
});
