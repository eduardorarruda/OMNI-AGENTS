import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { LinkSeguro, type EstadoDoTunel } from "../features/mobile/LinkSeguro";

const invoke = vi.mocked((await import("@tauri-apps/api/core")).invoke);

function estado(extra: Partial<EstadoDoTunel> = {}, cf: Partial<EstadoDoTunel["cloudflared"]> = {}): EstadoDoTunel {
  return {
    ligado: false, url: null, qr: null, erro: null, totp_confirmed: true, ...extra,
    cloudflared: { instalado: true, caminho: "/usr/bin/cloudflared", pode_baixar: true, instalando: false, progresso: null,
      erro: null, comando: "sudo pacman -S cloudflared", versao: "2026.10.7", ...cf },
  };
}

/** Responde `engine_api` por rota; o resto do app não entra nesta tela. */
function engine(rotas: Record<string, EstadoDoTunel | { status: number; error: string }>) {
  invoke.mockImplementation(async (comando: string, args?: Record<string, unknown>) => {
    if (comando !== "engine_api") return null;
    const resposta = rotas[`${args?.method} ${args?.path}`];
    if (!resposta) return { type: "api", status: 404, body: "{}" };
    if ("error" in resposta && "status" in resposta) return { type: "api", status: resposta.status, body: JSON.stringify({ error: resposta.error }) };
    return { type: "api", status: 200, body: JSON.stringify(resposta) };
  });
}

const tela = (onConfigurarAuthy = vi.fn()) => render(<LinkSeguro totpConfirmado corQr="#123456" onConfigurarAuthy={onConfigurarAuthy} />);

describe("link seguro (túnel da Cloudflare)", () => {
  beforeEach(() => invoke.mockReset());

  it("sem Authy, pede o cadastro antes e não oferece Ligar", async () => {
    engine({ "GET /tunel": estado({ totp_confirmed: false }) });
    const configurar = vi.fn();
    tela(configurar);
    const botao = await screen.findByRole("button", { name: "Proteger com o Authy primeiro" });
    expect(screen.queryByRole("button", { name: "Ligar" })).not.toBeInTheDocument();
    await act(async () => { await userEvent.click(botao); });
    expect(configurar).toHaveBeenCalled();
  });

  it("sem cloudflared, oferece instalar e mostra o comando do pacman", async () => {
    engine({
      "GET /tunel": estado({}, { instalado: false, caminho: null }),
      "POST /tunel/instalar": estado({}, { instalado: false, caminho: null, instalando: true, progresso: 0 }),
    });
    tela();
    const instalar = await screen.findByRole("button", { name: "Instalar o cloudflared" });
    expect(screen.getByText("sudo pacman -S cloudflared")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Ligar" })).not.toBeInTheDocument();
    await act(async () => { await userEvent.click(instalar); });
    expect(invoke).toHaveBeenCalledWith("engine_api", expect.objectContaining({ method: "POST", path: "/tunel/instalar" }));
    expect(await screen.findByRole("progressbar", { name: "Baixando o cloudflared" })).toBeInTheDocument();
  });

  it("instalando, mostra o progresso", async () => {
    engine({ "GET /tunel": estado({}, { instalado: false, instalando: true, progresso: 42 }) });
    tela();
    expect(await screen.findByText(/Baixando o cloudflared… 42%/)).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "42");
    expect(screen.queryByRole("button", { name: "Instalar o cloudflared" })).not.toBeInTheDocument();
  });

  it("com tudo pronto, Ligar liga e mostra o QR, o link e o aviso do endereço", async () => {
    const url = "https://palavras-de-teste.trycloudflare.com";
    engine({
      "GET /tunel": estado(),
      "POST /tunel/ligar": estado({ ligado: true, url, qr: `${url}/` }),
    });
    tela();
    const ligar = await screen.findByRole("button", { name: "Ligar" });
    await act(async () => { await userEvent.click(ligar); });
    expect(await screen.findByRole("img", { name: "Código QR do link seguro" })).toBeInTheDocument();
    expect(screen.getByText(url)).toBeInTheDocument();
    expect(screen.getByText(/O endereço muda quando o PC reinicia; leia o QR de novo/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copiar link" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Desligar" })).toBeInTheDocument();
  });

  it("mostra a recusa do engine em vez de fingir que ligou", async () => {
    engine({ "GET /tunel": estado(), "POST /tunel/ligar": { status: 409, error: "Configure o Authy primeiro: sem ele o link seguro não liga." } });
    tela();
    const ligar = await screen.findByRole("button", { name: "Ligar" });
    await act(async () => { await userEvent.click(ligar); });
    expect(await screen.findByRole("alert")).toHaveTextContent("Configure o Authy primeiro");
  });
});
