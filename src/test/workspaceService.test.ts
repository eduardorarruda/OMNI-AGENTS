import { beforeEach, describe, expect, it } from "vitest";
import { WORKSPACES_STORAGE_KEY, WORKSPACE_STORAGE_KEY, loadWorkspace, loadWorkspaces } from "../services/workspaceService";
import type { LayoutNode, PaneKind, WorkspaceState } from "../types/workspace";

/** `Record` obriga: tipo de aba novo sem entrada aqui não compila, e entrada aqui passa pela carga abaixo. */
const TODOS_OS_KINDS: Record<PaneKind, true> = {
  agent: true,
  terminal: true,
  command: true,
  file: true,
  markdown: true,
  "git-diff": true,
  "git-graph": true,
  kanban: true,
  grafo: true,
  fluxos: true,
  "execucao-projeto": true,
  logs: true,
  "docker-logs": true,
};

function aba(id: string, kind: string, title = id) {
  return { id, kind, title } as { id: string; kind: PaneKind; title: string };
}

function workspaceCom(layout: LayoutNode, activePaneId = "pane"): WorkspaceState {
  return {
    version: 1,
    id: "w",
    name: "Trabalho",
    activeProjectId: "p",
    updatedAt: "2026-11-06T00:00:00.000Z",
    projects: [{
      id: "p", name: "Projeto", path: "/p", gitRoot: null, branch: null, composeFile: null,
      lastOpenedAt: "2026-11-06T00:00:00.000Z", layout, activePaneId, maximizedPaneId: null,
    }],
  };
}

function reabrir(...workspaces: WorkspaceState[]) {
  localStorage.setItem(WORKSPACES_STORAGE_KEY, JSON.stringify({ version: 1, workspaces, activeWorkspaceId: workspaces[0].id }));
  return loadWorkspaces();
}

beforeEach(() => localStorage.clear());

describe("reabrir o app com o layout salvo", () => {
  it.each(Object.keys(TODOS_OS_KINDS))("aba %s volta com o workspace", (kind) => {
    const salvo = workspaceCom({ type: "pane", id: "pane", tabs: [aba("t", kind)], activeTabId: "t" });
    expect(reabrir(salvo).workspaces).toEqual([salvo]);
  });

  it("layout com a aba O que roda volta inteiro", () => {
    const salvo = workspaceCom({
      type: "split", id: "s", direction: "horizontal", ratio: 0.5,
      first: { type: "pane", id: "pane", tabs: [aba("a", "agent"), aba("r", "execucao-projeto", "O que roda")], activeTabId: "r" },
      second: { type: "pane", id: "outro", tabs: [aba("k", "kanban")], activeTabId: "k" },
    });
    expect(reabrir(salvo)).toEqual({ version: 1, workspaces: [salvo], activeWorkspaceId: "w" });
  });
});

describe("aba de tipo desconhecido", () => {
  const paneCom = (activeTabId: string) => ({ type: "pane" as const, id: "pane", tabs: [aba("a", "agent"), aba("x", "vindo-do-futuro")], activeTabId });

  it("sai sozinha: o workspace e as outras abas ficam", () => {
    const { workspaces } = reabrir(workspaceCom(paneCom("a")));
    expect(workspaces[0].name).toBe("Trabalho");
    expect(workspaces[0].projects[0].layout).toEqual({ type: "pane", id: "pane", tabs: [aba("a", "agent")], activeTabId: "a" });
  });

  it("se era a aba ativa, a primeira que sobrou assume", () => {
    const { workspaces } = reabrir(workspaceCom(paneCom("x")));
    expect(workspaces[0].projects[0].layout).toMatchObject({ activeTabId: "a" });
  });

  it("painel que fica sem aba continua de pé e vazio", () => {
    const { workspaces } = reabrir(workspaceCom({ type: "pane", id: "pane", tabs: [aba("x", "vindo-do-futuro")], activeTabId: "x" }));
    expect(workspaces[0].projects[0].layout).toEqual({ type: "pane", id: "pane", tabs: [], activeTabId: null });
  });

  it("vale dentro de divisão de painéis", () => {
    const { workspaces } = reabrir(workspaceCom({
      type: "split", id: "s", direction: "vertical", ratio: 0.5,
      first: paneCom("a"),
      second: { type: "pane", id: "outro", tabs: [aba("y", "vindo-do-futuro"), aba("k", "kanban")], activeTabId: "y" },
    }));
    expect(workspaces[0].projects[0].layout).toMatchObject({
      first: { tabs: [aba("a", "agent")] },
      second: { tabs: [aba("k", "kanban")], activeTabId: "k" },
    });
  });

  it("vale também na chave antiga de um workspace só", () => {
    localStorage.setItem(WORKSPACE_STORAGE_KEY, JSON.stringify(workspaceCom(paneCom("a"))));
    expect(loadWorkspace().name).toBe("Trabalho");
  });

  it("estrutura quebrada de verdade continua descartando o workspace", () => {
    const quebrado = workspaceCom({ type: "pane", id: "pane", tabs: [aba("a", "agent")], activeTabId: "nao-existe" });
    const bom = { ...workspaceCom({ type: "pane", id: "pane", tabs: [aba("a", "agent")], activeTabId: "a" }), id: "w2" };
    expect(reabrir(quebrado, bom).workspaces.map((w) => w.id)).toEqual(["w2"]);
  });
});
