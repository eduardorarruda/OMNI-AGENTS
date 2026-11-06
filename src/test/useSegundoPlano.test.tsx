import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useSegundoPlano } from "../features/segundoPlano/useSegundoPlano";

vi.mock("../features/terminal/terminalService", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../features/terminal/terminalService")>()),
  engineApi: vi.fn(),
}));
const { engineApi } = vi.mocked(await import("../features/terminal/terminalService"));

const resposta = { status: 200, body: JSON.stringify({ itens: [] }) } as Awaited<ReturnType<typeof engineApi>>;

async function passar(ms: number) {
  await act(async () => { await vi.advanceTimersByTimeAsync(ms); });
}

beforeEach(() => {
  vi.useFakeTimers();
  engineApi.mockReset();
  engineApi.mockResolvedValue(resposta);
});
afterEach(() => vi.useRealTimers());

it("consulta uma vez ao montar e uma a cada 4 s, sem laço entre as renderizações", async () => {
  renderHook(() => useSegundoPlano("p"));
  await passar(0);
  expect(engineApi).toHaveBeenCalledTimes(1);
  await passar(3900);
  expect(engineApi).toHaveBeenCalledTimes(1);
  await passar(200);
  expect(engineApi).toHaveBeenCalledTimes(2);
  await passar(4000);
  expect(engineApi).toHaveBeenCalledTimes(3);
});

it("depois de desmontar não consulta mais", async () => {
  const { unmount } = renderHook(() => useSegundoPlano("p"));
  await passar(0);
  unmount();
  engineApi.mockClear();
  await passar(20_000);
  expect(engineApi).not.toHaveBeenCalled();
});

it("dois assinantes do mesmo projeto dividem a mesma consulta", async () => {
  renderHook(() => useSegundoPlano("p"));
  renderHook(() => useSegundoPlano("p"));
  await passar(0);
  expect(engineApi).toHaveBeenCalledTimes(1);
  await passar(4000);
  expect(engineApi).toHaveBeenCalledTimes(2);
});
