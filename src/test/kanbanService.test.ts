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
  carregarKanbanComOrigem,
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
function engineFalso(inicial: { tarefas?: unknown[]; ajustes?: Record<string, unknown>; migrado?: boolean } = {}) {
  const quadro = { versao: 1, tarefas: [...(inicial.tarefas ?? [])] as Record<string, unknown>[], triagem: [], migrado: inicial.migrado ?? false, ajustes: { ...(inicial.ajustes ?? {}) } };
  const chamadas: { metodo: string; caminho: string; corpo: unknown }[] = [];
  api.mockImplementation(async (metodo, caminho, corpo) => {
    const json = corpo ? JSON.parse(corpo) : null;
    chamadas.push({ metodo, caminho, corpo: json });
    const ok = (body: unknown, status = 200) => ({ type: "api" as const, status, body: JSON.stringify(body) });
    if (metodo === "GET" && caminho === "/kanban") return ok(quadro);
    if (metodo === "POST" && caminho === "/kanban/cartoes") { quadro.tarefas.push(json); quadro.migrado = true; return ok(json, 201); }
    if (metodo === "POST" && caminho === "/kanban/migrar") {
      const vazio = !quadro.migrado && quadro.tarefas.length === 0 && Object.keys(quadro.ajustes).length === 0;
      if (vazio) { quadro.tarefas.push(...json.tarefas); Object.assign(quadro.ajustes, json.ajustes); quadro.migrado = true; }
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

describe("migração que não pode perder nem ressuscitar", () => {
  it("apagar todos os cartões depois de migrar não traz a cópia antiga de volta", async () => {
    localStorage.setItem(KANBAN_STORAGE_KEY, JSON.stringify(antigo));
    const { chamadas } = engineFalso({ tarefas: [], migrado: true });
    expect((await carregarKanban()).tasks).toEqual([]);
    expect(await migrarSePreciso()).toBe("nada");
    expect(chamadas.find((c) => c.caminho === "/kanban/migrar")).toBeUndefined();
  });

  it("migração que falha com o engine de pé e vazio mostra a cópia local, avisa e não aceita cartão novo", async () => {
    localStorage.setItem(KANBAN_STORAGE_KEY, JSON.stringify(antigo));
    const { quadro, chamadas } = engineFalso();
    const normal = api.getMockImplementation()!;
    api.mockImplementation(async (metodo, caminho, corpo, extra) => {
      if (caminho === "/kanban/migrar") throw new Error("queda no meio");
      return normal(metodo, caminho, corpo, extra);
    });
    const { estado, origem } = await carregarKanbanComOrigem();
    expect(origem).toBe("local");
    expect(estado.tasks.map((t) => t.id)).toEqual(["a", "b"]);
    expect(avisoDoKanban()).toMatch(/migra/i);

    const { result } = renderHook(() => useKanban());
    await waitFor(() => expect(result.current.carregado).toBe(true));
    expect(result.current.kanban.tasks).toHaveLength(2);
    act(() => result.current.dispatch({ type: "CREATE_TASK", projectId: "p", title: "Nova", description: "" }));
    await waitFor(() => expect(avisoDoKanban()).toMatch(/não foi possível/i));
    await waitFor(() => expect(result.current.kanban.tasks).toHaveLength(2));
    expect(quadro.tarefas).toHaveLength(0);
    expect(chamadas.find((c) => c.caminho === "/kanban/cartoes")).toBeUndefined();
    expect(localStorage.getItem(KANBAN_STORAGE_KEY)).toBe(JSON.stringify(antigo));
  });
});

describe("gravação por cartão", () => {
  it("falha na primeira de duas ações em voo descarta a segunda e relê o engine uma vez", async () => {
    const { quadro, chamadas } = engineFalso({ tarefas: [cartao("base")], migrado: true });
    const { result } = renderHook(() => useKanban());
    await waitFor(() => expect(result.current.carregado).toBe(true));
    const normal = api.getMockImplementation()!;
    api.mockImplementation(async (metodo, caminho, corpo, extra) => {
      if (metodo === "POST" && caminho === "/kanban/cartoes" && corpo?.includes("Ruim1")) throw new Error("disco cheio");
      return normal(metodo, caminho, corpo, extra);
    });
    chamadas.length = 0;
    act(() => {
      result.current.dispatch({ type: "CREATE_TASK", projectId: "p", title: "Ruim1", description: "" });
      result.current.dispatch({ type: "CREATE_TASK", projectId: "p", title: "Ruim2", description: "" });
    });
    expect(result.current.kanban.tasks).toHaveLength(3);
    await waitFor(() => expect(result.current.kanban.tasks).toHaveLength(1));
    expect(result.current.kanban.tasks[0].id).toBe("base");
    expect(quadro.tarefas).toHaveLength(1);
    expect(chamadas.filter((c) => c.metodo === "POST")).toHaveLength(0); // a primeira caiu antes de chegar; a segunda nem foi tentada
    expect(chamadas.filter((c) => c.metodo === "GET" && c.caminho === "/kanban")).toHaveLength(1); // relê uma vez
    expect(avisoDoKanban()).toMatch(/não gravadas|desfeit/i);
  });

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

describe("mudança feita fora da tela", () => {
  it("a tela vê o cartão que o agente pegou pelo MCP", async () => {
    const { quadro } = engineFalso({ tarefas: [cartao("a", { status: "ready" })], migrado: true });
    const { result } = renderHook(() => useKanban());
    await waitFor(() => expect(result.current.carregado).toBe(true));
    Object.assign(quadro.tarefas[0], { status: "doing", pegoPor: "mcp" });
    await waitFor(() => expect(result.current.kanban.tasks[0].status).toBe("doing"), { timeout: 8_000 });
    expect(result.current.kanban.tasks[0].pegoPor).toBe("mcp");
  }, 15_000);

  it("releitura que saiu antes de um gesto não desfaz o gesto", async () => {
    const { quadro } = engineFalso({ tarefas: [cartao("a")], migrado: true });
    const { result } = renderHook(() => useKanban());
    await waitFor(() => expect(result.current.carregado).toBe(true));
    const normal = api.getMockImplementation()!;
    const velho = JSON.stringify(quadro);
    let soltar: (() => void) | undefined;
    api.mockImplementation(async (metodo, caminho, corpo, extra) => {
      if (metodo === "GET" && caminho === "/kanban" && !soltar) {
        await new Promise<void>((resolve) => { soltar = resolve; });
        return { type: "api" as const, status: 200, body: velho };
      }
      return normal(metodo, caminho, corpo, extra);
    });
    await waitFor(() => expect(soltar).toBeDefined(), { timeout: 8_000 });
    act(() => result.current.dispatch({ type: "MOVE_TASK", id: "a", status: "ready" }));
    await waitFor(() => expect(quadro.tarefas[0].status).toBe("ready"));
    await act(async () => {
      soltar?.();
      await Promise.resolve();
    });
    expect(result.current.kanban.tasks[0].status).toBe("ready");
  }, 15_000);
});
