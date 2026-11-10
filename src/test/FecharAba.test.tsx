import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PaneView } from "../features/workspace/PaneView";
import type { PaneNode } from "../types/workspace";

const servico = vi.hoisted(() => ({ closeTerminal: vi.fn(async () => undefined) }));
vi.mock("../features/terminal/terminalService", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../features/terminal/terminalService")>()),
  closeTerminal: servico.closeTerminal,
}));
vi.mock("../features/terminal/TerminalPane", () => ({ TerminalPane: () => <div>Terminal ativo</div> }));
vi.mock("../features/conversa/AbaDeAgente", () => ({ AbaDeAgente: ({ children }: { children: React.ReactNode }) => <>{children}</> }));

const KANBAN = { version: 1 as const, tasks: [], agentByProject: {}, dispatcherEnabled: false, failureTimestamps: [], pausedUntil: null };

function montar(kind: "agent" | "terminal", outras: PaneNode["tabs"] = []) {
  const dispatch = vi.fn();
  const pane: PaneNode = { type: "pane", id: "p", activeTabId: "t", tabs: [{ id: "t", kind, title: "Claude", resourceId: "s1" }, ...outras] };
  render(<PaneView pane={pane} active onlyPane maximized={false} projectId="project" projectPath="/p" fileSaveMode="auto"
    agentView="terminal" kanban={KANBAN} kanbanDispatch={vi.fn()} dispatch={dispatch} />);
  return dispatch;
}

afterEach(() => { vi.restoreAllMocks(); servico.closeTerminal.mockClear(); });

/** "Aba é uma coisa, conversa é outra": o X da aba só tira a aba da tela; o agente segue vivo. */
describe("fechar a aba de agente", () => {
  it("o X fecha só a aba, sem encerrar a sessão", async () => {
    const dispatch = montar("agent");
    fireEvent.click(screen.getByRole("button", { name: "Fechar tab Claude" }));
    await waitFor(() => expect(dispatch).toHaveBeenCalledWith({ type: "CLOSE_TAB", paneId: "p", tabId: "t" }));
    expect(servico.closeTerminal).not.toHaveBeenCalled();
  });

  it("o botão do meio também só esconde", async () => {
    const dispatch = montar("agent");
    fireEvent(screen.getByRole("tab", { name: /Claude/ }).parentElement!, new MouseEvent("auxclick", { bubbles: true, button: 1 }));
    await waitFor(() => expect(dispatch).toHaveBeenCalledWith({ type: "CLOSE_TAB", paneId: "p", tabId: "t" }));
    expect(servico.closeTerminal).not.toHaveBeenCalled();
  });

  it("terminal puro continua encerrando o shell ao fechar", async () => {
    const dispatch = montar("terminal");
    fireEvent.click(screen.getByRole("button", { name: "Fechar tab Claude" }));
    await waitFor(() => expect(dispatch).toHaveBeenCalledWith({ type: "CLOSE_TAB", paneId: "p", tabId: "t" }));
    expect(servico.closeTerminal).toHaveBeenCalledWith("s1");
  });

  it("Encerrar agente fica no menu da aba e pede confirmação", async () => {
    const confirmar = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
    const dispatch = montar("agent");
    fireEvent.contextMenu(screen.getByRole("tab", { name: /Claude/ }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Encerrar agente/ }));
    expect(confirmar).toHaveBeenCalledTimes(1);
    expect(servico.closeTerminal).not.toHaveBeenCalled();

    fireEvent.contextMenu(screen.getByRole("tab", { name: /Claude/ }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Encerrar agente/ }));
    await waitFor(() => expect(servico.closeTerminal).toHaveBeenCalledWith("s1"));
    await waitFor(() => expect(dispatch).toHaveBeenCalledWith({ type: "CLOSE_TAB", paneId: "p", tabId: "t" }));
  });

  it("o menu também fecha só a aba", async () => {
    const dispatch = montar("agent");
    fireEvent.contextMenu(screen.getByRole("tab", { name: /Claude/ }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Fechar aba/ }));
    await waitFor(() => expect(dispatch).toHaveBeenCalledWith({ type: "CLOSE_TAB", paneId: "p", tabId: "t" }));
    expect(servico.closeTerminal).not.toHaveBeenCalled();
  });
});
