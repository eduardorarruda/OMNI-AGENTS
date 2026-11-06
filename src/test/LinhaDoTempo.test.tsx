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
