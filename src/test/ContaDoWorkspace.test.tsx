import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GitContaStatus } from "../features/git/GitBranchStatus";
import { ContaDoWorkspaceContext } from "../features/git/contaDoWorkspace";
import { workspacesReducer } from "../features/workspace/workspacesReducer";
import type { WorkspaceCollection } from "../types/workspace";

vi.mock("../features/git/gitService", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../features/git/gitService")>()),
  gitConta: vi.fn(),
  ghContas: vi.fn(),
}));

const { gitConta, ghContas } = vi.mocked(await import("../features/git/gitService"));

function rodape(conta: string | null, escolher = vi.fn()) {
  render(
    <ContaDoWorkspaceContext.Provider value={{ conta, escolher }}>
      <GitContaStatus projectPath="/dev/projeto" />
    </ContaDoWorkspaceContext.Provider>
  );
  return escolher;
}

beforeEach(() => {
  vi.clearAllMocks();
  gitConta.mockResolvedValue({ nome: "Pessoa", email_configurado: true, github: "conta-a", gh_instalado: true, problema: null });
  ghContas.mockResolvedValue(["conta-a", "conta-b"]);
});

describe("conta do GitHub no rodapé", () => {
  it("sem escolha mostra a conta ativa do gh", async () => {
    rodape(null);
    expect(await screen.findByRole("button", { name: /conta-a/ })).toBeInTheDocument();
  });

  it("mostra a conta do workspace no lugar da ativa", async () => {
    rodape("conta-b");
    expect(await screen.findByRole("button", { name: /conta-b/ })).toBeInTheDocument();
    expect(screen.queryByText("conta-a")).not.toBeInTheDocument();
  });

  it("clicar abre as contas e escolher grava no workspace", async () => {
    const escolher = rodape(null);
    await userEvent.click(await screen.findByRole("button", { name: /conta-a/ }));
    await userEvent.click(await screen.findByRole("button", { name: /○ conta-b/ }));
    expect(escolher).toHaveBeenCalledWith("conta-b");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("dá para voltar à conta ativa do gh", async () => {
    const escolher = rodape("conta-b");
    await userEvent.click(await screen.findByRole("button", { name: /conta-b/ }));
    await userEvent.click(await screen.findByRole("button", { name: /Conta ativa do gh/ }));
    expect(escolher).toHaveBeenCalledWith(null);
  });

  it("avisa quando a conta do workspace saiu do gh e quando não há contas", async () => {
    ghContas.mockResolvedValue([]);
    rodape("conta-b");
    await userEvent.click(await screen.findByRole("button", { name: /conta-b/ }));
    expect(await screen.findByText(/não está mais logada/)).toBeInTheDocument();
    expect(screen.getByText(/gh auth login/)).toBeInTheDocument();
  });
});

describe("SET_WORKSPACE_GITHUB", () => {
  const inicial = (): WorkspaceCollection => workspacesReducer(
    workspacesReducer({ version: 1, workspaces: [], activeWorkspaceId: null }, { type: "ADD_WORKSPACE", name: "Um" }),
    { type: "ADD_WORKSPACE", name: "Dois" }
  );

  it("grava a conta só no workspace pedido", () => {
    const antes = inicial();
    const [um, dois] = antes.workspaces;
    const depois = workspacesReducer(antes, { type: "SET_WORKSPACE_GITHUB", workspaceId: um.id, contaGithub: "conta-a" });
    expect(depois.workspaces[0].contaGithub).toBe("conta-a");
    expect(depois.workspaces[1]).toBe(dois);
    expect(antes.workspaces[0].contaGithub).toBeUndefined();
  });

  it("null volta para a conta ativa do gh", () => {
    const antes = inicial();
    const id = antes.workspaces[0].id;
    const com = workspacesReducer(antes, { type: "SET_WORKSPACE_GITHUB", workspaceId: id, contaGithub: "conta-a" });
    expect(workspacesReducer(com, { type: "SET_WORKSPACE_GITHUB", workspaceId: id, contaGithub: null }).workspaces[0].contaGithub).toBeNull();
  });
});
