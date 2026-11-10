import { describe, expect, it } from "vitest";
import { FALA_SEM_SERVICO, motivoDaFala, motivoDoMicrofone, preferirVozDoPc } from "../mobile/voz";

const erro = (name: string) => new DOMException("x", name);

describe("por que o microfone não ligou", () => {
  it("permissão negada no navegador diz onde liberar", () => {
    const texto = motivoDoMicrofone(erro("NotAllowedError"), { seguro: true, noApp: false });
    expect(texto).toMatch(/cadeado/);
    expect(texto).not.toMatch(/Error/);
  });

  it("permissão negada no app manda olhar a privacidade do sistema", () => {
    expect(motivoDoMicrofone(erro("NotAllowedError"), { seguro: true, noApp: true })).toMatch(/sistema/);
  });

  it("sem microfone conectado", () => {
    expect(motivoDoMicrofone(erro("NotFoundError"), { seguro: true, noApp: false })).toMatch(/Nenhum microfone/);
  });

  it("microfone em uso por outro programa", () => {
    expect(motivoDoMicrofone(erro("NotReadableError"), { seguro: true, noApp: false })).toMatch(/outro programa/);
  });

  it("endereço http: só https libera o microfone", () => {
    const texto = motivoDoMicrofone(new TypeError("mediaDevices is undefined"), { seguro: false, noApp: false });
    expect(texto).toMatch(/https/);
    expect(texto).toMatch(/Acesso remoto/);
  });

  it("falha desconhecida não culpa a permissão", () => {
    expect(motivoDoMicrofone(new Error("?"), { seguro: true, noApp: false })).not.toMatch(/permiss/i);
  });
});

describe("por que a fala do navegador parou", () => {
  it("permissão negada", () => {
    expect(motivoDaFala("not-allowed")).toMatch(/cadeado/);
  });
  it("sem o serviço de fala (Brave) manda baixar o modelo de voz", () => {
    expect(motivoDaFala("network")).toBe(FALA_SEM_SERVICO);
    expect(motivoDaFala("service-not-allowed")).toBe(FALA_SEM_SERVICO);
    expect(FALA_SEM_SERVICO).toMatch(/modelo de voz/);
    expect(FALA_SEM_SERVICO).toMatch(/Configurações/);
  });
  it("sem microfone", () => {
    expect(motivoDaFala("audio-capture")).toMatch(/microfone/);
  });
  it("silêncio e cancelamento não são erro", () => {
    expect(motivoDaFala("no-speech")).toBeNull();
    expect(motivoDaFala("aborted")).toBeNull();
  });
});

describe("qual microfone usar", () => {
  it("Brave com o modelo baixado grava pela voz do PC (não tem o serviço do Google)", () => {
    expect(preferirVozDoPc({ brave: true, semServico: false, vozDoPc: true })).toBe(true);
  });
  it("Brave sem o modelo fica com a fala do navegador (que explica o que fazer)", () => {
    expect(preferirVozDoPc({ brave: true, semServico: false, vozDoPc: false })).toBe(false);
  });
  it("Chrome usa a fala do navegador até ela falhar", () => {
    expect(preferirVozDoPc({ brave: false, semServico: false, vozDoPc: true })).toBe(false);
    expect(preferirVozDoPc({ brave: false, semServico: true, vozDoPc: true })).toBe(true);
  });
});

describe("microfone no Composer", () => {
  it("endereço http com a voz do PC: o botão explica que precisa de https", async () => {
    const { render, screen, act } = await import("@testing-library/react");
    const userEvent = (await import("@testing-library/user-event")).default;
    const { Composer } = await import("../mobile/Composer");
    const seguro = Object.getOwnPropertyDescriptor(window, "isSecureContext");
    Object.defineProperty(window, "isSecureContext", { configurable: true, value: false });
    try {
      render(<Composer texto="" onTexto={() => undefined} anexos={[]} onAdicionar={() => undefined} onRemover={() => undefined}
        onEnviar={() => undefined} podeEnviar ocupado={false} placeholder="msg"
        voz={{ transcrever: async () => "", corrigir: async (t) => t }} />);
      await act(async () => { await userEvent.click(screen.getByRole("button", { name: "Falar o prompt" })); });
      expect(screen.getByRole("status")).toHaveTextContent(/https/);
    } finally {
      if (seguro) Object.defineProperty(window, "isSecureContext", seguro); else delete (window as { isSecureContext?: boolean }).isSecureContext;
    }
  });
});
