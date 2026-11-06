import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { engineApi } from "../features/terminal/terminalService";
import { emptyKanban, kanbanReducer } from "../features/kanban/kanbanReducer";
import { useKanban } from "../features/kanban/useKanban";
import { avisoDoKanban, limparAvisoDoKanban } from "../features/kanban/kanbanAviso";
import {
  KANBAN_STORAGE_KEY,
  aplicarNoEngine,
  carregarKanban,
  migrarSePreciso,
} from "../services/kanbanService";
import type { KanbanState } from "../types/kanban";

vi.mock("../features/terminal/terminalService", () => ({ engineApi: vi.fn() }));

const api = vi.mocked(engineApi);

function cartao(id: string, extra: Record<string, unknown> = {}) {
  return {
    id, projectId: "p", title: `T ${id}`, description: "", status: "backlog", position: 1,
    loopTotal: null, loopRemaining: null, resultNote: null,
    createdAt: "2026-11-06T00:00:00Z", updatedAt: "2026-11-06T00:00:00Z", ...extra,
  };
}

/** Engine de mentira em memória, com as mesmas rotas do real. */
function engineFalso(inicial: { tarefas?: unknown[]; ajustes?: Record<string, unknown> } = {}) {
  const quadro = { versao: 1, tarefas: [...(inicial.tarefas ?? [])] as Record<string, unknown>[], triagem: [], ajustes: { ...(inicial.ajustes ?? {}) } };
  const chamadas: { metodo: string; caminho: string; corpo: unknown }[] = [];
  api.mockImplementation(async (metodo, caminho, corpo) => {
    const json = corpo ? JSON.parse(corpo) : null;
    chamadas.push({ metodo, caminho, corpo: json });
    const ok = (body: unknown, status = 200) => ({ type: "api" as const, status, body: JSON.stringify(body) });
    if (metodo === "GET" && caminho === "/kanban") return ok(quadro);
    if (metodo === "POST" && caminho === "/kanban/cartoes") { quadro.tarefas.push(json); return ok(json, 201); }
    if (metodo === "POST" && caminho === "/kanban/migrar") {
      const vazio = quadro.tarefas.length === 0 && Object.keys(quadro.ajustes).length === 0;
      if (vazio) { quadro.tarefas.push(...json.tarefas); Object.assign(quadro.ajustes, json.ajustes); }
      return ok({ migrou: vazio, quadro });
    }
    const m = caminho.match(/^\/kanban\/cartoes\/(.+)$/);
    if (m && metodo === "PATCH") {
      const alvo = quadro.tarefas.find((c) => c.id === m[1]);
      if (!alvo) return ok({ error: "x" }, 404);
      Object.assign(alvo, json);
      return ok(alvo);
    }
    if (m && metodo === "DELETE") { quadro.tarefas = quadro.tarefas.filter((c) => c.id !== m[1]); return ok({ removido: true }); }
    if (metodo === "PATCH" && caminho === "/kanban/ajustes") { Object.assign(quadro.ajustes, json); return ok(quadro.ajustes); }
    return ok({ error: "rota" }, 404);
  });
  return { quadro, chamadas };
}

const antigo = { version: 1, tasks: [cartao("a"), cartao("b")], agentByProject: { p: "claude" }, dispatcherEnabled: true, failureTimestamps: [], pausedUntil: null };

beforeEach(() => {
  localStorage.clear();
  api.mockReset();
  limparAvisoDoKanban();
});

describe("migração do Kanban", () => {
  it("migra o localStorage para o engine vazio", async () => {
    localStorage.setItem(KANBAN_STORAGE_KEY, JSON.stringify(antigo));
    const { quadro } = engineFalso();
    expect(await migrarSePreciso()).toBe("migrou");
    expect(quadro.tarefas.map((c) => c.id)).toEqual(["a", "b"]);
    expect(quadro.ajustes).toMatchObject({ dispatcherEnabled: true, agentByProject: { p: "claude" } });
  });

  it("mantém a chave antiga depois de migrar", async () => {
    const bruto = JSON.stringify(antigo);
    localStorage.setItem(KANBAN_STORAGE_KEY, bruto);
    engineFalso();
    await migrarSePreciso();
    expect(localStorage.getItem(KANBAN_STORAGE_KEY)).toBe(bruto);
  });

  it("engine fora do ar não apaga nada e avisa", async () => {
    const bruto = JSON.stringify(antigo);
    localStorage.setItem(KANBAN_STORAGE_KEY, bruto);
    api.mockRejectedValue(new Error("engine fora do ar"));
    expect(await migrarSePreciso()).toBe("falhou");
    expect(localStorage.getItem(KANBAN_STORAGE_KEY)).toBe(bruto);
    // sem engine, a tela ainda mostra os cartões antigos e avisa
    const estado = await carregarKanban();
    expect(estado.tasks).toHaveLength(2);
    expect(avisoDoKanban()).toMatch(/engine/i);
    expect(localStorage.getItem(KANBAN_STORAGE_KEY)).toBe(bruto);
  });

  it("localStorage corrompido não trava e não migra lixo", async () => {
    localStorage.setItem(KANBAN_STORAGE_KEY, "{ isto nao e json");
    const { chamadas } = engineFalso();
    expect(await migrarSePreciso()).toBe("nada");
    expect(chamadas.find((c) => c.caminho === "/kanban/migrar")).toBeUndefined();
    expect(localStorage.getItem(KANBAN_STORAGE_KEY)).toBe("{ isto nao e json");
    expect((await carregarKanban()).tasks).toEqual([]);
  });

  it("não migra de novo com quadro cheio", async () => {
    localStorage.setItem(KANBAN_STORAGE_KEY, JSON.stringify(antigo));
    const { quadro, chamadas } = engineFalso({ tarefas: [cartao("so-no-engine")] });
    const estado = await carregarKanban();
    expect(estado.tasks.map((t) => t.id)).toEqual(["so-no-engine"]);
    expect(chamadas.find((c) => c.caminho === "/kanban/migrar")).toBeUndefined();
    expect(await migrarSePreciso()).toBe("nada");
    expect(quadro.tarefas.map((c) => c.id)).toEqual(["so-no-engine"]);
  });
});

describe("gravação por cartão", () => {
  it("cada ação vira uma operação de um cartão", async () => {
    const { chamadas } = engineFalso();
    let estado: KanbanState = emptyKanban();
    const passo = async (acao: Parameters<typeof kanbanReducer>[1]) => {
      const antes = estado;
      estado = kanbanReducer(antes, acao);
      await aplicarNoEngine(antes, estado);
    };
    await passo({ type: "CREATE_TASK", projectId: "p", title: "Um", description: "" });
    await passo({ type: "CREATE_TASK", projectId: "p", title: "Dois", description: "" });
    const [um, dois] = estado.tasks;
    chamadas.length = 0;
    await passo({ type: "MOVE_TASK", id: um.id, status: "ready" });
    expect(chamadas).toHaveLength(1);
    expect(chamadas[0]).toMatchObject({ metodo: "PATCH", caminho: `/kanban/cartoes/${um.id}` });
    expect(chamadas[0].corpo).toMatchObject({ status: "ready" });
    chamadas.length = 0;
    await passo({ type: "REMOVE_TASK", id: dois.id });
    expect(chamadas).toEqual([{ metodo: "DELETE", caminho: `/kanban/cartoes/${dois.id}`, corpo: null }]);
    chamadas.length = 0;
    await passo({ type: "SET_DISPATCHER_ENABLED", enabled: true });
    expect(chamadas).toHaveLength(1);
    expect(chamadas[0]).toMatchObject({ metodo: "PATCH", caminho: "/kanban/ajustes", corpo: { dispatcherEnabled: true } });
  });

  it("falha ao gravar desfaz na tela e mostra erro", async () => {
    const { quadro } = engineFalso();
    const { result } = renderHook(() => useKanban());
    await waitFor(() => expect(result.current.carregado).toBe(true));
    act(() => result.current.dispatch({ type: "CREATE_TASK", projectId: "p", title: "Boa", description: "" }));
    await waitFor(() => expect(quadro.tarefas).toHaveLength(1));
    const antes = api.getMockImplementation()!;
    api.mockImplementation(async (metodo, caminho, corpo, extra) => {
      if (metodo === "POST" && caminho === "/kanban/cartoes") throw new Error("disco cheio");
      return antes(metodo, caminho, corpo, extra);
    });
    act(() => result.current.dispatch({ type: "CREATE_TASK", projectId: "p", title: "Ruim", description: "" }));
    expect(result.current.kanban.tasks).toHaveLength(2); // aparece na hora
    await waitFor(() => expect(result.current.kanban.tasks).toHaveLength(1)); // e é desfeito
    expect(result.current.kanban.tasks[0].title).toBe("Boa");
    expect(avisoDoKanban()).toMatch(/não foi possível/i);
  });
});
