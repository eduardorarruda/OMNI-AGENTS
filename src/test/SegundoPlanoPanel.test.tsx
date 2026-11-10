import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { SegundoPlanoPanel } from "../features/segundoPlano/SegundoPlanoPanel";
import type { ItemDeFundo } from "../features/segundoPlano/useSegundoPlano";

vi.mock("../features/segundoPlano/useSegundoPlano", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../features/segundoPlano/useSegundoPlano")>()),
  useSegundoPlano: vi.fn(),
}));
vi.mock("../features/terminal/terminalService", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../features/terminal/terminalService")>()),
  pararConversa: vi.fn(),
}));
vi.mock("../features/fluxos/fluxosService", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../features/fluxos/fluxosService")>()),
  acaoNaExecucao: vi.fn(),
}));

vi.mock("../features/segundoPlano/DetalheDoSubagente", () => ({
  DetalheDoSubagente: ({ conversaId, agenteId }: { conversaId: string; agenteId: string }) => <p>detalhe de {agenteId} em {conversaId}</p>,
}));

const { useSegundoPlano } = vi.mocked(await import("../features/segundoPlano/useSegundoPlano"));
const { pararConversa } = vi.mocked(await import("../features/terminal/terminalService"));
const { acaoNaExecucao } = vi.mocked(await import("../features/fluxos/fluxosService"));

const AGORA = 1_800_000_000_000;
const item = (extra: Partial<ItemDeFundo>): ItemDeFundo =>
  ({ tipo: "subagente", titulo: "Revisar fluxos", atividade: "Read src/fluxos.rs", desde_ms: AGORA - 5 * 60_000, projeto_id: "p", sessao: "c1", execucao: null, ...extra });

function abrir(itens: ItemDeFundo[], erro: string | null = null, concluidos: ItemDeFundo[] = []) {
  useSegundoPlano.mockReturnValue({ itens, concluidos, erro });
  const onAbrirConversa = vi.fn();
  const onAbrirFluxo = vi.fn();
  render(<SegundoPlanoPanel projetoId="p" agora={AGORA} onAbrirConversa={onAbrirConversa} onAbrirFluxo={onAbrirFluxo} />);
  return { onAbrirConversa, onAbrirFluxo };
}

beforeEach(() => {
  vi.clearAllMocks();
  pararConversa.mockResolvedValue(undefined);
  acaoNaExecucao.mockResolvedValue({ ok: true });
});

it("lista vazia explica que nada está rodando", () => {
  abrir([]);
  expect(screen.getByText(/Nada rodando em segundo plano/)).toBeInTheDocument();
  expect(screen.getByText(/subagentes, agentes em segundo plano e fluxos/)).toBeInTheDocument();
});

it("cada linha mostra o que faz e desde quando", () => {
  abrir([item({}), item({ tipo: "fluxo", titulo: "Revisão de PR", atividade: "Rodando", sessao: null, execucao: "e1", desde_ms: AGORA - 90_000 })]);
  const linha = screen.getByRole("listitem", { name: /Revisar fluxos/ });
  expect(within(linha).getByText("Read src/fluxos.rs")).toBeInTheDocument();
  expect(within(linha).getByText("há 5 min")).toBeInTheDocument();
  expect(within(linha).getByText("Subagente")).toBeInTheDocument();
  expect(screen.getByText("Fluxo")).toBeInTheDocument();
});

it("Abrir leva à conversa", async () => {
  const { onAbrirConversa, onAbrirFluxo } = abrir([item({}), item({ tipo: "fluxo", titulo: "Revisão de PR", sessao: null, execucao: "e1" })]);
  await userEvent.click(within(screen.getByRole("listitem", { name: /Revisar fluxos/ })).getByRole("button", { name: "Abrir" }));
  expect(onAbrirConversa).toHaveBeenCalledWith("c1");
  await userEvent.click(within(screen.getByRole("listitem", { name: /Revisão de PR/ })).getByRole("button", { name: "Abrir" }));
  expect(onAbrirFluxo).toHaveBeenCalledWith("e1");
});

it("Parar de subagente diz que para a sessão inteira", async () => {
  abrir([item({})]);
  const linha = screen.getByRole("listitem", { name: /Revisar fluxos/ });
  expect(within(linha).getByText(/para a sessão inteira, não só este subagente/i)).toBeInTheDocument();
  await userEvent.click(within(linha).getByRole("button", { name: "Parar a sessão inteira" }));
  await waitFor(() => expect(pararConversa).toHaveBeenCalledWith("c1"));
});

it("Parar de fluxo cancela a execução", async () => {
  abrir([item({ tipo: "fluxo", titulo: "Revisão de PR", sessao: null, execucao: "e1" })]);
  await userEvent.click(screen.getByRole("button", { name: "Parar" }));
  await waitFor(() => expect(acaoNaExecucao).toHaveBeenCalledWith("e1", "cancelar"));
});

it("falha ao parar aparece na linha, não some", async () => {
  pararConversa.mockRejectedValue(new Error("Sessão não está aberta no PC"));
  abrir([item({})]);
  await userEvent.click(screen.getByRole("button", { name: "Parar a sessão inteira" }));
  expect(await screen.findByText(/Sessão não está aberta no PC/)).toBeInTheDocument();
});

it("engine fora do ar mostra aviso, não lista vazia", () => {
  abrir([], "Falha HTTP 500");
  expect(screen.getByRole("alert")).toHaveTextContent(/Não foi possível consultar o que roda em segundo plano/);
  expect(screen.queryByText(/Nada rodando em segundo plano/)).not.toBeInTheDocument();
});

it("clicar no subagente abre o detalhe dele na própria linha", async () => {
  abrir([item({ subagente: "a1" })]);
  const linha = screen.getByRole("listitem", { name: /Revisar fluxos/ });
  expect(screen.queryByText(/detalhe de a1/)).not.toBeInTheDocument();
  await userEvent.click(within(linha).getByRole("button", { name: "Ver detalhes" }));
  expect(within(linha).getByText("detalhe de a1 em c1")).toBeInTheDocument();
  await userEvent.click(within(linha).getByRole("button", { name: "Fechar detalhes" }));
  expect(screen.queryByText(/detalhe de a1/)).not.toBeInTheDocument();
});

it("subagente sem id (engine antigo) não oferece detalhe", () => {
  abrir([item({})]);
  expect(screen.queryByRole("button", { name: "Ver detalhes" })).not.toBeInTheDocument();
});

it("concluídos ficam numa seção recolhida, com o resultado a um clique", async () => {
  abrir([], null, [item({ titulo: "Mapear rotas", atividade: "Concluído", subagente: "a2", concluido_ms: AGORA - 3 * 60_000 })]);
  expect(screen.queryByText(/Nada rodando em segundo plano/)).toBeInTheDocument();
  const secao = screen.getByText("Concluídos (1)").closest("details")!;
  expect(secao).not.toHaveAttribute("open");
  await userEvent.click(screen.getByText("Concluídos (1)"));
  const linha = within(secao).getByRole("listitem", { name: /Mapear rotas/ });
  expect(within(linha).getByText("terminou há 3 min")).toBeInTheDocument();
  await userEvent.click(within(linha).getByRole("button", { name: "Ver resultado" }));
  expect(within(linha).getByText("detalhe de a2 em c1")).toBeInTheDocument();
});
