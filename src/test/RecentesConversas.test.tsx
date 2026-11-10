import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { conversasDoProjeto, escolherRecentes, juntarComIndice, RecentesConversas } from "../features/history/RecentesConversas";
import { ProjetosContext } from "../features/projects/projetosContext";
import { createProject } from "../features/workspace/workspaceReducer";
import type { HistoryEntry } from "../features/history/historyService";
import type { Conversation } from "../features/terminal/terminalService";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const entrada = (session_id: string, updated_at_ms: number, extra: Partial<HistoryEntry> = {}): HistoryEntry => ({
  provider: "claude", profile_id: "p", profile_name: "Pessoal", session_id, title: `Conversa ${session_id}`,
  first_prompt: null, cwd: "C:/proj/omni", started_at_ms: null, updated_at_ms, path: `C:/h/${session_id}.jsonl`, ...extra,
});

describe("Conversas do projeto", () => {
  it("lista todas as conversas da pasta do projeto, sem limite, sem as abertas e sem as de outros projetos", () => {
    const daqui = Array.from({ length: 30 }, (_, i) => entrada(`d${i}`, i, { cwd: "/p/central" }));
    const lista = conversasDoProjeto([...daqui, entrada("fora", 99, { cwd: "/p/outro" })], new Set(["d0"]), "/p/central", ["/p/central", "/p/outro"]);
    expect(lista).toHaveLength(29);
    expect(lista[0].session_id).toBe("d29");
    expect(lista.some((item) => item.session_id === "fora")).toBe(false);
  });

  it("worktree conta como do projeto, e subprojeto aberto fica com o subprojeto", () => {
    const entradas = [entrada("w", 1, { cwd: "/p/central/.claude/worktrees/x" }), entrada("s", 2, { cwd: "/p/central/sub" })];
    const lista = conversasDoProjeto(entradas, new Set(), "/p/central", ["/p/central", "/p/central/sub"]);
    expect(lista.map((item) => item.session_id)).toEqual(["w"]);
  });
});

describe("Recentes na lateral", () => {
  it("as mais novas primeiro, sem as abertas e sem as que não sabem a pasta", () => {
    const lista = escolherRecentes([entrada("a", 1), entrada("b", 3), entrada("c", 2), entrada("d", 9, { cwd: null })], new Set(["c"]));
    expect(lista.map((item) => item.session_id)).toEqual(["b", "a"]);
  });

  /** "Ver as principais conversas para poder usar as mesmas, um mini histórico, com os ícones dos projetos." */
  it("mostra o projeto de cada uma e retoma no clique", async () => {
    vi.mocked(invoke).mockImplementation(async (comando: string, args?: unknown) =>
      comando === "agent_history" && (args as { provider: string }).provider === "claude"
        ? [entrada("a", Date.now() - 60_000), entrada("b", Date.now() - 120_000, { cwd: "C:/proj/site" })] : []);
    const onRetomar = vi.fn(async () => undefined);
    render(
      <ProjetosContext.Provider value={[{ ...createProject("C:/proj/omni", "p1"), name: "OMNI" }]}>
        <RecentesConversas abertas={[]} agora={Date.now()} onRetomar={onRetomar} onVerTodas={vi.fn()} />
      </ProjetosContext.Provider>,
    );
    const regiao = await screen.findByRole("region", { name: "Conversas recentes" });
    const linha = await within(regiao).findByRole("button", { name: "Retomar Conversa a (OMNI)" });
    expect(within(regiao).getByRole("button", { name: "Retomar Conversa b (site)" })).toBeInTheDocument();
    fireEvent.click(linha);
    expect(onRetomar).toHaveBeenCalledWith(expect.objectContaining({ session_id: "a" }));

    // Busca ali mesmo, sem abrir o histórico completo.
    fireEvent.change(within(regiao).getByRole("searchbox", { name: "Buscar nas conversas recentes" }), { target: { value: "site" } });
    expect(within(regiao).queryByRole("button", { name: "Retomar Conversa a (OMNI)" })).toBeNull();
    expect(within(regiao).getByRole("button", { name: "Retomar Conversa b (site)" })).toBeInTheDocument();
  });
});

const doIndice = (id: string, externo: string | null, extra: Partial<Conversation> = {}): Conversation => ({
  id, project_id: "p1", cwd: "/p/central", title: `Índice ${id}`, title_source: "auto", created_at_ms: 5,
  segments: [{ provider: "claude", profile_id: "conta", external_session_id: externo, transcript_path: null, terminal_session_id: null, started_at_ms: 7, ended_at_ms: null }],
  ...extra,
});

/** "A conversa não fica ali em Conversas do projeto": o índice do OMNI entra na lista junto do histórico das CLIs. */
describe("Conversas do projeto com o índice do OMNI", () => {
  it("conversa só no índice aparece; a que está nos dois não se repete", () => {
    const lista = juntarComIndice([entrada("x", 9, { cwd: "/p/central" })], [doIndice("c1", "x"), doIndice("c2", "y")]);
    expect(lista.map((item) => item.session_id).sort()).toEqual(["x", "y"]);
    const y = lista.find((item) => item.session_id === "y")!;
    expect(y).toMatchObject({ provider: "claude", profile_id: "conta", cwd: "/p/central", title: "Índice c2", updated_at_ms: 7 });
  });

  it("vale o segmento mais novo e ignora conversa sem sessão nativa nenhuma", () => {
    const duasContas = doIndice("c1", "velha", {
      segments: [
        { provider: "claude", profile_id: "a", external_session_id: "velha", transcript_path: null, terminal_session_id: null, started_at_ms: 1, ended_at_ms: 2 },
        { provider: "codex", profile_id: "b", external_session_id: "nova", transcript_path: null, terminal_session_id: null, started_at_ms: 3, ended_at_ms: null },
      ],
    });
    const lista = juntarComIndice([], [duasContas, doIndice("vazia", null)]);
    expect(lista).toHaveLength(1);
    expect(lista[0]).toMatchObject({ session_id: "nova", provider: "codex", profile_id: "b" });
  });

  it("qualquer segmento já no histórico basta para não duplicar", () => {
    const conversa = doIndice("c1", "a", {
      segments: [
        { provider: "claude", profile_id: "a", external_session_id: "a", transcript_path: null, terminal_session_id: null, started_at_ms: 1, ended_at_ms: 2 },
        { provider: "claude", profile_id: "a", external_session_id: "b", transcript_path: null, terminal_session_id: null, started_at_ms: 3, ended_at_ms: null },
      ],
    });
    expect(juntarComIndice([entrada("a", 1, { cwd: "/p/central" })], [conversa]).map((item) => item.session_id)).toEqual(["a"]);
  });

  it("a lista lateral do projeto mostra a conversa que só existe no índice", async () => {
    vi.mocked(invoke).mockImplementation(async (comando: string) => {
      if (comando === "list_conversations") return [doIndice("c9", "so-no-indice", { title: "Sem transcript ainda" })];
      return [];
    });
    render(
      <ProjetosContext.Provider value={[{ ...createProject("/p/central", "p1"), name: "Central" }]}>
        <RecentesConversas abertas={[]} agora={Date.now()} onRetomar={vi.fn(async () => undefined)} onVerTodas={vi.fn()}
          projeto="/p/central" projetos={["/p/central"]} />
      </ProjetosContext.Provider>,
    );
    expect(await screen.findByRole("button", { name: "Retomar Sem transcript ainda (Central)" })).toBeInTheDocument();
  });
});
