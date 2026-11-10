import { describe, expect, it, vi } from "vitest";
import { sessoesAoFechar } from "../features/workspace/fecharProjetos";
import type { TerminalSession } from "../features/terminal/terminalService";
import type { Project } from "../types/workspace";

const projeto = (id: string, abas: string[]): Project => ({
  id, name: id, path: `/p/${id}`, gitRoot: null, branch: null, composeFile: null, lastOpenedAt: "",
  layout: { type: "pane", id: `pane-${id}`, activeTabId: null, tabs: abas.map((resourceId) => ({ id: `t-${resourceId}`, kind: "agent" as const, title: resourceId, resourceId })) },
  activePaneId: `pane-${id}`, maximizedPaneId: null,
} as Project);

const sessao = (id: string, project_id: string, extra: Partial<TerminalSession> = {}): TerminalSession => ({
  id, name: id, provider: "claude", project_id, cwd: "/p", shell: "bash", state: "answered", pid: 1, output_seq: 0, rows: 24, cols: 80, ...extra,
});

/** Fechar a aba deixa o agente vivo; fechar o projeto não pode deixá-lo rodando sem dono. */
describe("fechar projeto ou workspace com agente em segundo plano", () => {
  it("sem agente fora das abas, fecha como antes e não pergunta", () => {
    const confirmar = vi.fn(() => true);
    const ids = sessoesAoFechar([projeto("a", ["s1"])], [], [sessao("s1", "a"), sessao("parada", "a", { state: "stopped" })], confirmar);
    expect(ids).toEqual(["s1"]);
    expect(confirmar).not.toHaveBeenCalled();
  });

  it("fechar projeto: com agente sem aba, pergunta uma vez e, aceito, encerra todos", () => {
    const confirmar = vi.fn(() => true);
    const ids = sessoesAoFechar([projeto("a", ["s1"])], [], [sessao("s1", "a"), sessao("s2", "a"), sessao("s3", "a", { provider: null }), sessao("x", "b")], confirmar);
    expect(confirmar).toHaveBeenCalledTimes(1);
    expect(confirmar).toHaveBeenCalledWith("Há 1 agente rodando em segundo plano neste projeto. Encerrar junto?");
    expect(ids).toEqual(["s1", "s2", "s3"]);
  });

  it("recusado, não fecha nada", () => {
    expect(sessoesAoFechar([projeto("a", [])], [], [sessao("s2", "a")], () => false)).toBeNull();
  });

  it("remover do workspace: sessão aberta em aba de outro projeto não conta", () => {
    const confirmar = vi.fn(() => true);
    expect(sessoesAoFechar([projeto("a", [])], [projeto("b", ["s2"])], [sessao("s2", "a")], confirmar)).toEqual([]);
    expect(confirmar).not.toHaveBeenCalled();
  });

  it("apagar workspace: soma os agentes de todos os projetos dele numa pergunta só", () => {
    const confirmar = vi.fn(() => true);
    const ids = sessoesAoFechar([projeto("a", ["s1"]), projeto("b", [])], [], [sessao("s1", "a"), sessao("s2", "a"), sessao("s3", "b")], confirmar);
    expect(confirmar).toHaveBeenCalledWith("Há 2 agentes rodando em segundo plano neste projeto. Encerrar junto?");
    expect(ids).toEqual(["s1", "s2", "s3"]);
  });
});
