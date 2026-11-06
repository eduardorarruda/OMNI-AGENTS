import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { DetalheDaConta } from "../features/terminal/DetalheDaConta";
import { AgentUsageStatus } from "../features/terminal/AgentUsageStatus";
import { UsoDeTodasAsContas } from "../features/terminal/UsoDeTodasAsContas";

const RENOVA_5H = Date.UTC(2026, 10, 6, 18, 0) / 1000;
const RENOVA_SEMANA = Date.UTC(2026, 10, 9, 12, 0) / 1000;
const completo = {
  status: "available", observed_at_ms: Date.now(), reason: null,
  primary: { used_percent: 42, resets_at: RENOVA_5H, reset_label: null },
  secondary: { used_percent: 88, resets_at: RENOVA_SEMANA, reset_label: null },
};
const conta = { nome: "Pessoal", email: "eu@exemplo.com", provider: "claude" };

function mockEngine(uso: unknown = completo) {
  vi.mocked(invoke).mockReset().mockImplementation(async (command) => {
    if (command === "list_profiles") return [{ id: "a", provider: "claude", name: "Pessoal", email: "eu@exemplo.com", last_used_at_ms: 1 }];
    if (command === "account_usage") return uso;
    return null;
  });
}

it("clicar no nome da conta abre o detalhe", async () => {
  mockEngine();
  render(<AgentUsageStatus engineOnline />);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  await userEvent.click(await screen.findByRole("button", { name: "Ver detalhe da conta Pessoal" }));
  const detalhe = await screen.findByRole("dialog", { name: "Detalhe da conta Pessoal" });
  expect(within(detalhe).getByText("eu@exemplo.com")).toBeInTheDocument();
});

it("mostra 5h e semana separados com a renovação", () => {
  render(<DetalheDaConta uso={completo} tokens={null} conta={conta} onFechar={() => undefined} />);
  const cinco = screen.getByRole("group", { name: "Janela de 5 horas" });
  const semana = screen.getByRole("group", { name: "Limite semanal" });
  expect(within(cinco).getByText("42%")).toBeInTheDocument();
  expect(within(semana).getByText("88%")).toBeInTheDocument();
  const hora = (s: number) => new Date(s * 1000).toLocaleString("pt-BR", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });
  expect(within(cinco).getByText(`Renova em ${hora(RENOVA_5H)}`)).toBeInTheDocument();
  expect(within(semana).getByText(`Renova em ${hora(RENOVA_SEMANA)}`)).toBeInTheDocument();
});

it("status unavailable mostra o motivo", () => {
  const uso = { status: "unavailable", observed_at_ms: null, primary: null, secondary: null, reason: "Faça login de novo na conta" };
  render(<DetalheDaConta uso={uso} tokens={null} conta={conta} onFechar={() => undefined} />);
  expect(screen.getByRole("alert")).toHaveTextContent("Faça login de novo na conta");
});

it('janela ausente mostra "não disponível", não 0%', () => {
  const uso = { ...completo, secondary: null };
  render(<DetalheDaConta uso={uso} tokens={null} conta={conta} onFechar={() => undefined} />);
  const semana = screen.getByRole("group", { name: "Limite semanal" });
  expect(within(semana).getByText("não disponível")).toBeInTheDocument();
  expect(within(semana).queryByText("0%")).not.toBeInTheDocument();
  expect(within(semana).queryByRole("progressbar")).not.toBeInTheDocument();
});

it("Esc e clique fora fecham", async () => {
  const fechar = vi.fn();
  render(<div><button>fora</button><DetalheDaConta uso={completo} tokens={null} conta={conta} onFechar={fechar} /></div>);
  await userEvent.keyboard("{Escape}");
  expect(fechar).toHaveBeenCalledTimes(1);
  await userEvent.click(screen.getByRole("button", { name: "fora" }));
  expect(fechar).toHaveBeenCalledTimes(2);
  await userEvent.click(screen.getByRole("dialog"));
  expect(fechar).toHaveBeenCalledTimes(2);
});

it("painel do topo usa o mesmo detalhe", async () => {
  mockEngine();
  render(<UsoDeTodasAsContas engineOnline contaAtiva={null} />);
  await userEvent.click(screen.getByRole("button", { name: "Uso de todas as contas" }));
  const painel = await screen.findByRole("dialog", { name: "Uso das contas" });
  await userEvent.click(await within(painel).findByRole("button", { name: "Ver detalhe da conta Pessoal" }));
  const detalhe = await screen.findByRole("dialog", { name: "Detalhe da conta Pessoal" });
  expect(within(detalhe).getByRole("group", { name: "Janela de 5 horas" })).toBeInTheDocument();
  await userEvent.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Detalhe da conta Pessoal" })).not.toBeInTheDocument());
  expect(screen.getByRole("dialog", { name: "Uso das contas" })).toBeInTheDocument();
});

it("rodapé busca os tokens da conta no engine e mostra \"não disponível\" onde a fonte não tem", async () => {
  vi.mocked(invoke).mockReset().mockImplementation(async (command, args) => {
    if (command === "list_profiles") return [{ id: "a", provider: "claude", name: "Pessoal", last_used_at_ms: 1 }];
    if (command === "account_usage") return completo;
    if (command === "engine_api") {
      expect((args as { path: string }).path).toBe("/contas/a/tokens");
      const corpo = { hoje: { entrada: 1200, saida: 340, cache: 5000 }, sete_dias: null, motivo: null };
      return { type: "api", status: 200, body: JSON.stringify(corpo) };
    }
    return null;
  });
  render(<AgentUsageStatus engineOnline />);
  await userEvent.click(await screen.findByRole("button", { name: "Ver detalhe da conta Pessoal" }));
  const detalhe = await screen.findByRole("dialog", { name: "Detalhe da conta Pessoal" });
  expect(await within(detalhe).findByText(/1\.200 entrada · 340 saída · 5\.000 cache/)).toBeInTheDocument();
  expect(within(detalhe).getByText("não disponível")).toBeInTheDocument();
});
