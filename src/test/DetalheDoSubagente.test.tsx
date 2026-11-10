import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { DetalheDoSubagente } from "../features/segundoPlano/DetalheDoSubagente";

vi.mock("../features/terminal/terminalService", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../features/terminal/terminalService")>()),
  engineApi: vi.fn(),
}));
const { engineApi } = vi.mocked(await import("../features/terminal/terminalService"));
type Resposta = Awaited<ReturnType<typeof engineApi>>;

const AGORA = 1_800_000_000_000;
const PROMPT = "Revise o módulo de fluxos.\nListe os problemas.";
const detalhe = (extra: object = {}) => ({
  id: "a1", tipo: "Explore", descricao: "Revisar fluxos", estado: "rodando", ultima_acao: "Read /p/fluxos.rs",
  atualizado_ms: AGORA - 1000, modelo: "claude-sonnet-5-5", desde_ms: AGORA - 5 * 60_000, prompt: PROMPT, resultado: null, ...extra,
});
const msg = (n: number, extra: object) => ({ id: `c:${n}`, role: "assistant", text: "", provider: "claude", timestamp: null, kind: "text", ...extra });
const corpo = (d: object, mensagens = [msg(0, { role: "user", text: PROMPT }), msg(1, { kind: "tool", tool: "Read", text: "/p/fluxos.rs" }),
  msg(2, { text: "Achei dois problemas." })]) =>
  ({ status: 200, body: JSON.stringify({ subagente: d, timeline: { messages: mensagens, next_cursor: null, prev_cursor: null, unavailable_segments: [] } }) }) as Resposta;

function rotasChamadas() { return engineApi.mock.calls.map((c) => `${c[0]} ${c[1]}`); }

beforeEach(() => { engineApi.mockReset(); });
afterEach(() => vi.useRealTimers());

it("mostra tipo, modelo, estado, pedido inteiro, ações com o alvo e falas", async () => {
  engineApi.mockResolvedValue(corpo(detalhe()));
  render(<DetalheDoSubagente conversaId="c1" agenteId="a1" agora={AGORA} onAbrirConversa={vi.fn()} />);
  expect(await screen.findByText("Explore")).toBeInTheDocument();
  expect(screen.getByText(/claude-sonnet-5-5/)).toBeInTheDocument();
  expect(screen.getByText("Rodando")).toBeInTheDocument();
  expect(screen.getByText(/começou há 5 min/)).toBeInTheDocument();
  // Pedido curto: aberto por padrão, com as quebras de linha.
  const pedido = screen.getByText("Pedido recebido").closest("details")!;
  expect(pedido).toHaveAttribute("open");
  expect(pedido.querySelector("pre")?.textContent).toBe(PROMPT);
  expect(screen.getByText("/p/fluxos.rs")).toBeInTheDocument();
  expect(screen.getByText("Achei dois problemas.")).toBeInTheDocument();
  // O pedido não se repete na linha do tempo.
  expect(screen.getAllByText(/Revise o módulo de fluxos/)).toHaveLength(1);
  expect(rotasChamadas()).toEqual(["GET /conversas/c1/subagentes/a1"]);
});

it("pedido longo começa recolhido", async () => {
  engineApi.mockResolvedValue(corpo(detalhe({ prompt: "x".repeat(2000) }), []));
  render(<DetalheDoSubagente conversaId="c1" agenteId="a1" agora={AGORA} onAbrirConversa={vi.fn()} />);
  const pedido = (await screen.findByText("Pedido recebido")).closest("details")!;
  expect(pedido).not.toHaveAttribute("open");
});

it("concluído mostra o resultado e não fica consultando", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  engineApi.mockResolvedValue(corpo(detalhe({ estado: "concluido", resultado: "Tudo certo: 3 problemas." })));
  render(<DetalheDoSubagente conversaId="c1" agenteId="a1" agora={AGORA} onAbrirConversa={vi.fn()} />);
  expect(await screen.findByText("Tudo certo: 3 problemas.")).toBeInTheDocument();
  expect(screen.getByText("Concluído")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Parar este subagente" })).not.toBeInTheDocument();
  await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
  expect(engineApi).toHaveBeenCalledTimes(1);
});

it("rodando, relê a cada 3 s", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  engineApi.mockResolvedValue(corpo(detalhe()));
  render(<DetalheDoSubagente conversaId="c1" agenteId="a1" agora={AGORA} onAbrirConversa={vi.fn()} />);
  await screen.findByText("Explore");
  await act(async () => { await vi.advanceTimersByTimeAsync(3_100); });
  expect(engineApi).toHaveBeenCalledTimes(2);
});

it("Parar este subagente usa a rota de parar só dele", async () => {
  engineApi.mockImplementation(async (metodo) => (metodo === "POST" ? { status: 200, body: JSON.stringify({ ok: true, na_tela: true }) } as Resposta : corpo(detalhe())));
  render(<DetalheDoSubagente conversaId="c1" agenteId="a1" agora={AGORA} onAbrirConversa={vi.fn()} />);
  await userEvent.click(await screen.findByRole("button", { name: "Parar este subagente" }));
  await waitFor(() => expect(rotasChamadas()).toContain("POST /conversas/c1/subagentes/a1/parar"));
  expect(rotasChamadas().some((r) => r.endsWith("/prompt"))).toBe(false);
});

it("sem achar a linha na tela, pede ao agente principal que pare só ele", async () => {
  engineApi.mockImplementation(async (metodo) => (metodo === "POST" ? { status: 200, body: JSON.stringify({ ok: true, na_tela: false }) } as Resposta : corpo(detalhe())));
  render(<DetalheDoSubagente conversaId="c1" agenteId="a1" agora={AGORA} onAbrirConversa={vi.fn()} />);
  await userEvent.click(await screen.findByRole("button", { name: "Parar este subagente" }));
  await waitFor(() => expect(rotasChamadas()).toContain("POST /conversas/c1/prompt"));
  const pedido = engineApi.mock.calls.find((c) => String(c[1]).endsWith("/prompt"))!;
  expect(JSON.parse(pedido[2] as string).texto).toMatch(/TaskStop/);
  expect(await screen.findByText(/Pedimos ao agente principal/)).toBeInTheDocument();
});

it("Abrir conversa e falha de leitura", async () => {
  const onAbrirConversa = vi.fn();
  engineApi.mockResolvedValue({ status: 404, body: JSON.stringify({ error: "Subagente não encontrado" }) } as Resposta);
  render(<DetalheDoSubagente conversaId="c1" agenteId="a1" agora={AGORA} onAbrirConversa={onAbrirConversa} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("Subagente não encontrado");
  await userEvent.click(screen.getByRole("button", { name: "Abrir conversa" }));
  expect(onAbrirConversa).toHaveBeenCalledWith("c1");
});
