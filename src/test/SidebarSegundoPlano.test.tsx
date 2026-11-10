import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Sidebar } from "../components/Sidebar";
import type { Project, WorkspaceState } from "../types/workspace";
import type { TerminalSession } from "../features/terminal/terminalService";

vi.mock("../components/FileTree", () => ({ FileTree: () => null }));
vi.mock("../components/GitPanel", () => ({ GitPanel: () => null }));
vi.mock("../components/SkillsList", () => ({ SkillsList: () => null }));
vi.mock("../components/KanbanPanel", () => ({ KanbanPanel: () => null }));
vi.mock("../components/DockerPanel", () => ({ DockerPanel: () => null }));
vi.mock("../hooks/useNotifications", () => ({ useNotifications: () => [] }));
vi.mock("../features/segundoPlano/useSegundoPlano", () => ({ useSegundoPlano: vi.fn(() => ({ itens: [], erro: null })) }));
vi.mock("../features/segundoPlano/SegundoPlanoPanel", () => ({ SegundoPlanoPanel: () => null }));

function workspace(id: string, name: string): WorkspaceState {
  return { version: 1, id, name, projects: [], activeProjectId: null, updatedAt: "" };
}


function renderSidebar(overrides: Partial<React.ComponentProps<typeof Sidebar>> = {}) {
  const props: React.ComponentProps<typeof Sidebar> = {
    workspaces: [workspace("ws-a", "Cliente X"), workspace("ws-b", "Estudos")],
    activeWorkspaceId: "ws-b",
    onSelectWorkspace: vi.fn(),
    onCreateWorkspace: vi.fn(),
    onHideWorkspace: vi.fn(),
    onShowWorkspace: vi.fn(),
    onDeleteWorkspace: vi.fn(),
    onMoveWorkspace: vi.fn(),
    onRemoveProjectFromWorkspace: vi.fn(),
    onRenameWorkspace: vi.fn(),
    attention: [],
    onFocusSession: vi.fn(),
    onDismissAttention: vi.fn(),
    onRunInTerminal: vi.fn(),
    projects: [],
    activeProjectId: null,
    activeProjectPath: null,
    onOpenFile: vi.fn(),
    onFileRenamed: vi.fn(),
    onPathDeleted: vi.fn(),
    terminalSessions: [],
    showSettings: false,
    showHistory: false,
    onOpenHistory: vi.fn(),
    onHome: vi.fn(),
    onAddProject: vi.fn(),
    onSelectProject: vi.fn(),
    onCloseProject: vi.fn(),
    onMoveProject: vi.fn(),
    onAttachTerminal: vi.fn(),
    onCloseTerminal: vi.fn(),
    onDuplicateTerminal: vi.fn(),
    onRestartTerminal: vi.fn(),
    onOpenSettings: vi.fn(),
    onOpenGitDiff: vi.fn(),
    onOpenGitGraph: vi.fn(),
    kanban: {
      version: 1,
      tasks: [],
      agentByProject: {},
      dispatcherEnabled: false,
      failureTimestamps: [],
      pausedUntil: null,
    },
    onOpenKanban: vi.fn(),
    ...overrides,
  };
  render(<Sidebar {...props} />);
  return props;
}

const projeto = (tabs: Project["layout"]): Project => ({
  id: "p", name: "api", path: "C:/p", gitRoot: null, branch: null, composeFile: null, lastOpenedAt: "",
  layout: tabs, activePaneId: "pane", maximizedPaneId: null,
} as Project);

const conversa = (id: string, name: string): TerminalSession => ({
  id, name, provider: "claude", project_id: "p", cwd: "C:/p", shell: "bash", state: "answered", pid: 1, output_seq: 0, rows: 24, cols: 80,
});

/** Fechar a aba não encerra mais o agente: a lateral diz quem segue rodando sem aba. */
describe("conversa viva sem aba", () => {
  it("marca em segundo plano só a conversa que não tem aba aberta", () => {
    renderSidebar({
      activeProjectId: "p", activeProjectPath: "C:/p",
      projects: [projeto({ type: "pane", id: "pane", activeTabId: "t", tabs: [{ id: "t", kind: "agent", title: "Com aba", resourceId: "s1" }] })],
      terminalSessions: [conversa("s1", "Com aba"), conversa("s2", "Sem aba")],
    });
    const semAba = screen.getByRole("button", { name: /^Sem aba/ });
    expect(within(semAba).getByText(/em segundo plano/)).toBeInTheDocument();
    expect(within(screen.getByRole("button", { name: /^Com aba/ })).queryByText(/em segundo plano/)).toBeNull();
  });

  it("encerrar o agente pela lateral pede confirmação", async () => {
    const confirmar = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
    const props = renderSidebar({ activeProjectId: "p", activeProjectPath: "C:/p", terminalSessions: [conversa("s2", "Sem aba")] });
    await userEvent.click(screen.getByRole("button", { name: "Encerrar agente Sem aba" }));
    expect(props.onCloseTerminal).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Encerrar agente Sem aba" }));
    expect(props.onCloseTerminal).toHaveBeenCalledWith("s2");
    expect(confirmar).toHaveBeenCalledTimes(2);
    confirmar.mockRestore();
  });
});
